// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{Context, Result};
use std::collections::HashMap;
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
        let mut mods = HashMap::new();
        mods.insert(
            "@chiselstrike/api".to_string(),
            "export * from 'chisel://api/api.ts';".to_string(),
        );

        for (name, code) in api::SOURCES_D_TS.iter() {
            mods.insert(name.to_string(), code.to_string());
        }

        let opts = CompileOptions {
            extra_libs: mods,
            ..Default::default()
        };

        self.tsc
            .compile_urls(vec![url], opts)
            .await
            .context("Could not compile TypeScript")
    }
}
