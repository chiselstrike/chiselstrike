// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

pub fn chisel_js() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/chisel.js"))
}

pub fn chisel_d_ts() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/chisel.d.ts"))
}

pub fn endpoint_js() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/endpoint.js"))
}

pub fn worker_js() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/worker.js"))
}
