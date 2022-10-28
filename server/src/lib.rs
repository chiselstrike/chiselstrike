// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use once_cell::sync::OnceCell;

pub use crate::opt::Opt;
pub use crate::server::run;
pub use authorization::is_auth_entity_name;

pub(crate) type JsonObject = serde_json::Map<String, serde_json::Value>;

pub(crate) static FEATURES: OnceCell<Features> = OnceCell::new();

pub(crate) fn feat_typescript_policies() -> bool {
    FEATURES
        .get()
        .map(|f| f.typescript_policies)
        .unwrap_or_default()
}

/// Chiseld experimental features
#[derive(Default)]
pub struct Features {
    typescript_policies: bool,
}

#[macro_use]
extern crate log;

#[macro_use]
pub(crate) mod error;

pub(crate) mod apply;
pub(crate) mod authentication;
pub(crate) mod authorization;
pub(crate) mod datastore;
pub(crate) mod http;
pub(crate) mod internal;
pub(crate) mod kafka;
pub(crate) mod module_loader;
pub mod ops;
pub(crate) mod opt;
pub(crate) mod policies;
mod policy;
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
