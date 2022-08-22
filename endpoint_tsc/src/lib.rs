// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{Context, Result};
use std::collections::HashMap;
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
        let mut mods = HashMap::from([(
            "@chiselstrike/api".to_string(),
            "export * from 'chisel:///api.ts';".to_string(),
        )]);

        for (name, code) in api::SOURCES_D_TS.iter() {
            mods.insert(name.to_string(), code.to_string());
        }

        let opts = CompileOptions {
            extra_libs: mods,
            ..Default::default()
        };

        self.tsc
            .compile_ts_code(file_names, opts)
            .await
            .context("Could not compile TypeScript")
    }
}

pub async fn compile_endpoints(file_names: &[&str]) -> Result<HashMap<String, String>> {
    let mut compiler = Compiler::new(true);
    compiler.compile_endpoints(file_names).await
}
