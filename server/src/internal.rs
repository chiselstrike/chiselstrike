// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;

async fn route(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let mut response = Response::new(Body::empty());

    let uri = Path::new(req.uri().path());
    let component = match uri.file_name() {
        None => {
            *response.status_mut() = StatusCode::NOT_FOUND;
            return Ok(response);
        }
        Some(x) => x,
    };
    match component.to_str() {
        None => {
            *response.status_mut() = StatusCode::NOT_FOUND;
        }
        // Conceptually those checks are different and could eventually become
        // more complex functions. But for now we just return simple strings.
        // FWIW, K8s does not require us to return those specific strings.
        // Anything that returns a code 200 is enough.
        Some(x) => match x {
            "status" => {
                *response.body_mut() = "ok".into();
            }
            "readiness" => {
                *response.body_mut() = "ready".into();
            }
            "liveness" => {
                *response.body_mut() = "alive".into();
            }
            _ => {
                *response.status_mut() = StatusCode::NOT_FOUND;
            }
        },
    }
    Ok(response)
}

/// Initialize ChiselStrike's internal routes.
///
/// Unlike the API server, it is strictly bound to 127.0.0.1. This is enough
/// for the Kubernetes checks to work, and it is one less thing for us to secure
/// and prevent DDoS attacks again - which is why this is a different server
pub(crate) fn init(addr: SocketAddr) {
    let make_svc = make_service_fn(|_conn| async {
        // service_fn converts our function into a `Service`
        Ok::<_, Infallible>(service_fn(route))
    });

    tokio::task::spawn(async move {
        let server = Server::bind(&addr).serve(make_svc);
        server.await
    });
}
