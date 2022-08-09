// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Write;
use tempfile::Builder;
use tempfile::NamedTempFile;
pub use tsc_compile;
use tsc_compile::CompileOptions;
use tsc_compile::FixedUrl;
use url::Url;

pub struct Compiler {
    pub tsc: tsc_compile::Compiler,
}

impl Compiler {
    pub fn new(use_snapshot: bool) -> Compiler {
        let tsc = tsc_compile::Compiler::new(use_snapshot);
        Compiler { tsc }
    }

    pub async fn compile(&mut self, url: Url) -> Result<Vec<(FixedUrl, String, bool)>> {
        let mods = HashMap::from([
            (
                "@chiselstrike/api".to_string(),
                "export * from 'chisel:///chisel.ts';".to_string(),
            ), (
                "chisel".to_string(),
                api::SOURCES.get("chisel.d.ts").unwrap().to_string(),
            ), (
                "crud".to_string(),
                api::SOURCES.get("crud.d.ts").unwrap().to_string(),
            ), (
                "routing".to_string(),
                api::SOURCES.get("routing.d.ts").unwrap().to_string(),
            ),
        ]);

        let chisel_global = include_str!("chisel-global.d.ts");
        let temp = to_tempfile(chisel_global, ".d.ts")?;

        let opts = CompileOptions {
            extra_default_lib: Some(temp.path().to_str().unwrap()),
            extra_libs: mods,
            ..Default::default()
        };

        self.tsc
            .compile_urls(vec![url], opts)
            .await
            .context("Could not compile TypeScript")
    }
}

fn to_tempfile(data: &str, suffix: &str) -> Result<NamedTempFile> {
    let mut f = Builder::new().suffix(suffix).tempfile()?;
    let inner = f.as_file_mut();
    inner.write_all(data.as_bytes())?;
    inner.flush()?;
    Ok(f)
}
