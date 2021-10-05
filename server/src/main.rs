// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

#[macro_use]
extern crate log;

pub mod api;
pub mod deno;
pub mod rpc;
pub mod store;
pub mod types;

use anyhow::Result;
use api::ApiService;
use rpc::RpcService;
use std::net::SocketAddr;
use std::sync::Arc;
use store::Store;
use structopt::StructOpt;
use tokio::sync::Mutex;

#[derive(StructOpt, Debug)]
#[structopt(name = "chiseld")]
struct Opt {
    /// API server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:3000")]
    api_listen_addr: SocketAddr,
    /// RPC server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    rpc_listen_addr: SocketAddr,
    /// Metadata database URI.
    #[structopt(short, long, default_value = "sqlite://chiseld.db?mode=rwc")]
    metadata_db_uri: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    let store = Store::connect(&opt.metadata_db_uri).await?;
    store.create_schema().await?;
    let ts = store.load_schema().await?;
    let store = Arc::new(Mutex::new(store));
    let api = Arc::new(Mutex::new(ApiService::new()));
    let ts = Arc::new(Mutex::new(ts));
    let rpc = RpcService::new(api.clone(), ts.clone(), store);
    for type_name in ts.lock().await.types.keys() {
        rpc.define_type_endpoints(type_name).await;
    }

    let sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let (tx, mut rx) = tokio::sync::watch::channel(());
    let sig_task = tokio::spawn(async move {
        use futures::StreamExt;
        let sigterm = tokio_stream::wrappers::SignalStream::new(sigterm);
        let sigint = tokio_stream::wrappers::SignalStream::new(sigint);
        let mut asig = futures::stream_select!(sigint, sigterm);
        asig.next().await;
        info!("Got signal");
        tx.send(())
    });

    let mut rpc_rx = rx.clone();
    let rpc_task = rpc::spawn(rpc, opt.rpc_listen_addr, async move {
        rpc_rx.changed().await.ok();
    });
    let api_task = api::spawn(api.clone(), opt.api_listen_addr, async move {
        rx.changed().await.ok();
    });

    let results = tokio::try_join!(rpc_task, api_task, sig_task)?;
    results.0?;
    results.1?;
    results.2?;
    Ok(())
}
