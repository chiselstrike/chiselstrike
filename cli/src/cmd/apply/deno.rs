// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

#![allow(unused_imports)]

use crate::cmd::apply::chiselc_output;
use crate::cmd::apply::parse_indexes;
use crate::codegen::codegen_root_module;
use crate::events::FileTopicMap;
use crate::proto::{IndexCandidate, Module};
use crate::routes::FileRouteMap;
use anyhow::{anyhow, bail, Context, Result};
use endpoint_tsc::Compiler;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use url::Url;

pub(crate) async fn apply(
    route_map: FileRouteMap,
    topic_map: FileTopicMap,
    entities: &[String],
    optimize: bool,
    auto_index: bool,
) -> Result<(Vec<Module>, Vec<IndexCandidate>)> {
    let import_fn = |path: &Path| -> Result<String> {
        Url::from_file_path(path)
            .map(|url| url.to_string())
            .map_err(|_| anyhow!("Cannot convert file path {} to import URL", path.display()))
    };

    let root_code = codegen_root_module(&route_map, &topic_map, &import_fn)
        .context("Could not generate code for file-based routing and event topics")?;
    let (_root_file, root_url) = temporary_source_file("__root.", &root_code)?;

    let mut compiler = Compiler::new(true);
    let compiled = compiler
        .compile(root_url.clone())
        .await
        .context("Could not compile routes (using deno-style modules)")?;

    let mut modules = Vec::new();
    let mut index_candidates = Vec::new();
    for (url, mut code, _is_dts) in compiled.into_iter() {
        let mut url = Url::parse(&url.to_string()).unwrap();
        if url == root_url {
            url = Url::parse("file:///__root.ts").unwrap();
        }

        if optimize {
            code = chiselc_output(code, "js", entities)?;
        }

        if auto_index {
            let mut candidates = parse_indexes(code.clone(), entities)?;
            index_candidates.append(&mut candidates);
        }

        modules.push(Module {
            url: url.to_string(),
            code,
        });
    }

    Ok((modules, index_candidates))
}

fn temporary_source_file(name_prefix: &str, code: &str) -> Result<(tempfile::NamedTempFile, Url)> {
    let mut file = tempfile::Builder::new()
        .suffix(".ts")
        .prefix(name_prefix)
        .tempfile()
        .context("Could not create a temporary file")?;
    file.write_all(code.as_bytes())
        .context("Could not write to a temporary file")?;
    file.flush().context("Could not flush a temporary file")?;
    let url = Url::from_file_path(file.path()).unwrap();
    Ok((file, url))
}
