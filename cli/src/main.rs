// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::chisel::StatusResponse;
use crate::cmd::apply::{apply, AllowTypeDeletion, TypeChecking};
use crate::project::{create_project, read_manifest, CreateProjectOptions};
use anyhow::{anyhow, Result};
use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::{
    ChiselDeleteRequest, DescribeRequest, PopulateRequest, RestartRequest, StatusRequest,
};
use futures::channel::mpsc::channel;
use futures::{SinkExt, StreamExt};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::env;
use std::fs;
use std::future::Future;
use std::io::ErrorKind;
use std::path::Path;
use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use tonic::transport::Channel;

mod chisel;
mod cmd;
mod project;
mod ts;

// Timeout when waiting for connection or server status.
const TIMEOUT: Duration = Duration::from_secs(10);

fn parse_version(version: &str) -> anyhow::Result<String> {
    anyhow::ensure!(!version.is_empty(), "version name can't be empty");
    Ok(version.to_string())
}

static DEFAULT_API_VERSION: &str = "dev";

#[derive(StructOpt, Debug)]
#[structopt(name = "chisel", version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT"))]
struct Opt {
    /// RPC server address.
    #[structopt(short, long, default_value = "http://localhost:50051")]
    rpc_addr: String,
    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(StructOpt, Debug)]
enum Command {
    /// Create a new ChiselStrike project in current directory.
    Init {
        /// Force project initialization by overwriting files if needed.
        #[structopt(long)]
        force: bool,
        /// Skip generating example code.
        #[structopt(long)]
        no_examples: bool,
    },
    /// Describe the endpoints, types, and policies.
    Describe,
    /// Start a ChiselStrike server for local development.
    Dev {
        /// calls tsc --noEmit to check types. Useful if your IDE isn't doing it.
        #[structopt(long)]
        type_check: bool,
    },
    /// Create a new ChiselStrike project.
    New {
        /// Path where to create the project.
        path: String,
        /// Skip generating example code.
        #[structopt(long)]
        no_examples: bool,
    },
    /// Start the ChiselStrike server.
    Start,
    /// Show ChiselStrike server status.
    Status,
    /// Restart the running ChiselStrike server.
    Restart,
    /// Wait for the ChiselStrike server to start.
    Wait,
    /// Apply configuration to the ChiselStrike server.
    Apply {
        #[structopt(long)]
        allow_type_deletion: bool,
        #[structopt(long, default_value = DEFAULT_API_VERSION, parse(try_from_str=parse_version))]
        version: String,
        /// calls tsc --noEmit to check types. Useful if your IDE isn't doing it.
        #[structopt(long)]
        type_check: bool,
    },
    /// Delete configuration from the ChiselStrike server.
    Delete {
        #[structopt(long, default_value = DEFAULT_API_VERSION, parse(try_from_str=parse_version))]
        version: String,
    },
    Populate {
        #[structopt(long)]
        version: String,
        #[structopt(long)]
        from: String,
    },
}

// Retry calling 'f(a)' until it succeeds. This uses an exponential
// backoff and gives up once the timeout has passed. On failure 'f'
// must return an 'A' that we feed to the next retry.  (This can be
// the same 'a' passed to it -- an idiomatic way to satisfy lifetime
// constraints.)
async fn with_retry<A, T, F, Fut>(timeout: Duration, mut a: A, mut f: F) -> Result<T>
where
    Fut: Future<Output = Result<T, A>>,
    F: FnMut(A) -> Fut,
{
    let mut wait_time = Duration::from_millis(1);
    let mut total = Duration::from_millis(0);
    loop {
        match f(a).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                a = e;
                if total > timeout {
                    anyhow::bail!("Timeout");
                }
                thread::sleep(wait_time);
                total += wait_time;
                wait_time *= 2;
            }
        }
    }
}

async fn connect_with_retry(server_url: String) -> Result<ChiselRpcClient<Channel>> {
    with_retry(TIMEOUT, (), |_| async {
        let c = ChiselRpcClient::connect(server_url.clone()).await;
        c.map_err(|_| ())
    })
    .await
}

