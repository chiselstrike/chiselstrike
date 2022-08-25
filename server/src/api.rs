// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::prefix_map::PrefixMap;
use anyhow::{Error, Result};
use deno_core::futures;
use futures::future::LocalBoxFuture;
use futures::ready;
use futures::stream::Stream;
use hyper::body::HttpBody;
use hyper::header::HeaderValue;
use hyper::service::{make_service_fn, service_fn};
use hyper::{HeaderMap, Request, Response, Server, StatusCode};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::convert::Infallible;
use std::convert::TryFrom;
use std::io::Cursor;
use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

type JsStream = Pin<Box<dyn Stream<Item = Result<Box<[u8]>>>>>;

pub enum Body {
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

// RouteFns are passed between threads in rpc.rs, as they are built in the RPC threads and then
// distributed to the executors.
//
// At the same time, we need to make this clonable to be able to sanely use interior mutability
// inside the ApiService struct, so we need some reference counted type instead of a Box
type RouteFn = Arc<
    dyn Fn(Request<hyper::Body>) -> LocalBoxFuture<'static, Result<Response<Body>>> + Send + Sync,
>;

type EventFn = Arc<
    dyn Fn(Option<Vec<u8>>, Option<Vec<u8>>) -> LocalBoxFuture<'static, Result<()>> + Send + Sync,
>;

#[derive(Default, Clone, Debug)]
pub struct RequestPath {
    api_version: String,
    path: String,
}

impl RequestPath {
    pub fn api_version(&self) -> &str {
        &self.api_version
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

thread_local! {
    static RP_REGEX: regex::Regex =
        regex::Regex::new("/(?P<version>[^/]+)(?P<path>/.+)").unwrap();
}

impl TryFrom<&str> for RequestPath {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let (api_version, path) = RP_REGEX.with(|rp| {
            let caps = rp.captures(value).ok_or(())?;
            let api_version = caps.name("version").ok_or(())?.as_str().to_string();
            let path = caps.name("path").ok_or(())?.as_str().to_string();
            Ok((api_version, path))
        })?;

        Ok(RequestPath { api_version, path })
    }
}

#[derive(Default, Debug, Clone)]
pub struct ApiInfo {
    pub name: String,
    pub tag: String,
}

impl ApiInfo {
    pub fn new(name: String, tag: String) -> Self {
        Self { name, tag }
    }

    pub fn all_routes() -> Self {
        let tag = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT").to_string();
        Self {
            name: "ChiselStrike all routes".into(),
            tag,
        }
    }

    pub fn chiselstrike() -> Self {
        let tag = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT").to_string();
        Self {
            name: "ChiselStrike Internal API".into(),
            tag,
        }
    }
}
pub type ApiInfoMap = HashMap<String, ApiInfo>;

/// API service for Chisel server.
pub struct ApiService {
    // Although we are on a TPC environment, this sync mutex should be fine. It will
    // never contend because the ApiService is thread-local. The alternative is a RefCell
    // with runtime checking, which is likely cheaper, but still this is safer and we don't
    // have to manually implement Send (which is unsafe).
    paths: Mutex<PrefixMap<RouteFn>>,
    event_handlers: Mutex<HashMap<String, EventFn>>,
    info: Mutex<ApiInfoMap>,
    debug: bool,
}

impl ApiService {
    pub fn new(mut info: ApiInfoMap, debug: bool) -> Self {
        info.insert("__chiselstrike".into(), ApiInfo::chiselstrike());
        info.insert("".into(), ApiInfo::all_routes());
        Self {
            paths: Default::default(),
            event_handlers: Default::default(),
            info: Mutex::new(info),
            debug,
        }
    }

    /// Finds the right RouteFn for this path.
    fn find_route_fn(&self, path: &str) -> Option<RouteFn> {
        let path = normalize_path(path);
        match self.paths.lock().unwrap().longest_prefix(&path) {
            None => None,
            Some((_, f)) => Some(f.clone()),
        }
    }

    /// Adds a route.
    ///
    /// Params:
    ///
    ///  * path: The path for the route, including any leading /
    ///  * code: A String containing the raw code of the endpoint, before any compilation.
    ///  * route_fn: the actual function to be executed, likely some call to deno.
    pub fn add_route(&self, path: String, route_fn: RouteFn) {
        self.paths.lock().unwrap().insert(path, route_fn);
    }

    /// Remove all routes that have this prefix.
    pub fn remove_routes(&self, prefix: &str) {
        self.paths.lock().unwrap().remove_prefix(prefix)
    }

