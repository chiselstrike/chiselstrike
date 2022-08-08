// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use lazy_static::lazy_static;
use std::collections::HashMap;

macro_rules! out_file {
    ($file:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/", $file))
    }
}

lazy_static! {
    pub static ref SOURCES: HashMap<&'static str, &'static str> = vec![
        ("crud.ts", out_file!("crud.js")),
        ("chisel.ts", out_file!("chisel.js")),
        ("chisel.d.ts", out_file!("chisel.d.ts")),
        ("run.ts", out_file!("run.js")),
        ("routing.ts", out_file!("routing.js")),
        ("routing.d.ts", out_file!("routing.d.ts")),
        ("serve.ts", out_file!("serve.js")),
        ("special.ts", out_file!("special.js")),
        ("__chiselstrike.ts", out_file!("__chiselstrike.js")),
    ].into_iter().collect();
}
