// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use compile::compile_ts_code;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<()> {
    let p = "src/chisel.ts";

    // Every other file we use comes from the snapshot, so these
    // should be the only rerun-if-changed that we need.
    println!("cargo:rerun-if-changed={}", p);
    println!("cargo:rerun-if-changed=src/dts/lib.deno_core.d.ts");

    let mut map = compile_ts_code(p, Some("src/dts/lib.deno_core.d.ts"))?;
    let code = map.remove(p).unwrap();

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let p = out.join("chisel.js");
    fs::write(&p, code)?;
    Ok(())
}
