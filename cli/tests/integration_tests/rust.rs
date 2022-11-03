// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::common::get_free_port;
use crate::database::{Database, DatabaseConfig, PostgresDb, SqliteDb};
use crate::framework::{
    execute_async, wait_for_chiseld_startup, Chisel, GuardedChild, TestContext,
};
use crate::suite::{Modules, TestInstance, TestSuite};
use crate::Opt;
use colored::Colorize;
use enclose::enclose;
use futures::ready;
use futures::stream::{FuturesUnordered, StreamExt};
use rskafka::client::ClientBuilder;
use std::any::Any;
use std::future::Future;
use std::io::{stdout, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::AtomicU16;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use std::{env, panic};
use tempdir::TempDir;

#[derive(Clone, Debug)]
pub struct ChiseldConfig {
    pub api_address: SocketAddr,
    pub internal_address: SocketAddr,
    pub rpc_address: SocketAddr,
    pub kafka_connection: Option<String>,
}

fn generate_chiseld_config(
    ports_counter: &AtomicU16,
    kafka_connection: Option<String>,
) -> ChiseldConfig {
    let make_address = || SocketAddr::from((Ipv4Addr::LOCALHOST, get_free_port(ports_counter)));
    ChiseldConfig {
        api_address: make_address(),
        rpc_address: make_address(),
        internal_address: make_address(),
        kafka_connection,
    }
}

async fn setup_test_context(
    instance: &TestInstance,
    opt: &Opt,
    ports_counter: &AtomicU16,
) -> TestContext {
    let chiseld_config = generate_chiseld_config(ports_counter, opt.kafka_connection.to_owned());
    let tmp_dir = Arc::new(TempDir::new("chiseld_test").expect("Could not create tempdir"));
    let chisel_path = bin_dir().join("chisel");

    let optimize_str = format!("{}", instance.optimize);

    match instance.modules {
        Modules::Deno => {
            execute_async(
                tokio::process::Command::new(&chisel_path)
                    .args(&[
                        "init",
                        "--no-examples",
                        "--optimize",
                        &optimize_str,
                        "--auto-index",
                        &optimize_str,
                    ])
                    .current_dir(tmp_dir.path()),
            )
            .await
            .expect("chisel init failed");
        }
        Modules::Node => {
            let create_app_js = repo_dir().join("packages/create-chiselstrike-app/dist/index.js");
            execute_async(
                tokio::process::Command::new("node")
                    .arg(&create_app_js)
                    .args(["--chisel-version", "latest"])
                    .arg("--no-install")
                    .arg("./")
                    .current_dir(tmp_dir.path()),
            )
            .await
            .expect("create-chiselstrike-app failed");

            // instead of running `npm install` from `create-chiselstrike-app`, we simply create a
            // symlink of `node_modules` pointing to the "cache" directory, where we have
            // previously installed all dependencies
            let modules_cache = repo_dir()
                .join("cli/tests/integration_tests/cache/node_modules")
                .canonicalize()
                .unwrap();
            let modules_dir = tmp_dir.path().join("node_modules");
            std::os::unix::fs::symlink(&modules_cache, &modules_dir)
                .expect("could not create symlink for node_modules/");
        }
    };

    let chisel = Chisel {
        rpc_address: chiseld_config.rpc_address,
        api_address: chiseld_config.api_address,
        chisel_path,
        tmp_dir: tmp_dir.clone(),
        client: reqwest::Client::new(),
        capture: !opt.nocapture,
    };

    let mut args = vec![
        "--debug".to_string(),
        "--api-listen-addr".to_string(),
        chiseld_config.api_address.to_string(),
        "--internal-routes-listen-addr".to_string(),
        chiseld_config.internal_address.to_string(),
        "--rpc-listen-addr".to_string(),
        chiseld_config.rpc_address.to_string(),
    ];
    if let Some(kafka_connection) = chiseld_config.kafka_connection {
        args.push("--kafka-connection".to_string());
        args.push(kafka_connection);
    }
    // add user provided arguments
    args.extend(instance.spec.chiseld_args.iter().map(ToString::to_string));

    let mut cmd = tokio::process::Command::new(chiseld());
    cmd.args(args);
    cmd.current_dir(tmp_dir.path());

    let db = match &instance.db_config {
        DatabaseConfig::Postgres(config) => {
            let db = PostgresDb::new(config.clone());
            cmd.args(["--db-uri", &db.url()]);
            Database::Postgres(db)
        }
        DatabaseConfig::Sqlite => {
            let db = SqliteDb::new(tmp_dir.clone(), ".chiseld.db");
            cmd.args(["--db-uri", &db.url()]);
            Database::Sqlite(db)
        }
        DatabaseConfig::LegacySplitSqlite => {
            let meta = SqliteDb::new(tmp_dir.clone(), "chiseld-meta.db");
            let data = SqliteDb::new(tmp_dir.clone(), "chiseld-data.db");
            cmd.args([
                "--metadata-db-uri",
                &meta.url(),
                "--data-db-uri",
                &data.url(),
            ]);
            Database::LegacySplitSqlite { meta, data }
        }
    };

    let mut chiseld = GuardedChild::new(cmd, !opt.nocapture);
    if instance.spec.start_chiseld {
        chiseld.start().await;
        wait_for_chiseld_startup(&mut chiseld, &chisel).await;
    }

    let kafka_connection = opt.kafka_connection.to_owned();
    let mut kafka_topics = vec![];
    if let Some(ref kafka_connection) = kafka_connection {
        for idx in 0..instance.spec.kafka_topics {
            let prefix = instance.spec.name.replace(':', "_");
            let kafka_topic = format!("{}_topic_{}", prefix, idx);
            create_kafka_topic(kafka_connection, &kafka_topic).await;
            kafka_topics.push(kafka_topic);
        }
    }

    TestContext {
        chiseld,
        chisel,
        _db: db,
        kafka_connection,
        kafka_topics,
        optimized: instance.optimize,
    }
}

fn bin_dir() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path
}

