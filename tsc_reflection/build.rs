// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::env;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let bundle_path = out_dir.join("generate_reflection.js");

    let status = std::process::Command::new("deno")
        .args(vec![
            "bundle",
            "src/main.ts",
            &bundle_path.to_string_lossy(),
        ])
        .status()
        .expect("Could not run `deno` to bundle tsc_reflection tool");

    if !status.success() {
        panic!("deno bundle exited with non-zero status while bundling tsc_reflection tool");
    }
}
