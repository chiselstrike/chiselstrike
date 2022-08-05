// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

#![allow(unused_imports)]

use crate::proto::{IndexCandidate, Module};
use crate::cmd::apply::chiselc_output;
use crate::cmd::apply::parse_indexes;
use crate::routes::FileRouteMap;
use anyhow::{bail, Context, Result};
use endpoint_tsc::compile_endpoints;
use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) async fn apply(
    _route_map: FileRouteMap,
    _entities: &[String],
    _optimize: bool,
    _auto_index: bool,
) -> Result<(Vec<Module>, Vec<IndexCandidate>)> {
    bail!("not implemented")
    /*
    let mut index_candidates_req = vec![];
    let paths: Result<Vec<_>> = endpoints
        .iter()
        .map(|f| f.to_str().ok_or_else(|| anyhow!("Path is not UTF8")))
        .collect();
    let mut output = compile_endpoints(&paths?)
        .await
        .context("could not compile endpoints (using deno-style modules)")?;
    for f in endpoints.iter() {
        let path = f.to_str().unwrap();
        let orig = output.get_mut(path).unwrap();
        if optimize {
            *orig = chiselc_output(orig.to_string(), "js", entities)?;
        }

        if auto_index {
            let mut indexes = parse_indexes(orig.clone(), entities)?;
            index_candidates_req.append(&mut indexes);
        }
    }
    Ok((output, index_candidates_req))
    */
}
