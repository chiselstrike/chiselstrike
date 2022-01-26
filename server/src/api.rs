// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::db::SqlValue;
use crate::prefix_map::PrefixMap;
use crate::query::engine::SqlWithArguments;
use anyhow::{Error, Result};
use futures::future::LocalBoxFuture;
use futures::ready;
use futures::stream::Stream;
use hyper::body::HttpBody;
use hyper::header::HeaderValue;
use hyper::service::{make_service_fn, service_fn};
use hyper::{HeaderMap, Request, Response, Server, StatusCode};
use socket2::{Domain, Protocol, Socket, Type};
use sqlx::Row;
use std::convert::Infallible;
use std::convert::TryFrom;
use std::io::Cursor;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
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

// RouteFns are passed between threads in rpc.rs, as they are built in the RPC threads and then
// distributed to the executors.
//
// At the same time, we need to make this clonable to be able to sanely use interior mutability
// inside the ApiService struct, so we need some reference counted type instead of a Box
type RouteFn = Arc<
    dyn Fn(Request<hyper::Body>) -> LocalBoxFuture<'static, Result<Response<Body>>> + Send + Sync,
>;

#[derive(Default, Clone, Debug)]
pub(crate) struct RequestPath {
    api_version: String,
    path: String,
}

impl RequestPath {
    pub(crate) fn api_version(&self) -> &str {
        &self.api_version
    }

    pub(crate) fn path(&self) -> &str {
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

/// Extracts the username of the logged-in user, or None if there was no login.
async fn get_username(req: &Request<hyper::Body>) -> Option<String> {
    let userid = match crate::auth::get_user(req).await {
        Ok(id) => id,
        Err(e) => {
            warn!("Token parsing error: {:?}", e);
            return None;
        }
    };

    let qeng = { crate::runtime::get().query_engine.clone() };

    match (userid, crate::auth::get_oauth_user_type()) {
        (None, _) => None,
        (Some(_), Err(e)) => {
            warn!("{:?}", e);
            None
        }
        (Some(id), Ok(user_type)) => {
            match qeng
                .fetch_one(SqlWithArguments {
                    sql: format!(
                        "SELECT username FROM {} WHERE id=$1",
                        user_type.backing_table()
                    ),
                    args: vec![SqlValue::String(id)],
                })
                .await
            {
                Err(e) => {
                    warn!("Username query error: {:?}", e);
                    None
                }
                Ok(row) => row.get("username"),
            }
        }
    }
}

/// API service for Chisel server.
#[derive(Default)]
pub(crate) struct ApiService {
    // Although we are on a TPC environment, this sync mutex should be fine. It will
    // never contend because the ApiService is thread-local. The alternative is a RefCell
    // with runtime checking, which is likely cheaper, but still this is safer and we don't
    // have to manually implement Send (which is unsafe).
    paths: Mutex<PrefixMap<RouteFn>>,
}

impl ApiService {
    /// Finds the right RouteFn for this request.
    fn find_route_fn<S: AsRef<Path>>(&self, request: S) -> Option<RouteFn> {
        match self.paths.lock().unwrap().longest_prefix(request.as_ref()) {
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
    pub(crate) fn add_route(&self, path: PathBuf, route_fn: RouteFn) {
        self.paths.lock().unwrap().insert(path, route_fn);
    }

    /// Remove all routes that have this prefix.
    pub(crate) fn remove_routes(&self, prefix: &Path) {
        self.paths.lock().unwrap().remove_prefix(prefix)
    }

    async fn route(&self, req: Request<hyper::Body>) -> hyper::http::Result<Response<Body>> {
        if let Some(route_fn) = self.find_route_fn(req.uri().path()) {
            if req.uri().path().starts_with("/__chiselstrike") {
                return match route_fn(req).await {
                    Ok(val) => Ok(val),
                    Err(err) => Self::internal_error(err),
                };
            }

            // TODO: Make this optional, for users who want to reject some OPTIONS requests.
            if req.method() == "OPTIONS" {
                return response_template().body("ok".to_string().into()); // Makes CORS preflights pass.
            }

            let pol = { crate::runtime::get().policies.clone() };
            let username = get_username(&req).await;
            let rp = match RequestPath::try_from(req.uri().path()) {
                Ok(rp) => rp,
                Err(_) => return ApiService::not_found(),
            };
            let is_allowed = {
                match pol.versions.get(rp.api_version()) {
                    None => {
                        return Self::internal_error(anyhow::anyhow!(
                            "found a route, but no version object for {}",
                            req.uri().path()
                        ))
                    }
                    Some(x) => x
                        .user_authorization
                        .is_allowed(username, rp.path().as_ref()),
                }
            };

            if !is_allowed {
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body("Unauthorized user\n".to_string().into());
            }
            return match route_fn(req).await {
                Ok(val) => Ok(val),
                Err(err) => Self::internal_error(err),
            };
        }
        ApiService::not_found()
    }

    fn not_found() -> hyper::http::Result<Response<Body>> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::default())
    }

    fn internal_error(err: anyhow::Error) -> hyper::http::Result<Response<Body>> {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(format!("{:?}\n", err).into())
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
    api: Rc<ApiService>,
    listen_addr: String,
    rx: async_channel::Receiver<()>,
) -> Result<Vec<tokio::task::JoinHandle<Result<(), hyper::Error>>>> {
    let mut tasks = Vec::new();
    let sock_addrs = listen_addr.to_socket_addrs()?;
    for addr in sock_addrs {
        info!("{} has address {:?}", listen_addr, addr);
        let api = api.clone();
        let rx = rx.clone();
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
                    rx.recv().await.ok();
                })
                .await;
            debug!("hyper shutdown");
            ret
        });
        tasks.push(task);
    }
    Ok(tasks)
}

pub(crate) fn response_template() -> http::response::Builder {
    Response::builder()
        // TODO: Let the user control this.
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "POST, PUT, GET, OPTIONS")
        .header(
            "Access-Control-Allow-Headers",
            "Content-Type,ChiselStrikeToken",
        )
}
