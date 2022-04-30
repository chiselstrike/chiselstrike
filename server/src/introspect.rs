// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

//! # API Introspection
//!
//! This module provides support for introspection of a ChiselStrike server.
//! The `init` function registers an introspection endpoint that returns
//! metadata of the ChiselStrike server endpoints as OpenAPI 2.0 format:
//!
//! https://swagger.io/specification/v2/

use crate::api::{response_template, ApiService, Body};
use crate::runtime;
use anyhow::Result;
use futures::FutureExt;
use hyper::{Request, Response};
use openapi::{Info, Operations, Spec};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

async fn introspect(req: Request<hyper::Body>) -> Result<Response<Body>> {
    let api = runtime::get().api.clone();

    let api_version = req.uri().path().trim_matches('/');
    let mut paths = BTreeMap::new();
    let routes = api.routes();
    // we have trimmed all / to avoid ////api_version, but now put it back to match the routes
    let prefix = format!("/{}", api_version);

    for route in routes {
        if route.starts_with(&prefix) {
            paths.insert(
                route,
                Operations {
                    get: None,
                    post: None,
                    put: None,
                    patch: None,
                    delete: None,
                    parameters: None,
                },
            );
        }
    }

    let info = match api.get_api_info(api_version) {
        Some(x) => x,
        None => {
            return ApiService::not_found();
        }
    };

    let spec = Spec {
        swagger: "2.0".to_string(),
        info: Info {
            title: info.name,
            version: info.tag,
            terms_of_service: None,
        },
        paths,
        definitions: BTreeMap::default(),
        schemes: None,
        host: None,
        base_path: None,
        consumes: None,
        produces: None,
        parameters: None,
        responses: None,
        security_definitions: None,
        tags: None,
    };
    Ok(response_template()
        .body(openapi::to_json(&spec).unwrap().into())
        .unwrap())
}

pub(crate) fn add_introspection<P: AsRef<Path>>(api: &ApiService, path: P) {
    let mut introspect_route = PathBuf::from("/");
    introspect_route.push(&path);
    api.add_route(
        introspect_route,
        Arc::new(move |req| { introspect(req) }.boxed_local()),
    );
}

pub(crate) fn init(api: &ApiService) {
    add_introspection(api, "/");
    add_introspection(api, "__chiselstrike");
}
