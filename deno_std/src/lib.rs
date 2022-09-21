// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use lazy_static::lazy_static;
use std::collections::HashMap;

// the included file is generated in build.rs
include!(concat!(env!("OUT_DIR"), "/", "SOURCES_JS.rs"));
