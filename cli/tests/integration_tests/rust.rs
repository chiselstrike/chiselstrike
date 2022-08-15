// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::database::{generate_database_config, Database, DatabaseConfig, PostgresDb, SqliteDb};
use crate::framework::{execute_async, Chisel, GuardedChild, TestContext};
use crate::suite::{Modules, TestInstance};
use crate::{rust_tests, Opt};
use colored::Colorize;
use enclose::enclose;
use futures::ready;
use futures::stream::{FuturesUnordered, StreamExt};
use std::any::Any;
use std::future::Future;
use std::io::{stdout, Write};
use std::marker::Unpin;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{env, panic};
use tempdir::TempDir;

fn get_free_port(ports_counter: &AtomicU16) -> u16 {
    for _ in 0..10000 {
        let port = ports_counter.fetch_add(1, Ordering::Relaxed);
        if port_scanner::local_port_available(port) {
            return port;
        }
    }
    panic!("failed to find free port in 10000 iterations");
}

#[derive(Clone, Debug)]
pub struct ChiseldConfig {
    pub api_address: SocketAddr,
    pub internal_address: SocketAddr,
    pub rpc_address: SocketAddr,
}

fn generate_chiseld_config(ports_counter: &AtomicU16) -> ChiseldConfig {
    let make_address = || SocketAddr::from((Ipv4Addr::LOCALHOST, get_free_port(ports_counter)));
    ChiseldConfig {
        api_address: make_address(),
        rpc_address: make_address(),
        internal_address: make_address(),
    }
}

async fn setup_test_context(
    opt: &Opt,
    ports_counter: &AtomicU16,
    instance: &TestInstance,
) -> TestContext {
    let db_config = generate_database_config(opt);
    let chiseld_config = generate_chiseld_config(ports_counter);
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
                    .current_dir(&*tmp_dir),
            )
            .await
            .expect("chisel init failed");
        }
        Modules::Node => {
            let create_app_js = repo_dir().join("packages/create-chiselstrike-app/dist/index.js");
            execute_async(
                tokio::process::Command::new("node")
                    .arg(&create_app_js)
                    .args(["--chisel-version", "latest", "./"])
                    .current_dir(&*tmp_dir),
            )
            .await
            .expect("create-chiselstrike-app failed");
        }
    };

    let chisel = Chisel {
        rpc_address: chiseld_config.rpc_address,
        api_address: chiseld_config.api_address,
        chisel_path,
        tmp_dir: tmp_dir.clone(),
        client: reqwest::Client::new(),
    };

    let db: Database = match db_config {
        DatabaseConfig::Postgres(config) => Database::Postgres(PostgresDb::new(config)),
        DatabaseConfig::Sqlite => Database::Sqlite(SqliteDb { tmp_dir }),
    };

    let mut chiseld = GuardedChild::new(tokio::process::Command::new(chiseld()).args([
        "--webui",
        "--db-uri",
        db.url().as_str(),
        "--api-listen-addr",
        &chiseld_config.api_address.to_string(),
        "--internal-routes-listen-addr",
        &chiseld_config.internal_address.to_string(),
        "--rpc-listen-addr",
        &chiseld_config.rpc_address.to_string(),
    ]));

    tokio::select! {
        exit_status = chiseld.wait() => {
            chiseld.show_output().await;
            panic!("chiseld prematurely exited with {}", exit_status);
        },
        res = chisel.wait() => {
            res.expect("failed to start up chiseld");
        },
    }

    TestContext {
        chiseld,
        chisel,
        _db: db,
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

struct TestFuture {
    instance: Option<Arc<TestInstance>>,
    task: tokio::task::JoinHandle<()>,
}

struct TestResult {
    instance: Arc<TestInstance>,
    result: Result<(), Box<dyn Any + Send + 'static>>,
}

impl Unpin for TestFuture {}

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
    let suite = rust_tests::suite();
    let ports_counter = Arc::new(AtomicU16::new(30000));
    let parallel = opt.parallel.unwrap_or(num_cpus::get());

    // By default, when a panic happens, the panic message is immediately written to stderr and
    // only then unwinding starts. However, we normally want to print the messages ourselves, to
    // make sure that messages from different tests running in parallel are not interleaved. This
    // can be accomplished by setting a custom panic hook, which simply does nothing.
    //
    // But when this hook is present, we cannot print the backtrace, so we keep the default hook
    // when `RUST_BACKTRACE` env is set. Also, when there is no parallelism, the messages cannot be
    // interleaved, so we also keep the default hook in this case.
    let setup_panic_hook = !env::var_os("RUST_BACKTRACE").is_some() && parallel > 1;

    let mut ok = true;
    let mut futures = FuturesUnordered::new();
    let mut instances = suite.instantiate(&opt);
    instances.reverse();
    let mut printed_pending_instance = None;

    while !instances.is_empty() || !futures.is_empty() {
        if !instances.is_empty() && futures.len() < parallel {
            let instance = Arc::new(instances.pop().unwrap());
            let future = enclose! {(instance, opt, ports_counter) async move {
                if setup_panic_hook {
                    panic::set_hook(Box::new(|_| {}));
                }
                let ctx = setup_test_context(&opt, &ports_counter, &instance).await;
                instance.spec.test_fn.call(ctx).await;
            }};
            let task = tokio::task::spawn(future);

            if printed_pending_instance.is_some() {
                println!();
            }
            print!("{} ... ", format_test_instance(&instance));
            stdout().flush().unwrap();
            printed_pending_instance = Some(instance.clone());

            futures.push(TestFuture {
                instance: Some(instance),
                task,
            });
            continue;
        }

        assert!(!futures.is_empty());
        let TestResult { instance, result } = futures.next().await.unwrap();

        match printed_pending_instance {
            Some(pending) if Arc::ptr_eq(&pending, &instance) => {}
            Some(_) => print!("\n{} ... ", format_test_instance(&instance)),
            None => print!("{} ... ", format_test_instance(&instance)),
        }
        printed_pending_instance = None;

        match result {
            Ok(_) => println!("{}", "PASSED".green()),
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
            println!("Consider running this test with RUST_BACKTRACE=1 or -p1 to help you with debugging.");
        }
    }

    ok
}
