// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

pub fn chisel_js() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/chisel.js"))
}

pub fn chisel_d_ts() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/chisel.d.ts"))
}
