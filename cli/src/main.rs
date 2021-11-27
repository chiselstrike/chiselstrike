// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::chisel::StatusResponse;
use anyhow::{anyhow, Context, Result};
use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::{
    AddTypeRequest, EndPointCreationRequest, EndPointRemoveRequest, FieldDefinition,
    PolicyUpdateRequest, RestartRequest, StatusRequest, TypeExportRequest,
};
use futures::channel::mpsc::channel;
use futures::{SinkExt, StreamExt};
use graphql_parser::schema::{parse_schema, Definition, TypeDefinition};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use serde_derive::Deserialize;
use std::fs;
use std::future::Future;
use std::io::{stdin, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use tonic::transport::Channel;

// Timeout when waiting for connection or server status.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Manifest defines the files that describe types, endpoints, and policies.
///
/// The manifest is a high-level declaration of application behavior.
/// The individual definitions are passed to `chiseld`, which processes them
/// accordingly. For example, type definitions are imported as types and
/// endpoints are made executable via Deno.
#[derive(Deserialize)]
struct Manifest {
    /// Vector of directories to scan for type definitions.
    types: Vec<String>,
    /// Vector of directories to scan for endpoint definitions.
    endpoints: Vec<String>,
    /// Vector of directories to scan for policy definitions.
    policies: Vec<String>,
}

fn dir_to_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), anyhow::Error> {
    for dentry in read_dir(dir)? {
        let dentry = dentry?;
        let path = dentry.path();
        if dentry.file_type()?.is_dir() {
            dir_to_paths(&path, paths)?;
        } else if !ignore_path(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

#[derive(PartialOrd, PartialEq, Eq, Ord)]
struct Endpoint {
    name: String,
    file_path: PathBuf,
}

impl Manifest {
    pub fn new(types: Vec<String>, endpoints: Vec<String>, policies: Vec<String>) -> Self {
        Manifest {
            types,
            endpoints,
            policies,
        }
    }

    pub fn types(&self) -> Result<Vec<PathBuf>, anyhow::Error> {
        Self::dirs_to_paths(&self.types)
    }

    pub fn endpoints(&self) -> Result<Vec<Endpoint>, anyhow::Error> {
        let mut ret = vec![];
        for dir in &self.endpoints {
            let mut paths = vec![];
            let dir = Path::new(dir);
            dir_to_paths(dir, &mut paths)?;
            for file_path in paths {
                // FIXME: We should probably require a .ts or .js
                // extension. We should also error out if we have both
                // foo.js and foo.ts.
                //
                // file_stem returns None only if there is no file name.
                let stem = file_path.file_stem().unwrap();
                // parent returns None only for the root.
                let mut parent = file_path.parent().unwrap().to_path_buf();
                parent.push(stem);
                let name = parent.strip_prefix(&dir)?;
                let name = name
                    .to_str()
                    .ok_or_else(|| anyhow!("filename is not utf8 {:?}", name))?
                    .to_string();
                ret.push(Endpoint { file_path, name });
            }
        }
        ret.sort_unstable();
        Ok(ret)
    }

    pub fn policies(&self) -> Result<Vec<PathBuf>, anyhow::Error> {
        Self::dirs_to_paths(&self.policies)
    }

    fn dirs_to_paths(dirs: &[String]) -> Result<Vec<PathBuf>, anyhow::Error> {
        let mut paths = vec![];
        for dir in dirs {
            dir_to_paths(Path::new(dir), &mut paths)?
        }
        paths.sort_unstable();
        Ok(paths)
    }
}

/// Return true if path should be ignored.
///
/// This function rejects paths that are known to be temporary files.
fn ignore_path(path: &Path) -> bool {
    let patterns = vec![".*~", ".*.swp"];
    for pat in patterns {
        let re = Regex::new(pat).unwrap();
        if re.is_match(path.to_str().unwrap()) {
            return true;
        }
    }
    false
}

#[derive(StructOpt, Debug)]
#[structopt(name = "chisel")]
struct Opt {
    /// RPC server address.
    #[structopt(short, long, default_value = "http://localhost:50051")]
    rpc_addr: String,
    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(StructOpt, Debug)]
enum Command {
    /// Initialize a new ChiselStrike project.
    Init,
    /// Start a ChiselStrike server for local development.
    Dev,
    /// Shows information about ChiselStrike server status.
    Status,
    Type {
        #[structopt(subcommand)]
        cmd: TypeCommand,
    },
    Restart,
    Wait,
    Apply,
}

#[derive(StructOpt, Debug)]
enum TypeCommand {
    /// Export the type system.
    Export,
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

/// Opens and reads an entire file (or stdin, if filename is "-")
fn read_to_string<P: AsRef<Path>>(filename: P) -> Result<String, std::io::Error> {
    if filename.as_ref() == Path::new("-") {
        let mut s = "".to_string();
        stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        fs::read_to_string(filename.as_ref())
    }
}

fn read_dir<P: AsRef<Path>>(dir: P) -> Result<fs::ReadDir, anyhow::Error> {
    fs::read_dir(dir.as_ref()).with_context(|| format!("Could not open {}", dir.as_ref().display()))
}

async fn import_types<P>(
    client: &mut ChiselRpcClient<tonic::transport::Channel>,
    filename: P,
) -> Result<()>
where
    P: AsRef<Path>,
{
    let schema = read_to_string(filename)?;
    let type_system = parse_schema::<String>(&schema)?;
    for def in &type_system.definitions {
        match def {
            Definition::TypeDefinition(TypeDefinition::Object(obj_def)) => {
                let mut field_defs = Vec::default();
                for field_def in &obj_def.fields {
                    field_defs.push(FieldDefinition {
                        name: field_def.name.to_owned(),
                        field_type: format!("{}", field_def.field_type.to_owned()),
                        labels: field_def
                            .directives
                            .iter()
                            .map(|d| d.name.clone())
                            .collect(),
                    });
                }
                let request = tonic::Request::new(AddTypeRequest {
                    name: obj_def.name.to_owned(),
                    field_defs,
                });
                let response = client.add_type(request).await?.into_inner();
                println!("Type defined: {}", response.message);
            }
            def => {
                println!("Ignoring type definition: {:?}", def);
            }
        }
    }
    Ok(())
}

async fn create_endpoint<P>(
    client: &mut ChiselRpcClient<tonic::transport::Channel>,
    path: String,
    filename: P,
) -> Result<()>
where
    P: AsRef<Path>,
{
    let code = read_to_string(&filename)?;
    let request = tonic::Request::new(EndPointCreationRequest { path, code });
    let response = client.create_end_point(request).await?.into_inner();
    println!("End point defined: {}", response.message);
    Ok(())
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
                    return Err(anyhow!("Timeout"));
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

const TYPES_DIR: &str = "./types";
const ENDPOINTS_DIR: &str = "./endpoints";
const POLICIES_DIR: &str = "./policies";

fn if_is_dir(path: &str) -> Vec<String> {
    let mut ret = vec![];
    if Path::new(path).is_dir() {
        ret.push(path.to_string());
    }
    ret
}

fn read_manifest() -> Result<Manifest> {
    Ok(match read_to_string("Chisel.toml") {
        Ok(manifest) => toml::from_str(&manifest)?,
        _ => {
            let types = if_is_dir(TYPES_DIR);
            let endpoints = if_is_dir(ENDPOINTS_DIR);
            let policies = if_is_dir(POLICIES_DIR);
            Manifest::new(types, endpoints, policies)
        }
    })
}

async fn apply(server_url: String) -> Result<()> {
    let manifest = read_manifest()?;
    let types = manifest.types()?;
    let endpoints = manifest.endpoints()?;
    let policies = manifest.policies()?;

    let mut client = ChiselRpcClient::connect(server_url).await?;

    for entry in types {
        // FIXME: will fail the second time until we implement type evolution
        import_types(&mut client, entry).await?;
    }

    let request = tonic::Request::new(EndPointRemoveRequest { path: None });
    client.remove_end_point(request).await?;

    for entry in endpoints {
        create_endpoint(&mut client, entry.name, entry.file_path).await?;
    }

    for entry in policies {
        let policystr = read_to_string(entry)?;

        let response = client
            .policy_update(tonic::Request::new(PolicyUpdateRequest {
                policy_config: policystr,
            }))
            .await?
            .into_inner();
        println!("Policy applied: {}", response.message);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let server_url = opt.rpc_addr;
    match opt.cmd {
        Command::Init => {
            fs::create_dir(TYPES_DIR)?;
            fs::create_dir(ENDPOINTS_DIR)?;
            fs::create_dir(POLICIES_DIR)?;
            let endpoints = std::str::from_utf8(include_bytes!("template/hello.js"))?.to_string();
            fs::write(format!("{}/hello.js", ENDPOINTS_DIR), endpoints)?;
        }
        Command::Dev => {
            let manifest = read_manifest()?;
            let types = manifest.types()?;
            let endpoints = manifest.endpoints()?;
            let policies = manifest.policies()?;
            let mut cmd = std::env::current_exe()?;
            cmd.pop();
            cmd.push("chiseld");
            let mut server = std::process::Command::new(cmd).spawn()?;
            wait(server_url.clone()).await?;
            apply(server_url.clone()).await?;
            let (mut tx, mut rx) = channel(1);
            let mut apply_watcher =
                RecommendedWatcher::new(move |res: Result<Event, notify::Error>| {
                    futures::executor::block_on(async {
                        tx.send(res).await.unwrap();
                    });
                })?;
            let watcher_config = notify::Config::OngoingEvents(Some(Duration::from_secs(1)));
            apply_watcher.configure(watcher_config)?;
            for ty in types {
                apply_watcher.watch(&ty, RecursiveMode::NonRecursive)?;
            }
            for endpoint in endpoints {
                apply_watcher.watch(&endpoint.file_path, RecursiveMode::NonRecursive)?;
            }
            for policy in policies {
                apply_watcher.watch(&policy, RecursiveMode::NonRecursive)?;
            }
            while let Some(res) = rx.next().await {
                match res {
                    Ok(event) => {
                        if event.kind.is_modify() {
                            apply(server_url.clone()).await?;
                        }
                    }
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
            server.wait()?;
        }
        Command::Status => {
            let mut client = ChiselRpcClient::connect(server_url).await?;
            let request = tonic::Request::new(StatusRequest {});
            let response = client.get_status(request).await?.into_inner();
            println!("Server status is {}", response.message);
        }
        Command::Type { cmd } => match cmd {
            TypeCommand::Export => {
                let mut client = ChiselRpcClient::connect(server_url).await?;
                let request = tonic::Request::new(TypeExportRequest {});
                let response = client.export_types(request).await?.into_inner();
                for def in response.type_defs {
                    println!("class {} {{", def.name);
                    for field in def.field_defs {
                        println!(
                            "  {}: {}{}",
                            field.name,
                            field.field_type,
                            field
                                .labels
                                .iter()
                                .map(|x| format!(" @{}", x))
                                .collect::<String>()
                        );
                    }
                    println!("}}");
                }
            }
        },
        Command::Restart => {
            let mut client = ChiselRpcClient::connect(server_url.clone()).await?;
            let response = client
                .restart(tonic::Request::new(RestartRequest {}))
                .await?
                .into_inner();
            println!("{}", if response.ok { "success" } else { "failure" });
            wait(server_url.clone()).await?;
            apply(server_url).await?;
        }
        Command::Wait => {
            wait(server_url).await?;
        }
        Command::Apply => {
            apply(server_url.clone()).await?;
        }
    }
    Ok(())
}
