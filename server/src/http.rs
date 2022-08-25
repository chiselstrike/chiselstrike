// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::auth;
use crate::server::Server;
use crate::version::{Version, VersionJob};
use anyhow::{Context, Error, Result};
use deno_core::serde_v8;
use enclose::enclose;
use futures::stream::{FuturesUnordered, TryStreamExt};
use futures::FutureExt;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::future::ready;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use utils::TaskHandle;

pub async fn spawn(
    server: Arc<Server>,
    listen_addr: String,
) -> Result<(Vec<SocketAddr>, TaskHandle<Result<()>>)> {
    let servers = FuturesUnordered::new();
    let mut local_addrs = Vec::new();
    for addr in tokio::net::lookup_host(listen_addr).await? {
        let make_service = hyper::service::make_service_fn(enclose! {(server) move |_conn| {
            let service = hyper::service::service_fn(enclose!{(server) move |request| {
                handle_request(server.clone(), request).map(Ok::<_, Infallible>)
            }});
            ready(Ok::<_, Infallible>(service))
        }});

        // TODO: implement graceful shutdown?
        let incoming = hyper::server::conn::AddrIncoming::bind(&addr)?;
        local_addrs.push(incoming.local_addr());
        let server = hyper::Server::builder(incoming).serve(make_service);

        servers.push(server);
    }

    let task = tokio::task::spawn(async move {
        servers
            .try_collect()
            .await
            .context("Error while serving HTTP API")
    });
    Ok((local_addrs, TaskHandle(task)))
}

async fn handle_request(
    server: Arc<Server>,
    request: hyper::Request<hyper::Body>,
) -> hyper::Response<hyper::Body> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let mut response = try_handle_request(server, request)
        .await
        .unwrap_or_else(|err| handle_error(&method, &uri, err));
    add_default_headers(&mut response);
    debug!("{} {} -> {}", method, uri, response.status());
    response
}

async fn try_handle_request(
    server: Arc<Server>,
    request: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>> {
    let path = request.uri().path();
    let normalized_path = normalize_path(path);
    if normalized_path != path {
        return Ok(redirect_to_path(request.uri(), &normalized_path));
    }

    if path == "/" {
        return Ok(handle_index(server));
    }

    if *request.method() == hyper::Method::OPTIONS {
        return Ok(handle_options());
    }

    if let Some((version_id, routing_path)) = get_version_path(path) {
        if let Some(trunk_version) = server.trunk.get_trunk_version(version_id) {
            let version = trunk_version.version;
            let job_tx = trunk_version.job_tx;
            let routing_path = routing_path.into();
            return handle_version_request(server, version, job_tx, request, routing_path).await;
        }
    }

    Ok(handle_not_found())
}

#[derive(Debug)]
pub struct HttpRequestResponse {
    pub request: HttpRequest,
    pub response_tx: oneshot::Sender<HttpResponse>,
}

/// HTTP request that is passed to JavaScript.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: serde_v8::ZeroCopyBuf,
    pub routing_path: String,
    pub user_id: Option<String>,
}

/// HTTP response that is received from JavaScript.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: serde_v8::ZeroCopyBuf,
}

async fn handle_version_request(
    server: Arc<Server>,
    version: Arc<Version>,
    job_tx: mpsc::Sender<VersionJob>,
    request: hyper::Request<hyper::Body>,
    routing_path: String,
) -> Result<hyper::Response<hyper::Body>> {
    let (req_parts, req_body) = request.into_parts();
    let req_body = hyper::body::to_bytes(req_body).await?;

    // TODO: we don't authenticate the user!!!
    let user_id: Option<String> = req_parts
        .headers
        .get("ChiselUID")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.into());

    let username = auth::get_username_from_id(&server, &version, user_id.as_deref()).await;
    if !version
        .policy_system
        .user_authorization
        .is_allowed(username.as_deref(), &routing_path)
    {
        return Ok(handle_forbidden("Unauthorized user"));
    }

    {
        let secrets = server.secrets.read();
        if !version.policy_system.secret_authorization.is_allowed(
            &req_parts,
            &secrets,
            &routing_path,
        ) {
            return Ok(handle_forbidden("Invalid header authentication"));
        }
    }

    let http_request = HttpRequest {
        method: req_parts.method.as_str().into(),
        uri: req_parts.uri.to_string(),
        headers: req_parts
            .headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_str().unwrap_or("").into()))
            .collect(),
        // TODO: unnecessary copy from `Bytes` to `Vec<u8>`
        body: serde_v8::ZeroCopyBuf::from(req_body.to_vec()),
        routing_path,
        user_id,
    };

    // send the job and wait for the response
    let (response_tx, response_rx) = oneshot::channel();
    let job = VersionJob::Http(HttpRequestResponse {
        request: http_request,
        response_tx,
    });
    let _: Result<_, _> = job_tx.send(job).await;
    let http_response = response_rx.await.context("Request was aborted")?;

    // TODO: unnecessary copy from `ZeroCopyBuf` to `Vec<u8>`
    let response_body = hyper::Body::from(http_response.body.to_vec());
    let mut response = hyper::Response::new(response_body);

    *response.status_mut() = hyper::StatusCode::from_u16(http_response.status)
        .context("Response specified an invalid status code")?;
    for (name, value) in http_response.headers.into_iter() {
        let name = hyper::header::HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("Response header {:?} is not a valid header name", name))?;
        let value = hyper::header::HeaderValue::from_str(&value)
            .with_context(|| format!("Response header {:?} has invalid value", name))?;
        response.headers_mut().append(name, value);
    }

    Ok(response)
}

