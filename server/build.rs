// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use compile::compile_ts_code;
use std::env;
use std::fs;
use std::path::PathBuf;
use vergen::{vergen, Config, SemverKind};

fn build_proto() -> Result<()> {
    let proto = "../proto/chisel.proto";
    tonic_build::compile_protos(proto)?;
    println!("cargo:rerun-if-changed={}", proto);
    Ok(())
}

fn build_version() -> Result<()> {
    let mut config = Config::default();
    *config.git_mut().semver_kind_mut() = SemverKind::Lightweight;
    vergen(config)?;
    Ok(())
}

fn build_ts() -> Result<()> {
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

fn main() -> Result<()> {
    build_proto()?;
    build_version()?;
    build_ts()?;
    Ok(())
}
