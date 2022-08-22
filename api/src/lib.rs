// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use lazy_static::lazy_static;
use std::collections::HashMap;

macro_rules! source_js {
    ($stem:literal) => {
        (
            $stem,
            include_str!(concat!(env!("OUT_DIR"), "/", $stem, ".js")),
        )
    };
}

macro_rules! source_d_ts {
    ($stem:literal) => {
        (
            $stem,
            include_str!(concat!(env!("OUT_DIR"), "/", $stem, ".d.ts")),
        )
    };
}

lazy_static! {
    pub static ref SOURCES_JS: HashMap<&'static str, &'static str> = vec![
        source_js!("api"),
        source_js!("crud"),
        source_js!("datastore"),
        source_js!("endpoint"),
        source_js!("event"),
        source_js!("request"),
        source_js!("utils"),
        source_js!("worker"),
    ]
    .into_iter()
    .collect();
    pub static ref SOURCES_D_TS: HashMap<&'static str, &'static str> = vec![
        source_d_ts!("api"),
        source_d_ts!("crud"),
        source_d_ts!("datastore"),
        source_d_ts!("endpoint"),
        source_d_ts!("event"),
        source_d_ts!("request"),
        source_d_ts!("utils"),
        source_d_ts!("worker"),
    ]
    .into_iter()
    .collect();
}
