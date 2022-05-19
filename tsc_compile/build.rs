// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use deno_core::anyhow;
use deno_core::op;
use deno_core::Extension;
use deno_core::JsRuntime;
use deno_core::RuntimeOptions;
use std::env;
use std::path::PathBuf;

#[op]
fn read(path: String) -> Result<String> {
    let content = tsc_compile_build::read(&path);
    if !content.is_empty() {
        return Ok(content.to_string());
    }
    panic!("Unexpected file at build time: {}", path);
}
#[op]
fn write(_path: String, _content: String) -> Result<()> {
    Ok(())
}
#[op]
fn get_cwd() -> Result<String> {
    Ok("/there/is/no/cwd".to_string())
}
#[op]
fn dir_exists(_path: String) -> Result<bool> {
    Ok(false)
}
#[op]
fn diagnostic(msg: String) -> Result<()> {
    panic!("unexpected: {}", msg);
}
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let snapshot_path = out.join("SNAPSHOT.bin");

    let ext = Extension::builder()
        .ops(vec![
            diagnostic::decl(),
            read::decl(),
            write::decl(),
            get_cwd::decl(),
            dir_exists::decl(),
        ])
        .build();

    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![ext],
        will_snapshot: true,
        ..Default::default()
    });

    for (p, code) in tsc_compile_build::JS_FILES {
        runtime.execute_script(p, code).unwrap();
    }

    let snapshot = runtime.snapshot();
    std::fs::write(&snapshot_path, snapshot).unwrap();
}
