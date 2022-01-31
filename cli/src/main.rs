// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::chisel::StatusResponse;
use anyhow::{anyhow, Context, Result};
use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::{
    ChiselApplyRequest, ChiselDeleteRequest, DescribeRequest, EndPointCreationRequest,
    PolicyUpdateRequest, PopulateRequest, RestartRequest, StatusRequest,
};
use compile::compile_ts_code as swc_compile;
use futures::channel::mpsc::channel;
use futures::{SinkExt, StreamExt};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::future::Future;
use std::io::{stdin, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use tempfile::Builder;
use tempfile::NamedTempFile;
use tonic::transport::Channel;
use tsc_compile::compile_ts_code;

mod ts;

// Timeout when waiting for connection or server status.
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize, PartialEq)]
enum Module {
    #[serde(rename = "node")]
    Node,
    #[serde(rename = "deno")]
    Deno,
}

impl Default for Module {
    fn default() -> Self {
        Module::Node
    }
}

/// Manifest defines the files that describe types, endpoints, and policies.
///
/// The manifest is a high-level declaration of application behavior.
/// The individual definitions are passed to `chiseld`, which processes them
/// accordingly. For example, type definitions are imported as types and
/// endpoints are made executable via Deno.
#[derive(Deserialize)]
struct Manifest {
    /// Vector of directories to scan for model definitions.
    models: Vec<String>,
    /// Vector of directories to scan for endpoint definitions.
    endpoints: Vec<String>,
    /// Vector of directories to scan for policy definitions.
    policies: Vec<String>,
    /// Whether to use deno-style or node-style modules
    #[serde(default)]
    modules: Module,
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
        } else if !dentry.file_name().to_str().map_or(false, ignore_path) {
            // files with names that can't be converted wtih to_str() or that start with . are
            // ignored
            paths.push(path);
        }
    }
    Ok(())
}

