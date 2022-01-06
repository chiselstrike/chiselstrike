// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::deno;
use crate::deno::init_deno;
use crate::deno::{activate_endpoint, compile_endpoint};
use crate::query::{DbConnection, MetaService, QueryEngine};
use crate::rpc::{GlobalRpcState, RpcService};
use crate::runtime;
use crate::runtime::Runtime;
use crate::types::{Type, OAUTHUSER_TYPE_NAME};
use anyhow::Result;
use async_mutex::Mutex;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use futures::StreamExt;
use std::net::SocketAddr;
use std::panic;
use std::rc::Rc;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::task::JoinHandle;

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "chiseld", version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT"))]
pub struct Opt {
    /// user-visible API server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:8080")]
    api_listen_addr: SocketAddr,
    /// RPC server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    rpc_listen_addr: SocketAddr,
    /// Internal routes (for k8s) listen address
    #[structopt(short, long, default_value = "127.0.0.1:9090")]
    internal_routes_listen_addr: SocketAddr,
    /// Metadata database URI.
    #[structopt(short, long, default_value = "sqlite://chiseld.db?mode=rwc")]
    metadata_db_uri: String,
    /// Data database URI.
    #[structopt(short, long, default_value = "sqlite://chiseld-data.db?mode=rwc")]
    data_db_uri: String,
    /// Should we wait for a debugger before executing any JS?
    #[structopt(long)]
    inspect_brk: bool,
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
    api_listen_addr: SocketAddr,
    inspect_brk: bool,
    executor_threads: usize,
    data_db: DbConnection,
    metadata_db: DbConnection,
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

async fn run(state: SharedState, mut cmd: ExecutorChannel) -> Result<()> {
    init_deno(state.inspect_brk).await?;

    let meta = Rc::new(MetaService::local_connection(&state.metadata_db).await?);
    let ts = meta.load_type_system().await?;

    ts.refresh_types()?;

    let routes = meta.load_endpoints().await?;
    let policies = meta.load_policies().await?;

    for (path, code) in routes.iter() {
        let path = path.to_str().unwrap();
        compile_endpoint(path.to_string(), code.to_string()).await?;
        activate_endpoint(path);
    }

    let mut api_service = ApiService::default();
    for (path, _) in routes.iter() {
        let func = Arc::new({
            let path = path.to_str().unwrap().to_string();
            move |req| deno::run_js(path.clone(), req).boxed_local()
        });
        api_service.add_route(path.into(), func);
    }
    crate::auth::init(&mut api_service);
    let api_service = Rc::new(api_service);

    let oauth_user_type = match ts.lookup_builtin_type(OAUTHUSER_TYPE_NAME) {
        Ok(Type::Object(t)) => t,
        _ => anyhow::bail!("Internal error: type {} not found", OAUTHUSER_TYPE_NAME),
    };
    crate::deno::define_type(&oauth_user_type)?;
    let query_engine = Rc::new(QueryEngine::local_connection(&state.data_db).await?);
    let mut transaction = query_engine.start_transaction().await?;
    query_engine
        .create_table(&mut transaction, &oauth_user_type)
        .await?;
    QueryEngine::commit_transaction(transaction).await?;

    let rt = Runtime::new(api_service.clone(), query_engine, meta, ts, policies);
    runtime::set(rt);

    let command_task = tokio::task::spawn_local(async move {
        while let Some(item) = cmd.rx.next().await {
            let res = item().await;
            cmd.tx.send(res).await.unwrap();
        }
    });

    let api_task = crate::api::spawn(api_service, state.api_listen_addr, async move {
        state.signal_rx.recv().await.ok();
    })?;
    state.readiness_tx.send(()).await?;

    info!(
        "ChiselStrike is ready 🚀 - URL: http://{} ",
        state.api_listen_addr
    );

    api_task.await??;
    command_task.await?;
    Ok(())
}

// Receives commands, returns results
pub struct ExecutorChannel {
    pub rx: async_channel::Receiver<Command>,
    pub tx: async_channel::Sender<CommandResult>,
}

// Sends commands, receives results.
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

pub async fn run_shared_state(
    opt: Opt,
) -> Result<(SharedTasks, SharedState, Vec<ExecutorChannel>)> {
    let meta_conn = DbConnection::connect(&opt.metadata_db_uri).await?;
    let data_db = DbConnection::connect(&opt.data_db_uri).await?;

    let meta = MetaService::local_connection(&meta_conn).await?;
    let query_engine = QueryEngine::local_connection(&data_db).await?;

    meta.create_schema().await?;

    let mut commands = vec![];
    let mut commands2 = vec![];

    for _ in 0..opt.executor_threads {
        let (ctx, crx) = async_channel::bounded(1);
        let (rtx, rrx) = async_channel::bounded(1);
        commands.push(ExecutorChannel { tx: rtx, rx: crx });
        commands2.push(CoordinatorChannel { tx: ctx, rx: rrx });
    }

    let state = Arc::new(Mutex::new(
        GlobalRpcState::new(meta, query_engine, commands2).await?,
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
        data_db,
        metadata_db: meta_conn,
    };

    let tasks = SharedTasks { rpc_task, sig_task };
    Ok((tasks, state, commands))
}

pub async fn run_on_new_localset(state: SharedState, command: ExecutorChannel) -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local.run_until(run(state, command)).await
}
