// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

#![cfg_attr(feature = "must_not_suspend", feature(must_not_suspend))]
#![cfg_attr(feature = "must_not_suspend", deny(must_not_suspend))]

#[macro_use]
extern crate log;

pub(crate) mod api;
pub(crate) mod auth;
pub(crate) mod db;
pub(crate) mod deno;
pub(crate) mod internal;
pub(crate) mod policies;
pub(crate) mod prefix_map;
pub(crate) mod query;
pub(crate) mod rcmut;
pub(crate) mod rpc;
pub(crate) mod runtime;
pub mod server;
pub(crate) mod types;

pub(crate) mod chisel {
    tonic::include_proto!("chisel");
}
