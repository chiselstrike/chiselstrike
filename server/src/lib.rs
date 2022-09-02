// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

#![cfg_attr(feature = "must_not_suspend", feature(must_not_suspend))]
#![cfg_attr(feature = "must_not_suspend", deny(must_not_suspend))]

use once_cell::sync::Lazy;

pub use crate::auth::is_auth_entity_name;
pub use crate::server::{run_all, DoRepeat, Opt};

pub(crate) type JsonObject = serde_json::Map<String, serde_json::Value>;

pub(crate) static FEATURES: Lazy<Features> = Lazy::new(|| Features {
    typescript_policies: false,
});

/// Chiseld experimental features
#[derive(Default)]
struct Features {
    typescript_policies: bool,
}

macro_rules! send_command {
    ( $code:block ) => {{
        Box::new({ move || async move { $code }.boxed_local() })
    }};
}

#[macro_use]
extern crate log;

pub(crate) mod api;
pub(crate) mod apply;
pub(crate) mod auth;
pub(crate) mod datastore;
pub(crate) mod deno;
pub(crate) mod internal;
pub(crate) mod introspect;
pub(crate) mod kafka;
pub(crate) mod policies;
pub(crate) mod policy;
pub(crate) mod prefix_map;
pub(crate) mod rcmut;
pub(crate) mod rpc;
pub(crate) mod runtime;
pub(crate) mod secrets;
pub(crate) mod server;
pub(crate) mod types;
pub(crate) mod vecmap;

#[allow(clippy::all)]
pub(crate) mod proto {
    tonic::include_proto!("chisel");
}
