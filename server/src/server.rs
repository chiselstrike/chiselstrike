// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::datastore::{DbConnection, MetaService, QueryEngine};
use crate::deno;
use crate::deno::init_deno;
use crate::deno::set_meta;
use crate::deno::set_policies;
use crate::deno::set_query_engine;
use crate::deno::set_type_system;
use crate::deno::update_secrets;
use crate::deno::{activate_endpoint, compile_endpoints};
use crate::rpc::{GlobalRpcState, RpcService};
use crate::runtime;
use crate::runtime::Runtime;
use crate::secrets::get_secrets;
use crate::JsonObject;
use anyhow::Result;
use async_lock::Mutex;
use deno_core::futures;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use futures::StreamExt;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::panic;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use tokio::task::JoinHandle;
use tokio::time::sleep;

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "chiseld", version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT"))]
pub struct Opt {
    /// user-visible API server listen address.
    #[structopt(short, long, default_value = "localhost:8080")]
    api_listen_addr: String,
    /// RPC server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    rpc_listen_addr: SocketAddr,
    /// Internal routes (for k8s) listen address
    #[structopt(short, long, default_value = "127.0.0.1:9090")]
    internal_routes_listen_addr: SocketAddr,
    /// Metadata database URI. [deprecated: use --db-uri instead]
    #[structopt(short, long, default_value = "sqlite://chiseld.db?mode=rwc")]
    _metadata_db_uri: String,
    /// Data database URI. [deprecated: use --db-uri instead]
    #[structopt(short, long, default_value = "sqlite://chiseld-data.db?mode=rwc")]
    _data_db_uri: String,
    /// Database URI.
    #[structopt(long, default_value = "sqlite://.chiseld.db?mode=rwc")]
    db_uri: String,
    /// Should we wait for a debugger before executing any JS?
    #[structopt(long)]
    inspect_brk: bool,
    /// size of database connection pool.
    #[structopt(short, long, default_value = "10")]
    nr_connections: usize,
    /// How many executor threads to create
    #[structopt(short, long, default_value = "1")]
    executor_threads: usize,
    /// If on, serve a web UI on an internal route.
    #[structopt(long)]
    webui: bool,
}

/// Whether an action should be repeated.
pub enum DoRepeat {
    Yes,
    No,
}

