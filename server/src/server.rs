// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::deno::init_deno;
use crate::query::{DbConnection, MetaService, QueryEngine};
use crate::rpc::RpcService;
use crate::runtime;
use crate::runtime::Runtime;
use anyhow::Result;
use std::net::SocketAddr;
use std::panic;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::sync::Mutex;
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

#[derive(Clone)]
pub struct SharedState {
    signal_rx: tokio::sync::watch::Receiver<()>,
    readiness_tx: tokio::sync::mpsc::Sender<()>,
    metadata_db_uri: String,
    data_db_uri: String,
    api_listen_addr: SocketAddr,
    inspect_brk: bool,
    executor_threads: usize,
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

async fn run(state: SharedState) -> Result<()> {
    let api_service = Arc::new(Mutex::new(ApiService::new()));

    // FIXME: We have to create one per thread. For now we only have
    // one thread, so this is fine.
    init_deno(state.inspect_brk).await?;

    let meta_conn = DbConnection::connect(&state.metadata_db_uri).await?;
    let meta = MetaService::local_connection(&meta_conn).await?;

    let data_conn = DbConnection::connect(&state.data_db_uri).await?;
    let query_engine = QueryEngine::local_connection(&data_conn).await?;
    meta.create_schema().await?;
    let ts = meta.load_type_system().await?;

    let rt = Runtime::new(api_service.clone(), query_engine, meta, ts);
    runtime::set(rt);

    let mut rx = state.signal_rx.clone();
    let api_task = crate::api::spawn(api_service, state.api_listen_addr, async move {
        rx.changed().await.ok();
    });
    state.readiness_tx.send(()).await?;

    api_task.await??;
    Ok(())
}

pub async fn run_shared_state(opt: Opt) -> Result<(SharedTasks, SharedState)> {
    assert_eq!(
        opt.executor_threads, 1,
        "For now, only one executor thread supported"
    );

    let rpc = RpcService::new();

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));

    let (tx, rx) = tokio::sync::watch::channel(());
    let sig_task = tokio::task::spawn(async move {
        use futures::FutureExt;
        let res = futures::select! {
            _ = sigterm.recv().fuse() => { info!("Got SIGTERM"); DoRepeat::No },
            _ = sigint.recv().fuse() => { info!("Got SIGINT"); DoRepeat::No },
            _ = sighup.recv().fuse() => { info!("Got SIGHUP"); DoRepeat::Yes },
        };
        info!("Got signal");
        tx.send(())?;
        Ok(res)
    });

    // rpc server should start listening only when all threads start
    let (readiness_tx, mut readiness_rx) = tokio::sync::mpsc::channel(opt.executor_threads);

    let start_wait = async move {
        for _id in 0..opt.executor_threads {
            readiness_rx.recv().await;
        }
    };

    let mut rpc_rx = rx.clone();
    let shutdown = async move {
        rpc_rx.changed().await.ok();
    };

    let rpc_task = crate::rpc::spawn(rpc, opt.rpc_listen_addr, start_wait, shutdown);

    let state = SharedState {
        signal_rx: rx,
        readiness_tx,
        metadata_db_uri: opt.metadata_db_uri.clone(),
        data_db_uri: opt.data_db_uri.clone(),
        api_listen_addr: opt.api_listen_addr,
        inspect_brk: opt.inspect_brk,
        executor_threads: opt.executor_threads,
    };

    let tasks = SharedTasks { rpc_task, sig_task };

    Ok((tasks, state))
}

pub async fn run_on_new_localset(state: SharedState) -> Result<()> {
    let local = tokio::task::LocalSet::new();
    local.run_until(run(state)).await
}
