// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::env;
use std::path::PathBuf;

fn main() {
    // The default is to scan the entire package. The following is the
    // solution recommended in
    // https://doc.rust-lang.org/cargo/reference/build-scripts.html#cargorerun-if-changedpath
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let bundle_path = out_dir.join("generate_reflection.js");

    std::process::Command::new("deno")
        .args(vec![
            "bundle",
            "src/main.ts",
            &bundle_path.to_string_lossy(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("Could not run `deno` to bundle tsc_reflection tool");
}