fn repo_dir() -> PathBuf {
    let mut path = bin_dir();
    path.pop();
    path.pop();
    path
}

fn chiseld() -> String {
    bin_dir().join("chiseld").to_str().unwrap().to_string()
}

async fn create_kafka_topic(connection: &str, topic: &str) {
    let client = ClientBuilder::new(vec![connection.to_string()])
        .build()
        .await
        .unwrap();
    let controller_client = client.controller_client().unwrap();
    let result = controller_client
        .create_topic(
            topic, 1,     // partitions
            1,     // replication factor
            5_000, // timeout (ms)
        )
        .await;
    if let Err(err) = result {
        println!("Warning: failed to create topic `{}`: {}", topic, err);
    }
}

struct TestFuture {
    instance: Option<Arc<TestInstance>>,
    task: tokio::task::JoinHandle<Duration>,
}

struct TestResult {
    instance: Arc<TestInstance>,
    result: Result<Duration, Box<dyn Any + Send + 'static>>,
}

impl Future for TestFuture {
    type Output = TestResult;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();
        let result = ready!(Pin::new(&mut this.task).poll(cx)).map_err(|err| err.into_panic());
        let instance = this.instance.take().unwrap();
        Poll::Ready(TestResult { instance, result })
    }
}

fn format_test_instance(instance: &TestInstance) -> String {
    format!(
        "test {} ({:?}, optimize={})",
        instance.spec.name.bold(),
        instance.modules,
        instance.optimize,
    )
}

#[tokio::main]
pub(crate) async fn run_tests(opt: Arc<Opt>) -> bool {
    let suite = TestSuite::from_inventory();
    let ports_counter = Arc::new(AtomicU16::new(30000));
    let parallel = opt.parallel.unwrap_or_else(num_cpus::get);
    let is_parallel = parallel > 1;

    // By default, when a panic happens, the panic message is immediately written to stderr and
    // only then unwinding starts. However, we normally want to print the messages ourselves, to
    // make sure that messages from different tests running in parallel are not interleaved. This
    // can be accomplished by setting a custom panic hook, which simply does nothing.
    //
    // But when this hook is present, we cannot print the backtrace, so we keep the default hook
    // when `RUST_BACKTRACE` env is set. Also, when there is no parallelism, the messages cannot be
    // interleaved, so we also keep the default hook in this case.
    let setup_panic_hook = env::var_os("RUST_BACKTRACE").is_none() && is_parallel;
    if setup_panic_hook {
        panic::set_hook(Box::new(|_| {}));
    }

    let mut ok = true;
    let mut futures = FuturesUnordered::new();
    let mut instances = suite.instantiate(&opt);
    instances.reverse();

    while !instances.is_empty() || !futures.is_empty() {
        if !instances.is_empty() && futures.len() < parallel {
            let instance = Arc::new(instances.pop().unwrap());
            let future = enclose! {(instance, opt, ports_counter) async move {
                let start = Instant::now();
                let ctx = setup_test_context(&instance, &opt, &ports_counter).await;
                instance.spec.test_fn.call(ctx).await;
                Instant::now().duration_since(start)
            }};
            let task = tokio::task::spawn(future);

            if !is_parallel {
                print!("{} ... ", format_test_instance(&instance));
                stdout().flush().unwrap();
            }

            futures.push(TestFuture {
                instance: Some(instance),
                task,
            });
            continue;
        }

        assert!(!futures.is_empty());
        let TestResult { instance, result } = futures.next().await.unwrap();

        if is_parallel {
            print!("{}: ", format_test_instance(&instance));
        }

        match result {
            Ok(duration) => println!("{} in {:.2} s", "PASSED".green(), duration.as_secs_f64()),
            Err(panic) => {
                let panic_msg = if let Some(&text) = panic.downcast_ref::<&'static str>() {
                    text
                } else if let Some(text) = panic.downcast_ref::<String>() {
                    text.as_str()
                } else {
                    "(unknown panic error)"
                };

                if setup_panic_hook {
                    println!(
                        "{}\n{}",
                        "FAILED".red(),
                        textwrap::indent(panic_msg, "    ")
                    );
                } else {
                    // when we have not set up our hook, the panic message has already been
                    // printed, so there is no need to print it again
                    println!("{}\n", "FAILED".red());
                }

                ok = false;
            }
        }
    }

    if !ok {
        println!("{}", "Some tests have failed".red());
        if setup_panic_hook {
            println!("Consider running this test with RUST_BACKTRACE=1 and -p1 to help you with debugging.");
        }
    }

    ok
}
