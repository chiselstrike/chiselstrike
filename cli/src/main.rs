// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::cmd::apply::apply;
use crate::cmd::dev::cmd_dev;
use crate::cmd::generate;
use crate::project::{create_project, CreateProjectOptions};
use crate::proto::chisel_rpc_client::ChiselRpcClient;
use crate::proto::{
    type_msg::TypeEnum, DeleteRequest, DescribeRequest, PopulateRequest, StatusRequest,
};
use crate::server::{start_server, wait};
use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use futures::{pin_mut, Future, FutureExt};
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::process::Child;

mod cmd;
mod codegen;
mod events;
mod project;
mod routes;
mod server;
mod ts;

#[allow(clippy::all)]
mod proto {
    tonic::include_proto!("chisel");
}

fn parse_version(version: &str) -> anyhow::Result<String> {
    anyhow::ensure!(!version.is_empty(), "version name can't be empty");
    Ok(version.to_string())
}

fn parse_generate_mode(mode: &str) -> anyhow::Result<generate::Mode> {
    match mode {
        "deno" => Ok(generate::Mode::Deno),
        "node" => Ok(generate::Mode::Node),
        _ => anyhow::bail!("allowed generate modes are 'deno' and 'node'. Got {mode:?}"),
    }
}

pub(crate) static DEFAULT_API_VERSION: &str = "dev";

#[derive(Parser, Debug)]
#[command(name = "chisel", version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT"))]
struct Opt {
    /// User-visible HTTP API server listen address.
    #[structopt(short, long, default_value = "localhost:8080")]
    api_listen_addr: String,
    /// RPC server address.
    #[arg(short, long, default_value = "http://localhost:50051")]
    rpc_addr: String,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a new ChiselStrike project in current directory.
    Init {
        /// Force project initialization by overwriting files if needed.
        #[arg(long)]
        force: bool,
        /// Skip generating example code.
        #[arg(long)]
        no_examples: bool,
        /// Enable the optimizer
        #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
        optimize: bool,
        /// Enable auto-indexing.
        #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
        auto_index: bool,
    },
    /// Describe the endpoints, types, and policies.
    Describe,
    /// Start a ChiselStrike server for local development.
    Dev {
        /// calls tsc --noEmit to check types. Useful if your IDE isn't doing it.
        #[arg(long)]
        type_check: bool,
        /// Activate inspector and let a debugger attach at any time.
        #[arg(long)]
        inspect: bool,
    },
    /// Generate a ChiselStrike client API for this project.
    Generate {
        /// Output directory where the generated client files will be written.
        /// If the folder doesn't exist, it will be created.
        output_dir: PathBuf,
        #[arg(long, default_value = DEFAULT_API_VERSION, value_parser = parse_version)]
        /// Specifies version of the chisel API for which the client will be generated.
        version: String,
        /// Compatibility mode of the generated client. Either 'node' or 'deno'.
        #[arg(long, default_value = "node", value_parser = parse_generate_mode)]
        mode: generate::Mode,
    },
    /// Create a new ChiselStrike project.
    New {
        /// Path where to create the project.
        path: String,
        /// Skip generating example code.
        #[arg(long)]
        no_examples: bool,
        /// Enable the optimizer
        #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
        optimize: bool,
        /// Enable auto-indexing.
        #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
        auto_index: bool,
    },
    /// Start the ChiselStrike server.
    Start,
    /// Show ChiselStrike server status.
    Status,
    /// Wait for the ChiselStrike server to start.
    Wait,
    /// Apply configuration to the ChiselStrike server.
    Apply {
        #[arg(long)]
        allow_type_deletion: bool,
        #[arg(long, default_value = DEFAULT_API_VERSION, value_parser = parse_version)]
        version: String,
        /// calls tsc --noEmit to check types. Useful if your IDE isn't doing it.
        #[arg(long)]
        type_check: bool,
    },
    /// Delete configuration from the ChiselStrike server.
    Delete {
        #[arg(long, default_value = DEFAULT_API_VERSION, value_parser = parse_version)]
        version: String,
    },
    Populate {
        #[arg(long)]
        version: String,
        #[arg(long)]
        from: String,
    },
}

