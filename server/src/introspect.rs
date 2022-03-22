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
use std::sync::Arc;

const INTROSPECT_PATH: &str = "/__chiselstrike/introspect";

async fn introspect(_req: Request<hyper::Body>) -> Result<Response<Body>> {
    let api = runtime::get().api.clone();
    let mut paths = BTreeMap::new();
    let routes = api.routes();
    for route in routes {
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
    let spec = Spec {
        swagger: "2.0".to_string(),
        info: Info {
            title: "ChiselStrike API".to_string(),
            version: "0.0.0".to_string(),
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
