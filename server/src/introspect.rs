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
use anyhow::anyhow;
use anyhow::Result;
use futures::FutureExt;
use hyper::{Request, Response};
use openapi::{Info, Operations, Spec};
use std::collections::BTreeMap;
use std::sync::Arc;

const INTROSPECT_PATH: &str = "/__chiselstrike/introspect";

async fn introspect(req: Request<hyper::Body>) -> Result<Response<Body>> {
    let api = runtime::get().api.clone();

    let req_path = req
        .uri()
        .path()
        .strip_prefix(INTROSPECT_PATH)
        .ok_or_else(|| {
            anyhow!(
                "Invalid URI for route! This is an internal problem. Request: {:?}",
                req
            )
        })?;

    let mut api_version = req_path.trim_matches('/');
    if api_version.is_empty() {
        api_version = "__chiselstrike"
    }

    let mut paths = BTreeMap::new();
    let routes = api.routes();
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

pub(crate) fn init(api: &mut ApiService) {
    api.add_route(
        INTROSPECT_PATH.into(),
        Arc::new(move |req| { introspect(req) }.boxed_local()),
    );
}