async fn wait(server_url: String) -> Result<tonic::Response<StatusResponse>> {
    let client = connect_with_retry(server_url).await?;
    with_retry(TIMEOUT, client, |mut client| async {
        let request = tonic::Request::new(StatusRequest {});
        let s = client.get_status(request).await;
        s.map_err(|_| client)
    })
    .await
}

fn start_server() -> anyhow::Result<std::process::Child> {
    println!("🙇‍♂️ Thank you for your interest in the ChiselStrike private beta! (Beta-Jan22.2)");
    println!("⚠️  This is provided to you for evaluation purposes and should not be used to host production at this time");
    println!("Docs with a description of expected functionality and command references at https://docs.chiselstrike.com");
    println!("For any question, concerns, or early feedback, contact us at beta@chiselstrike.com");
    println!("\n 🍾 We hope you have a great 2022! 🥂\n");

    let mut cmd = std::env::current_exe()?;
    cmd.pop();
    cmd.push("chiseld");
    let server = match std::process::Command::new(cmd.clone()).spawn() {
        Ok(server) => server,
        Err(e) => {
            match e.kind() {
                ErrorKind::NotFound => anyhow::bail!("Unable to start the server because `chiseld` program is missing. Please make sure `chiseld` is installed in {}", cmd.display()),
                _ => anyhow::bail!("Unable to start `chiseld` program: {}", e),
            }
        }
    };
    Ok(server)
}

async fn delete<S: ToString>(server_url: String, version: S) -> Result<()> {
    let version = version.to_string();
    let mut client = ChiselRpcClient::connect(server_url).await?;

    let msg = execute!(
        client
            .delete(tonic::Request::new(ChiselDeleteRequest { version }))
            .await
    );
    println!("{}", msg.result);
    Ok(())
}

async fn apply_from_dev(server_url: String, type_check: TypeChecking) {
    if let Err(e) = apply(
        server_url,
        DEFAULT_API_VERSION,
        AllowTypeDeletion::No,
        type_check,
    )
    .await
    {
        eprintln!("{:?}", e)
    }
}

