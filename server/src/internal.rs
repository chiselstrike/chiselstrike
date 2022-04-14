// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::chisel::{
    chisel_rpc_client::ChiselRpcClient, ChiselApplyRequest, EndPointCreationRequest,
};
use anyhow::Result;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use once_cell::sync::OnceCell;
use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;

/// If set, serve the web UI using this address for gRPC calls.
static SERVE_WEBUI: OnceCell<SocketAddr> = OnceCell::new();

fn response(body: &str, status: u16) -> Result<Response<Body>> {
    Ok(Response::builder()
        .status(status)
        .body(Body::from(body.to_string()))
        .unwrap())
}

#[derive(Serialize, Deserialize)]
struct WebUIPostBody {
    endpoint: String,
}

async fn webapply(body: Body, rpc_addr: &SocketAddr) -> Result<Response<Body>> {
    let body: WebUIPostBody = serde_json::from_slice(&hyper::body::to_bytes(body).await?)?;
    let mut client = ChiselRpcClient::connect(format!("http://{}", rpc_addr)).await?;
    client
        .apply(tonic::Request::new(ChiselApplyRequest {
            types: vec![],
            endpoints: vec![EndPointCreationRequest {
                path: "ep1".into(),
                code: body.endpoint,
            }],
            policies: vec![],
            allow_type_deletion: true,
            version: "dev".into(),
            version_tag: "dev".into(),
            app_name: "ChiselStrike WebUI".into(),
        }))
        .await?;
    response("applied", 200)
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
        ("/apply", Some(rpc_addr)) => webapply(req.into_body(), rpc_addr).await,
        ("/webui", Some(_)) => {
            let html = std::str::from_utf8(include_bytes!("webui.html"))?;
            response(html, 200)
        }
        _ => response("not found", 404),
    }
    .or_else(|e| response(&format!("{:?}", e), 500))
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
