#[macro_use]
extern crate log;

pub mod api;
pub mod deno;
pub mod rpc;
pub mod types;

use api::ApiService;
use deno::DenoService;
use rpc::RpcService;
use std::net::SocketAddr;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::sync::Mutex;
use types::TypeSystem;

#[derive(StructOpt, Debug)]
#[structopt(name = "chiseld")]
struct Opt {
    /// API server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:3000")]
    api_listen_addr: SocketAddr,
    /// RPC server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    rpc_listen_addr: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    let api = Arc::new(Mutex::new(ApiService::new()));
    let ts = Arc::new(Mutex::new(TypeSystem::new()));
    let rpc = RpcService::new(api.clone(), ts);
    let mut deno = DenoService::new();

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

    deno.runtime
        .execute_script("<internal>", r#"Deno.core.print("Hello from Deno\n")"#)?;

    let results = tokio::try_join!(rpc_task, api_task, sig_task)?;
    results.0?;
    results.1?;
    results.2?;
    Ok(())
}
