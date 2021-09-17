use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

type RouteFn = Box<dyn Fn() -> String + Send>;

/// API service for Chisel server.
#[derive(Default)]
pub struct ApiService {
    gets: HashMap<String, RouteFn>,
}

impl ApiService {
    pub fn new() -> Self {
        Self {
            gets: HashMap::default(),
        }
    }

    pub fn get(&mut self, path: &str, route_fn: RouteFn) {
        self.gets.insert(path.to_string(), route_fn);
    }

    pub async fn route(&mut self, req: Request<Body>) -> hyper::http::Result<Response<Body>> {
        match *req.method() {
            Method::GET => {
                if let Some(route_fn) = self.gets.get(req.uri().path()) {
                    return Ok(Response::new(route_fn().into()));
                }
                ApiService::not_found(req)
            }
            _ => ApiService::not_found(req),
        }
    }

    fn not_found(_req: Request<Body>) -> hyper::http::Result<Response<Body>> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::default())
    }
}

pub fn spawn(
    api: Arc<Mutex<ApiService>>,
    addr: SocketAddr,
    shutdown: impl core::future::Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<Result<(), hyper::Error>> {
    let make_svc = make_service_fn(move |_conn| {
        let api = api.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                let api = api.clone();
                async move {
                    let mut api = api.lock().await;
                    api.route(req).await
                }
            }))
        }
    });
    let server = Server::bind(&addr).serve(make_svc);
    tokio::spawn(async move {
        let ret = server.with_graceful_shutdown(shutdown).await;
        info!("hyper shutdown");
        ret
    })
}
