// SPDX-FileCopyrightText: © 2021-2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use vergen::{vergen, Config, SemverKind};

fn main() -> Result<()> {
    let proto = "../proto/chisel.proto";
    tonic_build::compile_protos(proto)?;
    println!("cargo:rerun-if-changed={}", proto);
    let mut config = Config::default();
    *config.git_mut().semver_kind_mut() = SemverKind::Lightweight;
    vergen(config)?;
    Ok(())
}
