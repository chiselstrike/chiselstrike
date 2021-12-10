// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::{Error, Result};
use async_mutex::Mutex;
use futures::future::LocalBoxFuture;
use futures::ready;
use futures::stream::Stream;
use hyper::body::HttpBody;
use hyper::header::HeaderValue;
use hyper::service::{make_service_fn, service_fn};
use hyper::{HeaderMap, Request, Response, Server, StatusCode};
use socket2::{Domain, Protocol, Socket, Type};
use std::convert::Infallible;
use std::io::Cursor;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

type JsStream = Pin<Box<dyn Stream<Item = Result<Box<[u8]>>>>>;

pub(crate) enum Body {
    Const(Option<Box<[u8]>>),
    Stream(JsStream),
}

impl From<String> for Body {
    fn from(a: String) -> Self {
        Body::Const(Some(a.into_boxed_str().into_boxed_bytes()))
    }
}

impl Default for Body {
    fn default() -> Self {
        "".to_string().into()
    }
}

impl HttpBody for Body {
    type Data = Cursor<Box<[u8]>>;
    type Error = Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let r = match self.get_mut() {
            Body::Const(ref mut inner) => inner.take().map(|x| Ok(Cursor::new(x))),
            Body::Stream(ref mut stream) => {
                ready!(stream.as_mut().poll_next(cx)).map(|x| x.map(Cursor::new))
            }
        };
        Poll::Ready(r)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}

type RouteFn = Box<
    dyn Fn(Request<hyper::Body>) -> LocalBoxFuture<'static, Result<Response<Body>>> + Send + Sync,
>;

#[derive(Default)]
pub(crate) struct RoutePaths {
    // Kept reverse sorted so that if an entry is a prefix of another,
    // it comes later. This makes it easy to find which entry shares
    // the longest prefix with a request.
    //
    // Both insertions and search are O(n) and could be O(request path
    // size) with a tree, but that is probably OK with a normal number
    // of endpoints.
    paths: Vec<(PathBuf, String, RouteFn)>,
}

impl RoutePaths {
    fn longest_prefix<S: AsRef<Path>>(&self, request: S) -> Option<&RouteFn> {
        let request = request.as_ref();
        for p in &self.paths {
            if request.starts_with(&p.0) {
                return Some(&p.2);
            }
        }
        None
    }

    /// Adds a route.
    ///
    /// Params:
    ///
    ///  * path: The path for the route, including any leading /
    ///  * code: A String containing the raw code of the endpoint, before any compilation.
    ///  * route_fn: the actual function to be executed, likely some call to deno.
    pub(crate) fn add_route<S: AsRef<str>, C: ToString>(
        &mut self,
        path: S,
        code: C,
        route_fn: RouteFn,
    ) {
        let path: PathBuf = path.as_ref().into();
        let pos = self.paths.binary_search_by(|p| path.cmp(&p.0));
        let elem = (path, code.to_string(), route_fn);
        match pos {
            Ok(pos) => {
                self.paths[pos] = elem;
            }
            Err(pos) => {
                self.paths.insert(pos, elem);
            }
        }
    }

    pub(crate) fn route_data(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.paths.iter().map(|x| (x.0.as_path(), x.1.as_str()))
    }

    /// Remove all routes that match this regular expression, and return
    /// the amount of routes removed.
    pub(crate) fn remove_routes(&mut self, path: regex::Regex) -> usize {
        let before = self.paths.len();
        self.paths.retain(|x| {
            let s = x.0.clone().into_os_string().into_string().unwrap();
            !path.is_match(&s)
        });
        before - self.paths.len()
    }
}

/// API service for Chisel server.
#[derive(Default)]
pub(crate) struct ApiService {
    paths: RoutePaths,
}

impl ApiService {
    pub(crate) fn new(paths: RoutePaths) -> Self {
        Self { paths }
    }

    fn longest_prefix<S: AsRef<Path>>(&self, request: S) -> Option<&RouteFn> {
        self.paths.longest_prefix(request)
    }

    pub(crate) fn add_route<S: AsRef<str>, C: ToString>(
        &mut self,
        path: S,
        code: C,
        route_fn: RouteFn,
    ) {
        self.paths.add_route(path, code, route_fn)
    }

    /// Remove all routes that match this regular expression, and return
    /// the amount of routes removed.
    pub(crate) fn remove_routes(&mut self, path: regex::Regex) -> usize {
        self.paths.remove_routes(path)
    }

    pub(crate) async fn route(
        &mut self,
        req: Request<hyper::Body>,
    ) -> hyper::http::Result<Response<Body>> {
        if let Some(route_fn) = self.longest_prefix(req.uri().path()) {
            let runtime = crate::runtime::get().await;
            match req.headers().get("ChiselStrikeToken") {
                Some(token) => {
                    let token = token.to_str();
                    if token.is_err()
                        || crate::runtime::get()
                            .await
                            .meta
                            .get_username(token.unwrap())
                            .await
                            .is_err()
                    {
                        return Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .body("Token not recognized\n".to_string().into());
                    }
                }
                None => {
                    let authorize = &runtime.policies.authorize;
                    if authorize.contains(req.uri().path().strip_prefix('/').unwrap_or("")) {
                        return Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .body("Must be logged in\n".to_string().into());
                    }
                }
            }
            return match route_fn(req).await {
                Ok(val) => Ok(val),
                Err(err) => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(format!("{:?}\n", err).into()),
            };
        }
        ApiService::not_found()
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

pub(crate) fn spawn(
    api: Arc<Mutex<ApiService>>,
    addr: SocketAddr,
    shutdown: impl core::future::Future<Output = ()> + 'static,
) -> Result<tokio::task::JoinHandle<Result<(), hyper::Error>>> {
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

    let domain = if addr.is_ipv6() {
        Domain::ipv6()
    } else {
        Domain::ipv4()
    };
    let sk = Socket::new(domain, Type::stream(), Some(Protocol::tcp()))?;
    let addr = socket2::SockAddr::from(addr);
    sk.set_reuse_port(true)?;
    sk.bind(&addr)?;
    sk.listen(1024)?;

    let server = Server::from_tcp(sk.into_tcp_listener())?
        .executor(LocalExec)
        .serve(make_svc);
    Ok(tokio::task::spawn_local(async move {
        let ret = server.with_graceful_shutdown(shutdown).await;
        info!("hyper shutdown");
        ret
    }))
}
