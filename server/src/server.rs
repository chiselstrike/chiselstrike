// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::deno::init_deno;
use crate::query::{DbConnection, MetaService, QueryEngine};
use crate::rpc::{GlobalRpcState, RpcService};
use crate::runtime;
use crate::runtime::Runtime;
use anyhow::Result;
use async_mutex::Mutex;
use futures::future::LocalBoxFuture;
use futures::StreamExt;
use std::net::SocketAddr;
use std::panic;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::task::JoinHandle;

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "chiseld")]
pub struct Opt {
    /// API server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:8080")]
    api_listen_addr: SocketAddr,
    /// RPC server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    rpc_listen_addr: SocketAddr,
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
}

/// Whether an action should be repeated.
pub enum DoRepeat {
    Yes,
    No,
}

pub type Command = Box<dyn (FnOnce() -> LocalBoxFuture<'static, Result<()>>) + Send>;
pub type CommandResult = Result<()>;

#[derive(Clone)]
pub struct SharedState {
    signal_rx: async_channel::Receiver<()>,
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

async fn run(
    state: SharedState,
    command_rx: async_channel::Receiver<Command>,
    result_tx: async_channel::Sender<CommandResult>,
) -> Result<()> {
    let api_service = Arc::new(Mutex::new(ApiService::new()));

    // FIXME: We have to create one per thread. For now we only have
    // one thread, so this is fine.
    init_deno(state.inspect_brk).await?;

    let meta = MetaService::local_connection(&state.metadata_db).await?;

    let ts = meta.load_type_system().await?;

    let rt = Runtime::new(
        api_service.clone(),
        QueryEngine::local_connection(&state.data_db).await?,
        meta,
        ts,
    );
    runtime::set(rt);

    let command_task = tokio::task::spawn_local(async move {
        let mut stream = command_rx;
        while let Some(item) = stream.next().await {
            let res = item().await;
            result_tx.send(res).await.unwrap();
        }
    });

    let api_task = crate::api::spawn(api_service, state.api_listen_addr, async move {
        state.signal_rx.recv().await.ok();
    });
    state.readiness_tx.send(()).await?;

    api_task.await??;
    command_task.await?;
    Ok(())
}

pub async fn run_shared_state(
    opt: Opt,
) -> Result<(
    SharedTasks,
    SharedState,
    Vec<async_channel::Receiver<Command>>,
    Vec<async_channel::Sender<CommandResult>>,
)> {
    assert_eq!(
        opt.executor_threads, 1,
        "For now, only one executor thread supported"
    );

    let meta_conn = DbConnection::connect(&opt.metadata_db_uri).await?;
    let meta = MetaService::local_connection(&meta_conn).await?;

    meta.create_schema().await?;
    let ts = meta.load_type_system().await?;
    let (command_tx, command_rx) = async_channel::bounded(1);
    let command_tx = vec![command_tx];
    let command_rx = vec![command_rx];

    let (result_tx, result_rx) = async_channel::bounded(1);
    let result_tx = vec![result_tx];
    let result_rx = vec![result_rx];
    let state = Arc::new(Mutex::new(GlobalRpcState::new(ts, command_tx, result_rx)));

    let rpc = RpcService::new(state);

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));

    let (tx, rx) = async_channel::bounded(1);
    let sig_task = tokio::task::spawn(async move {
        use futures::FutureExt;
        let res = futures::select! {
            _ = sigterm.recv().fuse() => { info!("Got SIGTERM"); DoRepeat::No },
            _ = sigint.recv().fuse() => { info!("Got SIGINT"); DoRepeat::No },
            _ = sighup.recv().fuse() => { info!("Got SIGHUP"); DoRepeat::Yes },
        };
        info!("Got signal");
        tx.send(()).await?;
        Ok(res)
    });

    // rpc server should start listening only when all threads start
    let (readiness_tx, readiness_rx) = async_channel::bounded(opt.executor_threads);

    let start_wait = async move {
        for _id in 0..opt.executor_threads {
            readiness_rx.recv().await.unwrap();
        }
    };

    let rpc_rx = rx.clone();
    let shutdown = async move {
        rpc_rx.recv().await.ok();
    };

    let rpc_task = crate::rpc::spawn(rpc, opt.rpc_listen_addr, start_wait, shutdown);

    let state = SharedState {
        signal_rx: rx,
        readiness_tx,
        api_listen_addr: opt.api_listen_addr,
        inspect_brk: opt.inspect_brk,
        executor_threads: opt.executor_threads,
        data_db: DbConnection::connect(&opt.data_db_uri).await?,
        metadata_db: meta_conn,
    };

    let tasks = SharedTasks { rpc_task, sig_task };

    Ok((tasks, state, command_rx, result_tx))
}

pub async fn run_on_new_localset(
    state: SharedState,
    command_rx: async_channel::Receiver<Command>,
    result_tx: async_channel::Sender<CommandResult>,
) -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local.run_until(run(state, command_rx, result_tx)).await
}