fn ignore_path(path: &str) -> bool {
    if path.starts_with('.') {
        return true;
    }
    if path.ends_with('~') {
        return true;
    }
    if path.starts_with('#') && path.ends_with('#') {
        // Emacs auto-save files.
        return true;
    }
    false
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
    pub fn models(&self) -> anyhow::Result<Vec<PathBuf>> {
        Self::dirs_to_paths(&self.models)
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
    Dev,
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
const TYPES_DIR: &str = "./models";
const ENDPOINTS_DIR: &str = "./endpoints";
const POLICIES_DIR: &str = "./policies";
const VSCODE_DIR: &str = "./.vscode/";

/// Writes contents to a file in a directory.
fn write(contents: &[u8], dir: &Path, file: &str) -> Result<()> {
    let s = std::str::from_utf8(contents)?.to_string();
    fs::write(dir.join(file), s).map_err(|e| e.into())
}

/// Writes "template/$file" content into $dir/$file.  The file content is read at compile time but written at
/// runtime.
macro_rules! write_template {
    ( $file:expr, $dir:expr ) => {{
        write(include_bytes!(concat!("template/", $file)), $dir, $file)
    }};
}

/// Project creation options.
struct CreateProjectOptions {
    /// Force project creation by overwriting existing project files.
    force: bool,
    /// Generate example code for project.
    examples: bool,
}

fn create_project(path: &Path, opts: CreateProjectOptions) -> Result<()> {
    if !opts.force && project_exists(path) {
        anyhow::bail!("You cannot run `chisel init` on an existing ChiselStrike project");
    }
    fs::create_dir_all(path.join(TYPES_DIR))?;
    fs::create_dir_all(path.join(ENDPOINTS_DIR))?;
    fs::create_dir_all(path.join(POLICIES_DIR))?;
    fs::create_dir_all(path.join(VSCODE_DIR))?;
    write_template!("package.json", path)?;
    write_template!("tsconfig.json", path)?;
    write_template!("Chisel.toml", path)?;
    // creating through chisel instead of npx: default to deno resolution
    let mut toml = include_bytes!("template/Chisel.toml").to_vec();
    toml.extend_from_slice("modules = \"deno\"\n".as_bytes());
    write(&toml, path, "Chisel.toml")?;

    write_template!("settings.json", &path.join(VSCODE_DIR))?;

    if opts.examples {
        write_template!("hello.ts", &path.join(ENDPOINTS_DIR))?;
    }
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
    let cwd = env::current_dir()?;
    if !Path::new(MANIFEST_FILE).exists() {
        anyhow::bail!("Could not find `{}` in `{}`. Did you forget to run `chisel init` to initialize the project?", MANIFEST_FILE, cwd.display());
    }
    let manifest = read_to_string(MANIFEST_FILE)?;
    let manifest: Manifest = match toml::from_str(&manifest) {
        Ok(manifest) => manifest,
        Err(error) => {
            anyhow::bail!(
                "Failed to parse manifest at `{}`:\n\n{}",
                cwd.join(MANIFEST_FILE).display(),
                error
            );
        }
    };
    for dir in &manifest.models {
        if !Path::new(dir).exists() {
            anyhow::bail!(
                "Manifest at `{}` has models directory `{}` that does not exist.",
                cwd.join(MANIFEST_FILE).display(),
                cwd.join(dir).display()
            );
        }
    }
    for dir in &manifest.endpoints {
        if !Path::new(dir).exists() {
            anyhow::bail!(
                "Manifest at `{}` has endpoints directory `{}` that does not exist.",
                cwd.join(MANIFEST_FILE).display(),
                cwd.join(dir).display()
            );
        }
    }
    for dir in &manifest.policies {
        if !Path::new(dir).exists() {
            anyhow::bail!(
                "Manifest at `{}` has policies directory `{}` that does not exist.",
                cwd.join(MANIFEST_FILE).display(),
                cwd.join(dir).display()
            );
        }
    }
    Ok(manifest)
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

fn to_tempfile(data: &str, suffix: &str) -> Result<NamedTempFile> {
    let mut f = Builder::new().suffix(suffix).tempfile()?;
    let inner = f.as_file_mut();
    inner.write_all(data.as_bytes())?;
    inner.flush()?;
    Ok(f)
}

async fn apply<S: ToString>(
    server_url: String,
    version: S,
    allow_type_deletion: AllowTypeDeletion,
) -> Result<()> {
    let version = version.to_string();

    let manifest = read_manifest().with_context(|| "Reading manifest file".to_string())?;
    let models = manifest.models()?;
    let endpoints = manifest.endpoints()?;
    let policies = manifest.policies()?;

    let types_req = crate::ts::parse_types(&models)?;
    let mut endpoints_req = vec![];
    let mut policy_req = vec![];

    let import_str = "import * as ChiselAlias from \"@chiselstrike/api\";
         declare global {
             var Chisel: typeof ChiselAlias;
         }"
    .to_string();
    let import_temp = to_tempfile(&import_str, ".d.ts")?;

    let mut types_string = String::new();
    for t in &models {
        types_string += &read_to_string(&t)?;
    }

    if manifest.modules == Module::Node {
        // For existing installations that didn't have webpack, we create the conf
        // file here
        let webpack_conf_dir = PathBuf::from("./.webpack");
        let _ignored = tokio::fs::create_dir(&webpack_conf_dir).await;
        if let Err(path) = tokio::fs::metadata(webpack_conf_dir.join("webpack.config.js")).await {
            if path.kind() == std::io::ErrorKind::NotFound {
                // synchronous, but that's fine because only happens once.
                write_template!("webpack.config.js", &webpack_conf_dir)?;
            }
        }

        let webpack_output = tempfile::tempdir()?;
        let webpack_output_dname = webpack_output.path().to_str().unwrap();
        let cwd = env::current_dir()?;

        for endpoint in endpoints.iter() {
            let mut f = Builder::new().suffix(".ts").tempfile()?;
            let inner = f.as_file_mut();
            let mut import_path = endpoint.file_path.to_owned();
            import_path.set_extension("");

            let code = format!(
                "import fun from \"{}/{}\";\nexport default fun",
                cwd.display(),
                import_path.display()
            );
            inner.write_all(code.as_bytes())?;
            inner.flush()?;
            let webpack_entry_fname = f.path().to_str().unwrap();
            let res = std::process::Command::new("npx")
                .arg("webpack")
                .arg("--color")
                .arg("-c")
                .arg("./.webpack/webpack.config.js")
                .arg("--entry")
                .arg(webpack_entry_fname)
                .arg("-o")
                .arg(webpack_output_dname)
                .output()
                .with_context(|| {
                    "trying to execute `npx webpack`. Is npx on your PATH?".to_string()
                })?;

            if !res.status.success() {
                let out = String::from_utf8(res.stdout).expect("command output not utf-8");
                let err = String::from_utf8(res.stderr).expect("command output not utf-8");

                return Err(anyhow!(
                    "compiling endpoint {}",
                    endpoint.file_path.display()
                ))
                .with_context(|| format!("{}\n{}", out, err));
            }
            let code = read_to_string(webpack_output.path().join("endpoint.mjs"))?;

            endpoints_req.push(EndPointCreationRequest {
                path: endpoint.name.clone(),
                code,
            });
        }
    } else {
        let mods: HashMap<String, String> = [(
            "@chiselstrike/api".to_string(),
            api::chisel_d_ts().to_string(),
        )]
        .into_iter()
        .collect();

        for f in endpoints.iter() {
            let ext = f.file_path.extension().unwrap().to_str().unwrap();
            let path = f.file_path.to_str().unwrap();

            let code = if ext == "ts" {
                let mut code = compile_ts_code(
                    path,
                    Some(import_temp.path().to_str().unwrap()),
                    mods.clone(),
                )
                .with_context(|| format!("parsing endpoint /{}/{}", version, f.name))?;
                code.remove(path).unwrap()
            } else {
                read_to_string(&f.file_path)?
            };

            let code = types_string.clone() + &code;
            let code = swc_compile(code)?;
            endpoints_req.push(EndPointCreationRequest {
                path: f.name.clone(),
                code,
            });
        }
    }

    for p in policies {
        policy_req.push(PolicyUpdateRequest {
            policy_config: read_to_string(p)?,
        });
    }

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
        println!("Model defined: {}", ty);
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
                        apply_from_dev(server_url.clone()).await;
                    }
                    Ok(_) => { /* ignore */ }
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
            server.wait()?;
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
        } => {
            apply(server_url, version, allow_type_deletion.into()).await?;
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