    /// Finds the right EventFn for this topic.
    fn find_event_fn(&self, topic: &str) -> Option<EventFn> {
        match self.event_handlers.lock().unwrap().get(topic) {
            None => None,
            Some(f) => Some(f.clone()),
        }
    }

    pub fn add_event_handler(&self, path: String, event_fn: EventFn) {
        self.event_handlers.lock().unwrap().insert(path, event_fn);
    }

    pub fn update_api_info(&self, api_version: &str, info: ApiInfo) {
        crate::introspect::add_introspection(self, api_version);
        self.info.lock().unwrap().insert(api_version.into(), info);
    }

    pub fn get_api_info(&self, api_version: &str) -> Option<ApiInfo> {
        self.info.lock().unwrap().get(api_version).cloned()
    }

    pub fn routes(&self) -> Vec<String> {
        let mut result = vec![];
        for (path, _) in self.paths.lock().unwrap().iter() {
            result.push(path.to_string());
        }
        result
    }

    async fn route_impl(&self, req: Request<hyper::Body>) -> Result<Response<Body>> {
        if let Some(route_fn) = self.find_route_fn(req.uri().path()) {
            return route_fn(req).await;
        }
        ApiService::not_found()
    }

    async fn route(&self, req: Request<hyper::Body>) -> hyper::http::Result<Response<Body>> {
        match self.route_impl(req).await {
            Ok(val) => Ok(val),
            Err(err) => self.internal_error(err),
        }
    }

    pub async fn handle_event(
        &self,
        topic: String,
        key: Option<Vec<u8>>,
        value: Option<Vec<u8>>,
    ) -> Result<()> {
        let versions: Vec<String> = {
            let info = self.info.lock().unwrap();
            info.keys().cloned().collect()
        };
        for version in versions {
            // skip internal versions
            if version == "__chiselstrike" || version.is_empty() {
                continue;
            }
            let path = format!("/{}/{}", version, topic);
            if let Some(event_fn) = self.find_event_fn(&path) {
                if let Err(err) = event_fn(key.clone(), value.clone()).await {
                    println!("Warning: event handler for {} failed: {}", path, err);
                }
            } else {
                println!("Warning: event handler for {} not found.", path);
            }
        }
        Ok(())
    }

    pub fn not_found() -> Result<Response<Body>> {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::default())?)
    }

    fn internal_error(&self, err: anyhow::Error) -> hyper::http::Result<Response<Body>> {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(if self.debug {
                format!("{:?}\n", err).into()
            } else {
                log::error!("Internal server error: {:?}", err);
                Body::default()
            })
    }

    pub fn forbidden(err: &str) -> Result<Response<Body>> {
        Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(err.to_string().into())?)
    }
}

fn normalize_path(path: &str) -> String {
    let mut path = path.to_string();
    loop {
        let deduped = path.replace("//", "/");
        if deduped.len() < path.len() {
            path = deduped;
        } else {
            return path;
        }
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

/// Spawn an API server
///
/// # Arguments
/// * `api` - the API service of the server
/// * `listen_addr` - the listen address of the API server
/// * `shutdown` - channel that notifies the server of shutdown
pub fn spawn(
    api: Rc<ApiService>,
    listen_addr: String,
    shutdown: async_channel::Receiver<()>,
) -> Result<Vec<tokio::task::JoinHandle<Result<(), hyper::Error>>>> {
    let mut tasks = Vec::new();
    let sock_addrs = listen_addr.to_socket_addrs()?;
    for addr in sock_addrs {
        debug!("{} has address {:?}", listen_addr, addr);
        let api = api.clone();
        let shutdown = shutdown.clone();
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

        let make_svc = make_service_fn(move |_conn| {
            let api = api.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let api = api.clone();
                    async move { api.route(req).await }
                }))
            }
        });
        let server = Server::from_tcp(sk.into_tcp_listener())?
            .executor(LocalExec)
            .serve(make_svc);
        let task = tokio::task::spawn_local(async move {
            let ret = server
                .with_graceful_shutdown(async {
                    shutdown.recv().await.ok();
                })
                .await;
            debug!("hyper shutdown");
            ret
        });
        tasks.push(task);
    }
    Ok(tasks)
}

pub fn response_template() -> http::response::Builder {
    Response::builder()
        // TODO: Let the user control this.
        .header("Access-Control-Allow-Origin", "*")
        .header(
            "Access-Control-Allow-Methods",
            "POST, PUT, GET, OPTIONS, DELETE",
        )
        .header("Access-Control-Allow-Headers", "Content-Type,ChiselUID")
}
