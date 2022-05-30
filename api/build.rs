// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;
use tsc_compile::compile_ts_code;
use tsc_compile::CompileOptions;

async fn compile(stem: &str) -> Result<()> {
    let src = &format!("src/{}.ts", stem);
    println!("cargo:rerun-if-changed={}", src);

    let opts = CompileOptions {
        emit_declarations: true,
        ..Default::default()
    };
    let mut map = compile_ts_code(&[src], opts).await?;
    let code = map.remove(src).unwrap();

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let js = format!("{}.js", stem);
    fs::write(&out.join(js), code)?;
    let dts = format!("{}.d.ts", stem);
    let src_dts = &format!("src/{}", dts);
    fs::write(&out.join(dts), map.remove(src_dts).unwrap())?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Every other file we use comes from the snapshot, so these
    // should be the only rerun-if-changed that we need.
    println!("cargo:rerun-if-changed=../third_party/deno/core/lib.deno_core.d.ts");

    compile("chisel").await?;
    compile("endpoint").await?;
    compile("worker").await?;
    Ok(())
}
