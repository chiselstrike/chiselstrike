// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::deno::init_deno;
use crate::query::{MetaService, Store};
use crate::rpc::RpcService;
use crate::runtime;
use crate::runtime::Runtime;
use anyhow::Result;
use std::net::SocketAddr;
use std::panic;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::sync::Mutex;

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "chiseld")]
pub struct Opt {
    /// API server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:3000")]
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
}

/// Whether an action should be repeated.
pub enum DoRepeat {
    Yes,
    No,
}

async fn run(opt: Opt) -> Result<DoRepeat> {
    // FIXME: We have to create one per thread. For now we only have
    // one thread, so this is fine.
    init_deno(opt.inspect_brk).await?;
    let meta = MetaService::connect(&opt.metadata_db_uri).await?;
    let store = Store::connect(&opt.data_db_uri).await?;
    meta.create_schema().await?;
    let ts = meta.load_type_system().await?;
    let api = Arc::new(Mutex::new(ApiService::new()));
    let rpc = RpcService::new(api.clone());
    for type_name in ts.types.keys() {
        rpc.define_type_endpoints(type_name).await;
    }
    let rt = Runtime::new(store, meta, ts);
    runtime::set(rt);

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));
    let (tx, mut rx) = tokio::sync::watch::channel(());
    let sig_task = tokio::task::spawn_local(async move {
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

    let mut rpc_rx = rx.clone();
    let rpc_task = crate::rpc::spawn(rpc, opt.rpc_listen_addr, async move {
        rpc_rx.changed().await.ok();
    });
    let api_task = crate::api::spawn(api.clone(), opt.api_listen_addr, async move {
        rx.changed().await.ok();
    });

    let results = tokio::try_join!(rpc_task, api_task, sig_task)?;
    results.0?;
    results.1?;
    results.2
}

pub async fn run_on_new_localset(opt: Opt) -> Result<DoRepeat> {
    let local = tokio::task::LocalSet::new();
    local.run_until(run(opt)).await
}
