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
use deno_core::futures;
use futures::FutureExt;
use hyper::{Request, Response};
use openapi::v2::{Info, PathItem, Spec};
use openapi::OpenApi;
use std::collections::BTreeMap;
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
                PathItem {
                    get: None,
                    post: None,
                    put: None,
                    patch: None,
                    delete: None,
                    parameters: None,
                    head: None,
                    options: None,
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
            title: Some(info.name),
            version: Some(info.tag),
            contact: None,
            description: None,
            license: None,
            terms_of_service: None,
        },
        paths,
        definitions: Some(BTreeMap::default()),
        schemes: None,
        host: None,
        base_path: None,
        consumes: None,
        produces: None,
        parameters: None,
        responses: None,
        security_definitions: None,
        tags: None,
        external_docs: None,
        security: None,
    };
    let spec = OpenApi::V2(spec);
    Ok(response_template()
        .body(openapi::to_json(&spec).unwrap().into())
        .unwrap())
}

pub fn add_introspection(api: &ApiService, path: &str) {
    let introspect_route = format!("/{}", path);
    api.add_route(
        introspect_route,
        Arc::new(move |req| { introspect(req) }.boxed_local()),
    );
}

pub fn init(api: &ApiService) {
    add_introspection(api, "");
    add_introspection(api, "__chiselstrike");
}
