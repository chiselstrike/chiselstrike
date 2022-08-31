// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

pub use crate::auth::is_auth_entity_name;
pub use crate::opt::Opt;
pub use crate::server::{run, Restart};

pub(crate) type JsonObject = serde_json::Map<String, serde_json::Value>;

#[macro_use]
extern crate log;

pub(crate) mod apply;
pub(crate) mod auth;
pub(crate) mod datastore;
pub(crate) mod http;
pub(crate) mod internal;
pub(crate) mod kafka;
pub(crate) mod ops;
pub(crate) mod opt;
pub(crate) mod policies;
pub(crate) mod prefix_map;
pub(crate) mod rpc;
pub(crate) mod secrets;
pub(crate) mod server;
pub(crate) mod trunk;
pub(crate) mod types;
pub(crate) mod version;
pub(crate) mod worker;

#[allow(clippy::all)]
pub(crate) mod proto {
    tonic::include_proto!("chisel");
}
