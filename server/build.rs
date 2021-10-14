// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

fn main() -> std::io::Result<()> {
    let proto = "../proto/chisel.proto";
    tonic_build::compile_protos(proto)?;
    println!("cargo:rerun-if-changed={}", proto);
    Ok(())
}
