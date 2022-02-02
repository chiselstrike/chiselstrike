// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;
use tsc_compile::compile_ts_code;

fn main() -> Result<()> {
    let chisel_ts = "src/chisel.ts";

    // Every other file we use comes from the snapshot, so these
    // should be the only rerun-if-changed that we need.
    println!("cargo:rerun-if-changed={}", chisel_ts);
    println!("cargo:rerun-if-changed=../third_party/deno/core/lib.deno_core.d.ts");

    let mut map = compile_ts_code(chisel_ts, Default::default())?;
    let code = map.remove(chisel_ts).unwrap();

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(&out.join("chisel.js"), code)?;
    fs::write(
        &out.join("chisel.d.ts"),
        map.remove("src/chisel.d.ts").unwrap(),
    )?;
    Ok(())
}