async fn delete(server_url: String, version_id: String) -> Result<()> {
    let mut client = ChiselRpcClient::connect(server_url).await?;

    let msg = execute!(
        client
            .delete(tonic::Request::new(DeleteRequest { version_id }))
            .await
    );
    println!("{}", msg.message);
    Ok(())
}

async fn populate(
    server_url: String,
    to_version_id: String,
    from_version_id: String,
) -> Result<()> {
    let mut client = ChiselRpcClient::connect(server_url).await?;

    let msg = execute!(
        client
            .populate(tonic::Request::new(PopulateRequest {
                to_version_id,
                from_version_id,
            }))
            .await
    );
    println!("{}", msg.message);
    Ok(())
}

async fn spawn_server<T, F, Fut, Fut2>(chiseld_args: Vec<String>, fut: Fut, cb: F) -> Result<()>
where
    Fut: Future<Output = T>,
    Fut2: Future<Output = Result<()>>,
    F: FnOnce(Child, T) -> Fut2,
{
    let mut server = start_server(chiseld_args)?;
    let fut = fut.fuse();

    pin_mut!(fut);

    tokio::select! {
        res = server.wait() => {
            res?;
        }
        res = &mut fut => {
            cb(server, res).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let chisel_args = std::env::args().take_while(|arg| arg != "--");
    let mut chiseld_args = std::env::args()
        .skip_while(|arg| arg != "--")
        .skip(1)
        .collect::<Vec<_>>();

    let opt = Opt::parse_from(chisel_args);
    let server_url = opt.rpc_addr;
    let api_listen_addr = opt.api_listen_addr;
    match opt.cmd {
        Command::Init {
            force,
            no_examples,
            optimize,
            auto_index,
        } => {
            let cwd = env::current_dir()?;
            let opts = CreateProjectOptions {
                force,
                examples: !no_examples,
                optimize,
                auto_index,
            };
            create_project(&cwd, opts)?;
        }
        Command::Describe => {
            let mut client = ChiselRpcClient::connect(server_url).await?;
            let request = tonic::Request::new(DescribeRequest {});
            let response = execute!(client.describe(request).await);

            for version_def in response.version_defs {
                println!("Version: {} {{", version_def.version_id);
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
                            format!("@labels({}) ", labels)
                        };
                        let field_type = field.field_type()?;
                        println!(
                            "    {}{}{}{}: {}{};",
                            if field.is_unique { "@unique " } else { "" },
                            labels,
                            field.name,
                            if field.is_optional { "?" } else { "" },
                            field_type,
                            field
                                .default_value
                                .as_ref()
                                .map(|d| if matches!(field_type, TypeEnum::String(_)) {
                                    format!(" = \"{}\"", d)
                                } else {
                                    format!(" = {}", d)
                                })
                                .unwrap_or_else(|| "".into()),
                        );
                    }
                    println!("  }}");
                }
                for def in &version_def.label_policy_defs {
                    println!("  Label policy: {}", def.label);
                }
                println!("}}");
            }
        }
        Command::Dev {
            type_check,
            inspect,
        } => {
            let fut = cmd_dev(server_url.clone(), type_check);
            let cb = |mut server: Child, res| async move {
                let sig_task = res?;
                server.kill().await?;
                server.wait().await?;
                sig_task.await??;

                Ok(())
            };
            chiseld_args.push("--debug".to_string());
            if inspect {
                chiseld_args.push("--inspect".to_string());
            }
            spawn_server(chiseld_args, fut, cb).await?;
        }
        Command::Generate {
            output_dir,
            version,
            mode,
        } => {
            let args = generate::Opts {
                server_url,
                api_addres: api_listen_addr,
                output_dir,
                version,
                mode,
            };
            generate::cmd_generate(args).await?;
        }
        Command::New {
            path,
            no_examples,
            optimize,
            auto_index,
        } => {
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
                optimize,
                auto_index,
            };
            create_project(path, opts)?;
        }
        Command::Start => {
            let fut = wait(server_url);
            let cb = |mut server: Child, res: Result<_>| async move {
                res?;
                server.wait().await?;

                Ok(())
            };

            spawn_server(chiseld_args, fut, cb).await?;
        }
        Command::Status => {
            let mut client = ChiselRpcClient::connect(server_url).await?;
            let request = tonic::Request::new(StatusRequest {});
            let response = execute!(client.get_status(request).await);
            println!("Server status is {}", response.message);
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
