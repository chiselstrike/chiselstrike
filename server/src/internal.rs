// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use once_cell::sync::OnceCell;
use std::net::SocketAddr;

/// If set, serve the web UI using this address for gRPC calls.
static SERVE_WEBUI: OnceCell<SocketAddr> = OnceCell::new();

fn response(body: &str, status: u16) -> Result<Response<Body>> {
    Ok(Response::builder()
        .status(status)
        .body(Body::from(body.to_string()))
        .unwrap())
}

async fn route(req: Request<Body>) -> Result<Response<Body>> {
    match (req.uri().path(), SERVE_WEBUI.get()) {
        // Conceptually those checks are different and could eventually become
        // more complex functions. But for now we just return simple strings.
        // FWIW, K8s does not require us to return those specific strings.
        // Anything that returns a code 200 is enough.
        ("/status", _) => response("ok", 200),
        ("/readiness", _) => response("ready", 200),
        ("/liveness", _) => response("alive", 200),
        _ => response("not found", 404),
    }
}

/// Initialize ChiselStrike's internal routes.
///
/// Unlike the API server, it is strictly bound to 127.0.0.1. This is enough
/// for the Kubernetes checks to work, and it is one less thing for us to secure
/// and prevent DDoS attacks again - which is why this is a different server
pub(crate) fn init(addr: SocketAddr, serve_webui: bool, rpc_addr: SocketAddr) {
    if serve_webui {
        SERVE_WEBUI
            .set(rpc_addr)
            .expect("SERVE_WEBUI already initialized before internal::init()");
    }
    let make_svc = make_service_fn(|_conn| async {
        // service_fn converts our function into a `Service`
        Ok::<_, anyhow::Error>(service_fn(route))
    });

    tokio::task::spawn(async move {
        let server = Server::bind(&addr).serve(make_svc);
        server.await
    });
}
