// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::chisel::StatusResponse;
use anyhow::{anyhow, Context, Result};
use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::{
    ChiselApplyRequest, ChiselDeleteRequest, DescribeRequest, EndPointCreationRequest,
    PolicyUpdateRequest, RestartRequest, StatusRequest,
};
use futures::channel::mpsc::channel;
use futures::{SinkExt, StreamExt};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::future::Future;
use std::io::{stdin, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use tonic::transport::Channel;
mod ts;

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

enum AllowTypeDeletion {
    No,
    Yes,
}

impl From<AllowTypeDeletion> for bool {
    fn from(v: AllowTypeDeletion) -> Self {
        match v {
            AllowTypeDeletion::No => false,
            AllowTypeDeletion::Yes => true,
        }
    }
}

impl From<bool> for AllowTypeDeletion {
    fn from(v: bool) -> Self {
        match v {
            false => AllowTypeDeletion::No,
            true => AllowTypeDeletion::Yes,
        }
    }
}

fn dir_to_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for dentry in read_dir(dir)? {
        let dentry = dentry?;
        let path = dentry.path();
        if dentry.file_type()?.is_dir() {
            dir_to_paths(&path, paths)?;
        } else if !dentry
            .file_name()
            .to_str()
            .map_or(false, |x| x.starts_with('.') || x.ends_with('~'))
        {
            // files with names that can't be converted wtih to_str() or that start with . are
            // ignored
            paths.push(path);
        }
    }
    Ok(())
}

