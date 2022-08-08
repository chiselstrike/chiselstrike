// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

#![allow(unused_imports)]

use crate::proto::{IndexCandidate, Module};
use crate::cmd::apply::chiselc_output;
use crate::cmd::apply::parse_indexes;
use crate::routes::{FileRouteMap, codegen_route_map};
use anyhow::{anyhow, bail, Context, Result};
use endpoint_tsc::Compiler;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use url::Url;

pub(crate) async fn apply(
    route_map: FileRouteMap,
    entities: &[String],
    optimize: bool,
    auto_index: bool,
) -> Result<(Vec<Module>, Vec<IndexCandidate>)> {
    let route_import_fn = |path: &Path| -> Result<String> {
        Url::from_file_path(path)
            .map(|url| url.to_string())
            .map_err(|_| anyhow!("Cannot convert file path {} to import URL", path.display()))
    };
    let route_map_code = codegen_route_map(&route_map, &route_import_fn)
        .context("Could not generate code for file-based routing")?;

    let mut route_map_file = tempfile::Builder::new()
        .suffix(".ts")
        .prefix("__route_map.")
        .tempfile()
        .context("Could not create a temporary file")?;
    route_map_file.write_all(route_map_code.as_bytes())
        .context("Could not write to a temporary file")?;
    route_map_file.flush()
        .context("Could not flush a temporary file")?;
    let route_map_url = Url::from_file_path(route_map_file.path()).unwrap();

    let mut compiler = Compiler::new(true);
    let compiled = compiler.compile(route_map_url.clone()).await
        .context("Could not compile routes (using deno-style modules)")?;

    let mut modules = Vec::new();
    let mut index_candidates = Vec::new();
    for (url, mut code, _is_dts) in compiled.into_iter() {
        let mut url = Url::parse(&url.to_string()).unwrap();
        if url == route_map_url {
            url = Url::parse("file:///__route_map.ts").unwrap();
        }

        if optimize {
            code = chiselc_output(code, "js", entities)?;
        }

        if auto_index {
            let mut candidates = parse_indexes(code.clone(), entities)?;
            index_candidates.append(&mut candidates);
        }

        println!("module {}", url);
        modules.push(Module { url: url.to_string(), code });
    }

    Ok((modules, index_candidates))
}
