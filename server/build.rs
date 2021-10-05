// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

fn main() -> std::io::Result<()> {
    tonic_build::compile_protos("../proto/chisel.proto")?;
    Ok(())
}
