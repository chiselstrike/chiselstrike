// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
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

fn main() -> Result<()> {
    build_proto()?;
    build_version()?;
    Ok(())
}