fn parse_version(version: &str) -> anyhow::Result<String> {
    anyhow::ensure!(!version.is_empty(), "version name can't be empty");
    Ok(version.to_string())
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

    pub fn types(&self) -> anyhow::Result<Vec<PathBuf>> {
        Self::dirs_to_paths(&self.types)
    }

    pub fn endpoints(&self) -> anyhow::Result<Vec<Endpoint>> {
        let mut ret = vec![];
        for dir in &self.endpoints {
            let mut paths = vec![];
            let dir = Path::new(dir);
            dir_to_paths(dir, &mut paths)?;
            let mut routes = BTreeMap::new();
            for file_path in paths {
                // file_stem returns None only if there is no file name.
                let stem = file_path.file_stem().unwrap();
                // parent returns None only for the root.
                let mut parent = file_path.parent().unwrap().to_path_buf();
                parent.push(stem);

                let name = parent.strip_prefix(&dir)?;

                if let Some(old) = routes.insert(name.to_owned(), file_path.to_owned()) {
                    anyhow::bail!("Cannot add both {} {} as routes. ChiselStrike uses filesystem-based routing, so we don't know what to do. Sorry! 🥺", old.display(), file_path.display());
                }

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

    pub fn policies(&self) -> anyhow::Result<Vec<PathBuf>> {
        Self::dirs_to_paths(&self.policies)
    }

    fn dirs_to_paths(dirs: &[String]) -> anyhow::Result<Vec<PathBuf>> {
        let mut paths = vec![];
        for dir in dirs {
            dir_to_paths(Path::new(dir), &mut paths)?
        }
        paths.sort_unstable();
        Ok(paths)
    }
}

static DEFAULT_API_VERSION: &str = "dev";

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
    /// Create a new ChiselStrike project in current directory.
    Init,
    /// Describe the endpoints, types, and policies.
    Describe,
    /// Start a ChiselStrike server for local development.
    Dev,
    /// Create a new ChiselStrike project.
    New {
        /// Path where to create the project.
        path: String,
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
    },
    /// Delete configuration from the ChiselStrike server.
    Delete {
        #[structopt(long, default_value = DEFAULT_API_VERSION, parse(try_from_str=parse_version))]
        version: String,
    },
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

/// Opens and reads an entire file (or stdin, if filename is "-")
fn read_to_string<P: AsRef<Path>>(filename: P) -> anyhow::Result<String> {
    if filename.as_ref() == Path::new("-") {
        let mut s = "".to_string();
        stdin()
            .read_to_string(&mut s)
            .with_context(|| "while reading stdin".to_string())?;
        Ok(s)
    } else {
        fs::read_to_string(filename.as_ref())
            .with_context(|| format!("while reading {}", filename.as_ref().display()))
    }
}

fn read_dir<P: AsRef<Path>>(dir: P) -> anyhow::Result<fs::ReadDir> {
    fs::read_dir(dir.as_ref()).with_context(|| format!("Could not open {}", dir.as_ref().display()))
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

const MANIFEST_FILE: &str = "Chisel.toml";
const TYPES_DIR: &str = "./types";
const ENDPOINTS_DIR: &str = "./endpoints";
const POLICIES_DIR: &str = "./policies";
const DTS_DIR: &str = "./dts";

fn if_is_dir(path: &str) -> Vec<String> {
    let mut ret = vec![];
    if Path::new(path).is_dir() {
        ret.push(path.to_string());
    }
    ret
}

fn create_project(path: &Path) -> Result<()> {
    if project_exists(path) {
        anyhow::bail!("You cannot run `chisel init` on an existing ChiselStrike project");
    }
    fs::create_dir(path.join(TYPES_DIR))?;
    fs::create_dir(path.join(ENDPOINTS_DIR))?;
    fs::create_dir(path.join(POLICIES_DIR))?;
    fs::create_dir(path.join(DTS_DIR))?;
    let endpoints = std::str::from_utf8(include_bytes!("template/hello.ts"))?.to_string();
    fs::write(path.join(ENDPOINTS_DIR).join("hello.ts"), endpoints)?;
    let tsconfig = std::str::from_utf8(include_bytes!("template/tsconfig.json"))?.to_string();
    fs::write(path.join("tsconfig.json"), tsconfig)?;
    println!("Created ChiselStrike project in {}", path.display());
    Ok(())
}

fn project_exists(path: &Path) -> bool {
    path.join(Path::new(MANIFEST_FILE)).exists()
        || path.join(Path::new(TYPES_DIR)).exists()
        || path.join(Path::new(ENDPOINTS_DIR)).exists()
        || path.join(Path::new(POLICIES_DIR)).exists()
}

fn read_manifest() -> Result<Manifest> {
    Ok(match read_to_string(MANIFEST_FILE) {
        Ok(manifest) => toml::from_str(&manifest)?,
        _ => {
            let types = if_is_dir(TYPES_DIR);
            let endpoints = if_is_dir(ENDPOINTS_DIR);
            let policies = if_is_dir(POLICIES_DIR);
            Manifest::new(types, endpoints, policies)
        }
    })
}

fn start_server() -> anyhow::Result<std::process::Child> {
    println!("🙇‍♂️ Thank you for your interest in the ChiselStrike private beta! (Beta-Jan22.1)");
    println!("⚠️  This is provided to you for evaluation purposes and should not be used to host production at this time");
    println!("Docs with a description of expected functionality and command references at https://docs.chiselstrike.com");
    println!("For any question, concerns, or early feedback, contact us at beta@chiselstrike.com");
    println!("\n🎅 Happy Holidays! 🎄\n");

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

macro_rules! execute {
    ( $cmd:expr ) => {{
        $cmd.map_err(|x| anyhow!(x.message().to_owned()))?
            .into_inner()
    }};
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

async fn apply<S: ToString>(
    server_url: String,
    version: S,
    allow_type_deletion: AllowTypeDeletion,
) -> Result<()> {
    let version = version.to_string();

    let manifest = read_manifest()?;
    let types = manifest.types()?;
    let endpoints = manifest.endpoints()?;
    let policies = manifest.policies()?;

    let mut types_req = vec![];
    let mut endpoints_req = vec![];
    let mut policy_req = vec![];

    let mut decorator_definitions = String::new();
    let mut type_dts_definitions = String::new();

    for t in crate::ts::parse_types(&types)?.into_iter() {
        type_dts_definitions += &format!("    {}: ChiselIterator<{}>;\n", t.name, t.name);
        for field in &t.field_defs {
            for label in &field.labels {
                decorator_definitions += &format!(
                    "function {}(target: any, propertyName: string): void {{}}\n",
                    label
                );
            }
        }
        types_req.push(t);
    }

    // FIXME: for now this is a static string, but we want to add information about the
    // types we created.
    let dts_definitions = format!(
        "
/// <reference lib=\"esnext\" />
/// <reference lib=\"dom\" />

declare type ChiselIterator<T> = {{
    findMany(restrictions: Partial<T>): ChiselIterator<T>;
    select(...columns: (keyof T)[]): ChiselIterator<T>;
    [Symbol.asyncIterator]: () => AsyncIterator<T>;
    join<U>(right: ChiselIterator<U>): ChiselIterator<T & U>;
}}

declare type Chisel = {{
    store: <T>(typeName: string, content: T) => Promise<void>;
    json: (body: any, status?: number) => Response;
{}
}}
declare const Chisel: Chisel
",
        type_dts_definitions
    );

    for f in endpoints.iter() {
        let code = read_to_string(&f.file_path)?;
        endpoints_req.push(EndPointCreationRequest {
            path: f.name.clone(),
            code,
        });
    }

    for p in policies {
        policy_req.push(PolicyUpdateRequest {
            policy_config: read_to_string(p)?,
        });
    }

    // if we fail we'll just write again next time, so it's fine to not worry too much
    // about races here.
    let dts_path = Path::new(DTS_DIR);
    let _ = fs::write(dts_path.join("chisel-decorators.ts"), decorator_definitions);
    let _ = fs::write(dts_path.join("chisel.d.ts"), dts_definitions);

    let mut client = ChiselRpcClient::connect(server_url).await?;
    let msg = execute!(
        client
            .apply(tonic::Request::new(ChiselApplyRequest {
                types: types_req,
                endpoints: endpoints_req,
                policies: policy_req,
                allow_type_deletion: allow_type_deletion.into(),
                version,
            }))
            .await
    );

    for ty in msg.types {
        println!("Type defined: {}", ty);
    }

    for end in msg.endpoints {
        println!("End point defined: {}", end);
    }

    for lbl in msg.labels {
        println!("Policy defined for label {}", lbl);
    }

    Ok(())
}

async fn apply_from_dev(server_url: String) {
    if let Err(e) = apply(server_url, DEFAULT_API_VERSION, AllowTypeDeletion::No).await {
        eprintln!("{:?}", e)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let server_url = opt.rpc_addr;
    match opt.cmd {
        Command::Init => {
            let cwd = env::current_dir()?;
            create_project(&cwd)?;
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
                        println!(
                            "    {} {}{}: {}{};",
                            field
                                .labels
                                .iter()
                                .map(|x| format!(" @{}", x))
                                .collect::<String>(),
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
        Command::Dev => {
            let manifest = read_manifest()?;
            let mut server = start_server()?;
            wait(server_url.clone()).await?;
            apply_from_dev(server_url.clone()).await;
            let (mut tx, mut rx) = channel(1);
            let mut apply_watcher =
                RecommendedWatcher::new(move |res: Result<Event, notify::Error>| {
                    futures::executor::block_on(async {
                        tx.send(res).await.unwrap();
                    });
                })?;
            let watcher_config = notify::Config::OngoingEvents(Some(Duration::from_secs(1)));
            apply_watcher.configure(watcher_config)?;
            for types_dir in &manifest.types {
                let types_dir = Path::new(types_dir);
                apply_watcher.watch(types_dir, RecursiveMode::Recursive)?;
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
                    Ok(event) => {
                        if event.kind.is_modify() {
                            apply_from_dev(server_url.clone()).await;
                        }
                    }
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
            server.wait()?;
        }
        Command::New { path } => {
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
            create_project(path)?;
        }
        Command::Start => {
            let mut server = start_server()?;
            wait(server_url.clone()).await?;
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
        } => {
            apply(server_url.clone(), version, allow_type_deletion.into()).await?;
        }
        Command::Delete { version } => {
            delete(server_url.clone(), version).await?;
        }
    }
    Ok(())
}
