// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use tsc_compile::compile_ts_code;
use tsc_compile::CompileOptions;

// TODO: maybe we can import all .ts files in just a single invocation of tsc?

async fn compile(stem: &str, is_worker: bool) -> Result<()> {
    let src = &format!("src/{}.ts", stem);
    println!("cargo:rerun-if-changed={}", src);

    let opts = CompileOptions {
        emit_declarations: true,
        is_worker,
        ..Default::default()
    };
    let mut map = compile_ts_code(&[src], opts)
        .await
        .context(format!("Could not compile {:?}", src))?;
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

    compile("api", false).await?;
    compile("crud", false).await?;
    compile("datastore", false).await?;
    compile("endpoint", false).await?;
    compile("event", false).await?;
    compile("request", false).await?;
    compile("utils", false).await?;
    compile("worker", true).await?;

    Ok(())
}