fn get_version_path(path: &str) -> Option<(&str, &str)> {
    lazy_static! {
        static ref REGEX: Regex = Regex::new(
            r"(?x)
            ^
            /(?P<version_id> [^/]*)
            (?P<routing_path> (/.*)?)
            $
        "
        )
        .unwrap();
    }
    let captures = REGEX.captures(path)?;
    let version_id = captures.name("version_id").unwrap().as_str();
    let routing_path = captures.name("routing_path").unwrap().as_str();
    Some((version_id, routing_path))
}

fn handle_index(server: Arc<Server>) -> hyper::Response<hyper::Body> {
    let mut versions = server.trunk.list_versions();
    versions.sort_unstable_by(|x, y| x.version_id.cmp(&y.version_id));

    let mut paths = serde_json::Map::new();
    paths.insert("/".into(), serde_json::json!({}));
    for version in versions.into_iter() {
        paths.insert(format!("/{}", version.version_id), serde_json::json!({}));
    }

    let swagger = serde_json::json!({
        "swagger": "2.0",
        "info": {
            "title": "ChiselStrike all routes",
            "version": env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT"),
        },
        "paths": paths,
    });

    let response = serde_json::to_string_pretty(&swagger).unwrap();
    hyper::Response::builder()
        .header("content-type", "application/json")
        .body(hyper::Body::from(response))
        .unwrap()
}

fn handle_options() -> hyper::Response<hyper::Body> {
    // Makes CORS preflights pass.
    // NOTE: This is a very heavy-handed way to handle CORS!
    hyper::Response::builder()
        .status(hyper::StatusCode::OK)
        .body(hyper::Body::from("ok"))
        .unwrap()
}

fn handle_not_found() -> hyper::Response<hyper::Body> {
    hyper::Response::builder()
        .status(hyper::StatusCode::NOT_FOUND)
        .body(hyper::Body::empty())
        .unwrap()
}

fn handle_forbidden(msg: &'static str) -> hyper::Response<hyper::Body> {
    hyper::Response::builder()
        .status(hyper::StatusCode::FORBIDDEN)
        .body(hyper::Body::from(msg))
        .unwrap()
}

fn handle_error(
    method: &hyper::Method,
    uri: &hyper::Uri,
    err: Error,
) -> hyper::Response<hyper::Body> {
    log::error!("Error while handling {} {}: {:?}", method, uri, err);
    hyper::Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(hyper::Body::empty())
        .unwrap()
}

fn normalize_path(path: &str) -> String {
    let mut normalized = String::with_capacity(path.len());
    normalized.push('/');
    for (i, segment) in path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .enumerate()
    {
        if i != 0 {
            normalized.push('/');
        }
        normalized.push_str(segment);
    }
    normalized
}

fn redirect_to_path(uri: &hyper::Uri, path: &str) -> hyper::Response<hyper::Body> {
    let mut parts = uri.clone().into_parts();

    let path_and_query = parts
        .path_and_query
        .unwrap_or_else(|| http::uri::PathAndQuery::from_static("/"));
    let path_and_query_str = match path_and_query.query() {
        Some(query) => format!("{}?{}", path, query),
        None => path.to_string(),
    };
    parts.path_and_query = Some(http::uri::PathAndQuery::from_str(&path_and_query_str).unwrap());

    let redirect_uri = hyper::Uri::from_parts(parts).unwrap();
    hyper::Response::builder()
        .status(hyper::StatusCode::PERMANENT_REDIRECT)
        .header("location", redirect_uri.to_string())
        .body(hyper::Body::empty())
        .unwrap()
}

fn add_default_headers(response: &mut hyper::Response<hyper::Body>) {
    // TODO: we probably should not add these headers to every response
    let default_headers = &[
        ("access-control-allow-origin", "*"),
        (
            "access-control-allow-methods",
            "POST, PUT, GET, OPTIONS, DELETE",
        ),
        ("access-control-allow-headers", "Content-Type,ChiselUID"),
    ];

    let headers = response.headers_mut();
    for (name, value) in default_headers.iter() {
        let name = hyper::header::HeaderName::from_static(name);
        if !headers.contains_key(&name) {
            headers.insert(name, hyper::header::HeaderValue::from_static(value));
        }
    }
}
