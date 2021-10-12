// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::{Error, Result};
use futures::future::LocalBoxFuture;
use hyper::body::{Bytes, HttpBody};
use hyper::header::HeaderValue;
use hyper::service::{make_service_fn, service_fn};
use hyper::{HeaderMap, Method, Request, Response, Server, StatusCode};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::Mutex;

pub enum Body {
    Const(Option<Bytes>),
}

impl From<String> for Body {
    fn from(a: String) -> Self {
        Body::Const(Some(a.into()))
    }
}

impl Default for Body {
    fn default() -> Self {
        "".to_string().into()
    }
}

impl HttpBody for Body {
    type Data = Bytes;
    type Error = Error;

    fn poll_data(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let Body::Const(ref mut inner) = self.get_mut();
        Poll::Ready(inner.take().map(Ok))
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}

type RouteFn = Box<dyn Fn() -> LocalBoxFuture<'static, Result<Response<Body>>> + Send + Sync>;

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

    pub async fn route(
        &mut self,
        req: Request<hyper::Body>,
    ) -> hyper::http::Result<Response<Body>> {
        match *req.method() {
            Method::GET => {
                if let Some(route_fn) = self.gets.get(req.uri().path()) {
                    return match route_fn().await {
                        Ok(val) => Ok(val),
                        Err(err) => Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(format!("{:?}\n", err).into()),
                    };
                }
                ApiService::not_found()
            }
            _ => ApiService::not_found(),
        }
    }

    fn not_found() -> hyper::http::Result<Response<Body>> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::default())
    }
}

#[derive(Clone)]
struct LocalExec;

impl<F> hyper::rt::Executor<F> for LocalExec
where
    F: std::future::Future + 'static,
{
    fn execute(&self, fut: F) {
        tokio::task::spawn_local(fut);
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
    let server = Server::bind(&addr).executor(LocalExec).serve(make_svc);
    tokio::task::spawn_local(async move {
        let ret = server.with_graceful_shutdown(shutdown).await;
        info!("hyper shutdown");
        ret
    })
}