async fn populate(server_url: String, to_version: String, from_version: String) -> Result<()> {
    let mut client = ChiselRpcClient::connect(server_url).await?;

    let msg = execute!(
        client
            .populate(tonic::Request::new(PopulateRequest {
                to_version,
                from_version,
            }))
            .await
    );
    println!("{}", msg.msg);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let server_url = opt.rpc_addr;
    match opt.cmd {
        Command::Init { force, no_examples } => {
            let cwd = env::current_dir()?;
            let opts = CreateProjectOptions {
                force,
                examples: !no_examples,
            };
            create_project(&cwd, opts)?;
        }
        Command::Describe => {
            let mut client = ChiselRpcClient::connect(server_url).await?;
            let request = tonic::Request::new(DescribeRequest {});
            let response = execute!(client.describe(request).await);

            for version_def in response.version_defs {
                println!("Version: {} {{", version_def.version);
                for def in &version_def.type_defs {
                    println!("  class {} {{", def.name);
                    for field in &def.field_defs {
                        let labels = if field.labels.is_empty() {
                            "".into()
                        } else {
                            let mut labels = field
                                .labels
                                .iter()
                                .map(|x| format!("\"{}\", ", x))
                                .collect::<String>();
                            // We add a , and a space in the map() function above to each element,
                            // so for the last element we pop them both.
                            labels.pop();
                            labels.pop();
                            format!("@labels({})", labels)
                        };
                        println!(
                            "    {} {}{}: {}{};",
                            labels,
                            field.name,
                            if field.is_optional { "?" } else { "" },
                            field.field_type,
                            field
                                .default_value
                                .as_ref()
                                .map(|d| if field.field_type == "string" {
                                    format!(" = \"{}\"", d)
                                } else {
                                    format!(" = {}", d)
                                })
                                .unwrap_or_else(|| "".into()),
                        );
                    }
                    println!("  }}");
                }
                for def in &version_def.endpoint_defs {
                    println!("  Endpoint: {}", def.path);
                }
                for def in &version_def.label_policy_defs {
                    println!("  Label policy: {}", def.label);
                }
                println!("}}");
            }
        }
        Command::Dev { type_check } => {
            cmd_dev(server_url.clone(), type_check).await?;
        }
        Command::New { path, no_examples } => {
            let path = Path::new(&path);
            if let Err(e) = fs::create_dir(path) {
                match e.kind() {
                    ErrorKind::AlreadyExists => {
                        anyhow::bail!("Directory `{}` already exists. Use `chisel init` to initialize a project in the directory.", path.display());
                    }
                    _ => {
                        anyhow::bail!(
                            "Unable to create a ChiselStrike project in `{}`: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
            let opts = CreateProjectOptions {
                force: false,
                examples: !no_examples,
            };
            create_project(path, opts)?;
        }
        Command::Start => {
            let mut server = start_server()?;
            wait(server_url).await?;
            server.wait()?;
        }
        Command::Status => {
            let mut client = ChiselRpcClient::connect(server_url).await?;
            let request = tonic::Request::new(StatusRequest {});
            let response = execute!(client.get_status(request).await);
            println!("Server status is {}", response.message);
        }
        Command::Restart => {
            let mut client = ChiselRpcClient::connect(server_url.clone()).await?;
            let response = execute!(client.restart(tonic::Request::new(RestartRequest {})).await);
            println!(
                "{}",
                if response.ok {
                    "Server restarted successfully."
                } else {
                    "Server failed to restart."
                }
            );
            wait(server_url.clone()).await?;
        }
        Command::Wait => {
            wait(server_url).await?;
        }
        Command::Apply {
            allow_type_deletion,
            version,
            type_check,
        } => {
            apply(
                server_url,
                version,
                allow_type_deletion.into(),
                type_check.into(),
            )
            .await?;
        }
        Command::Delete { version } => {
            delete(server_url, version).await?;
        }
        Command::Populate { version, from } => {
            populate(server_url, version, from).await?;
        }
    }
    Ok(())
}

async fn cmd_dev(server_url: String, type_check: bool) -> Result<()> {
    let type_check = type_check.into();
    let manifest = read_manifest()?;
    let mut server = start_server()?;
    wait(server_url.clone()).await?;
    apply_from_dev(server_url.clone(), type_check).await;
    let (mut tx, mut rx) = channel(1);
    let mut apply_watcher = RecommendedWatcher::new(move |res: Result<Event, notify::Error>| {
        futures::executor::block_on(async {
            tx.send(res).await.unwrap();
        });
    })?;
    let watcher_config = notify::Config::OngoingEvents(Some(Duration::from_millis(100)));
    apply_watcher.configure(watcher_config)?;
    for models_dir in &manifest.models {
        let models_dir = Path::new(models_dir);
        apply_watcher.watch(models_dir, RecursiveMode::Recursive)?;
    }
    for endpoints_dir in &manifest.endpoints {
        let endpoints_dir = Path::new(endpoints_dir);
        apply_watcher.watch(endpoints_dir, RecursiveMode::Recursive)?;
    }
    for policies_dir in &manifest.policies {
        let policies_dir = Path::new(policies_dir);
        apply_watcher.watch(policies_dir, RecursiveMode::Recursive)?;
    }
    while let Some(res) = rx.next().await {
        match res {
            Ok(notify::event::Event {
                kind: notify::event::EventKind::Modify(notify::event::ModifyKind::Data(_)),
                ..
            }) => {
                apply_from_dev(server_url.clone(), type_check).await;
            }
            Ok(_) => { /* ignore */ }
            Err(e) => println!("watch error: {:?}", e),
        }
    }
    server.wait()?;

    Ok(())
}