pub trait CommandTrait: (FnOnce() -> LocalBoxFuture<'static, Result<()>>) + Send + 'static {}

impl<T> CommandTrait for T where
    T: (FnOnce() -> LocalBoxFuture<'static, Result<()>>) + Send + 'static
{
}

pub type Command = Box<dyn CommandTrait>;
pub type CommandResult = Result<()>;

#[derive(Clone)]
pub struct SharedState {
    signal_rx: async_channel::Receiver<()>,
    /// ChiselRpc waits on all API threads to send here before it starts serving RPC.
    readiness_tx: async_channel::Sender<()>,
    api_listen_addr: String,
    inspect_brk: bool,
    executor_threads: usize,
    db: DbConnection,
    nr_connections: usize,
}

impl SharedState {
    pub fn executor_threads(&self) -> usize {
        self.executor_threads
    }
}

pub struct SharedTasks {
    rpc_task: JoinHandle<Result<()>>,
    sig_task: JoinHandle<Result<DoRepeat>>,
}

impl SharedTasks {
    pub async fn join(self) -> Result<DoRepeat> {
        self.rpc_task.await??;
        self.sig_task.await?
    }
}

pub(crate) async fn add_endpoints(
    sources: HashMap<String, String>,
    api_service: &ApiService,
) -> Result<()> {
    compile_endpoints(sources.clone()).await?;

    for path in sources.keys() {
        activate_endpoint(path).await?;

        let func = Arc::new({
            let path = path.to_string();
            move |req| deno::run_js(path.clone(), req).boxed_local()
        });
        api_service.add_route(path.into(), func);
    }
    Ok(())
}

async fn read_secrets() -> JsonObject {
    static LAST_TRY_WAS_FAILURE: Mutex<bool> = Mutex::new(false);
    let secrets = get_secrets().await;
    let mut was_failure = LAST_TRY_WAS_FAILURE.lock().await;
    match secrets {
        Ok(secrets) => {
            *was_failure = false;
            secrets
        }
        Err(e) => {
            if !*was_failure {
                warn!("Could not read secrets: {:?}", e);
            }
            *was_failure = true;
            Default::default()
        }
    }
}

async fn run(state: SharedState, mut cmd: ExecutorChannel) -> Result<()> {
    init_deno(state.inspect_brk).await?;

    // Ensure we read the secrets before spawning an ApiService; secrets may dictate API authorization.
    update_secrets(read_secrets().await).await;

    let meta = MetaService::local_connection(&state.db, state.nr_connections).await?;
    let ts = meta.load_type_system().await?;

    let routes = meta.load_endpoints().await?;
    let policies = meta.load_policies().await?;
    let api_info = meta.load_api_info().await?;

    let mut api_service = ApiService::new(api_info);
    crate::auth::init(&mut api_service).await?;
    crate::introspect::init(&api_service);

    let query_engine =
        Arc::new(QueryEngine::local_connection(&state.db, state.nr_connections).await?);
    ts.create_builtin_backing_tables(query_engine.as_ref())
        .await?;
    let api_service = Rc::new(api_service);
    let versions: Vec<&String> = ts.versions.keys().collect();

    for v in versions {
        crate::introspect::add_introspection(&api_service, v);
    }

    let rt = Runtime::new(api_service.clone());
    runtime::set(rt);
    set_type_system(ts).await;
    set_query_engine(query_engine).await;
    set_policies(policies).await;
    set_meta(meta).await;

    let sources = routes
        .iter()
        .map(|(k, v)| (k.to_str().unwrap().to_string(), v.clone()))
        .collect();
    add_endpoints(sources, &api_service).await?;

    let command_task = tokio::task::spawn_local(async move {
        while let Some(item) = cmd.rx.next().await {
            let res = item().await;
            cmd.tx.send(res).await.unwrap();
        }
    });

    let api_tasks = crate::api::spawn(
        api_service,
        state.api_listen_addr.clone(),
        state.signal_rx.clone(),
    )?;
    state.readiness_tx.send(()).await?;

    info!(
        "ChiselStrike is ready 🚀 - URL: http://{} ",
        state.api_listen_addr
    );

    for api_task in api_tasks {
        api_task.await??;
    }
    command_task.await?;
    deno::shutdown();
    Ok(())
}

// Receives commands, returns results
pub struct ExecutorChannel {
    pub rx: async_channel::Receiver<Command>,
    pub tx: async_channel::Sender<CommandResult>,
}

// Sends commands, receives results.
#[derive(Clone)]
pub struct CoordinatorChannel {
    pub tx: async_channel::Sender<Command>,
    pub rx: async_channel::Receiver<CommandResult>,
}

impl CoordinatorChannel {
    pub async fn send(&self, cmd: Command) -> CommandResult {
        // Send fails only if the channel is closed, so unwrap is ok.
        self.tx.send(cmd).await.unwrap();
        self.rx.recv().await.unwrap()
    }
}

fn extract(s: &str) -> Option<String> {
    let sqlite = regex::Regex::new("sqlite://(?P<fname>[^?]+)").unwrap();
    sqlite
        .captures(s)
        .map(|caps| caps.name("fname").unwrap().as_str().to_string())
}

fn find_legacy_sqlite_dbs(opt: &Opt) -> Vec<PathBuf> {
    let mut sources = vec![];
    if let Some(x) = extract(&opt._metadata_db_uri) {
        sources.push(PathBuf::from(x));
    }
    if let Some(x) = extract(&opt._data_db_uri) {
        sources.push(PathBuf::from(x));
    }
    sources
}

pub async fn run_shared_state(
    opt: Opt,
) -> Result<(SharedTasks, SharedState, Vec<ExecutorChannel>)> {
    let db_conn = DbConnection::connect(&opt.db_uri, opt.nr_connections).await?;
    let meta = MetaService::local_connection(&db_conn, opt.nr_connections).await?;

    let legacy_dbs = find_legacy_sqlite_dbs(&opt);
    if extract(&opt.db_uri).is_some() && legacy_dbs.len() == 2 {
        meta.maybe_migrate_sqlite_database(&legacy_dbs, &opt.db_uri)
            .await?;
    }

    let query_engine = QueryEngine::local_connection(&db_conn, opt.nr_connections).await?;

    meta.create_schema().await?;

    let mut commands = vec![];
    let mut commands2 = vec![];

    for _ in 0..opt.executor_threads {
        let (ctx, crx) = async_channel::bounded(1);
        let (rtx, rrx) = async_channel::bounded(1);
        commands.push(ExecutorChannel { tx: rtx, rx: crx });
        commands2.push(CoordinatorChannel { tx: ctx, rx: rrx });
    }

    let rpc_commands = commands2.clone();
    let state = Arc::new(Mutex::new(
        GlobalRpcState::new(meta, query_engine, rpc_commands).await?,
    ));

    let rpc = RpcService::new(state);

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));

    let (signal_tx, signal_rx) = async_channel::bounded(1);
    let sig_task = tokio::task::spawn(async move {
        let res = futures::select! {
            _ = sigterm.recv().fuse() => { debug!("Got SIGTERM"); DoRepeat::No },
            _ = sigint.recv().fuse() => { debug!("Got SIGINT"); DoRepeat::No },
            _ = sighup.recv().fuse() => { debug!("Got SIGHUP"); DoRepeat::Yes },
        };
        debug!("Got signal");
        signal_tx.send(()).await?;
        Ok(res)
    });

    let secret_commands = commands2.clone();

    let secret_shutdown = signal_rx.clone();
    // Spawn periodic hot-reload of secrets.  This doesn't load secrets immediately, though.
    let _secret_reader = tokio::task::spawn(async move {
        loop {
            futures::select! {
                _ = sleep(Duration::from_millis(1000)).fuse() => {},
                _ = secret_shutdown.recv().fuse() => {
                    break;
                }
            };

            let secrets = read_secrets().await;
            for cmd in &secret_commands {
                let v = secrets.clone();
                let payload = send_command!({
                    update_secrets(v).await;
                    Ok(())
                });
                cmd.send(payload).await.unwrap();
            }
        }
    });

    // rpc server should start listening only when all threads start
    let (readiness_tx, readiness_rx) = async_channel::bounded(opt.executor_threads);

    let start_wait = async move {
        for _id in 0..opt.executor_threads {
            readiness_rx.recv().await.unwrap();
        }
    };

    let rpc_rx = signal_rx.clone();
    let shutdown = async move {
        rpc_rx.recv().await.ok();
    };

    let rpc_task = crate::rpc::spawn(rpc, opt.rpc_listen_addr, start_wait, shutdown);
    debug!("RPC is ready. URL: {}", opt.rpc_listen_addr);

    crate::internal::init(
        opt.internal_routes_listen_addr,
        opt.webui,
        opt.rpc_listen_addr,
    );
    debug!(
        "Internal HTTP server is ready. URL: {}",
        opt.internal_routes_listen_addr
    );

    let state = SharedState {
        signal_rx,
        readiness_tx,
        api_listen_addr: opt.api_listen_addr,
        inspect_brk: opt.inspect_brk,
        executor_threads: opt.executor_threads,
        db: db_conn,
        nr_connections: opt.nr_connections,
    };

    let tasks = SharedTasks { rpc_task, sig_task };
    Ok((tasks, state, commands))
}

pub async fn run_on_new_localset(state: SharedState, command: ExecutorChannel) -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local.run_until(run(state, command)).await
}
