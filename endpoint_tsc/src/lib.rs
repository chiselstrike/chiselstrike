// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Write;
use tempfile::Builder;
use tempfile::NamedTempFile;
pub use tsc_compile;
use tsc_compile::CompileOptions;

pub struct Compiler {
    pub tsc: tsc_compile::Compiler,
}

impl Compiler {
    pub fn new(use_snapshot: bool) -> Compiler {
        let tsc = tsc_compile::Compiler::new(use_snapshot);
        Compiler { tsc }
    }

    pub async fn compile_endpoints(
        &mut self,
        file_names: &[&str],
    ) -> Result<HashMap<String, String>> {
        let mods = HashMap::from([(
            "@chiselstrike/api".to_string(),
            api::SOURCES.get("chisel.d.ts").unwrap().to_string(),
        )]);

        let chisel_global = include_str!("chisel-global.d.ts");
        let temp = to_tempfile(chisel_global, ".d.ts")?;

        let opts = CompileOptions {
            extra_default_lib: Some(temp.path().to_str().unwrap()),
            extra_libs: mods,
            ..Default::default()
        };

        self.tsc
            .compile_ts_code(file_names, opts)
            .await
            .context("could not compile TypeScript")
    }
}

fn to_tempfile(data: &str, suffix: &str) -> Result<NamedTempFile> {
    let mut f = Builder::new().suffix(suffix).tempfile()?;
    let inner = f.as_file_mut();
    inner.write_all(data.as_bytes())?;
    inner.flush()?;
    Ok(f)
}

pub async fn compile_endpoints(file_names: &[&str]) -> Result<HashMap<String, String>> {
    let mut compiler = Compiler::new(true);
    compiler.compile_endpoints(file_names).await
}
