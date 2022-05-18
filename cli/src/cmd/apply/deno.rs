// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::IndexCandidate;
use crate::cmd::apply::chiselc_output;
use crate::cmd::apply::parse_indexes;
use anyhow::{anyhow, Context, Result};
use endpoint_tsc::compile_endpoints;
use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) async fn apply(
    endpoints: &[PathBuf],
    entities: &[String],
    optimize: bool,
    auto_index: bool,
) -> Result<(HashMap<String, String>, Vec<IndexCandidate>)> {
    let mut index_candidates_req = vec![];
    let paths: Result<Vec<_>> = endpoints
        .iter()
        .map(|f| f.to_str().ok_or_else(|| anyhow!("Path is not UTF8")))
        .collect();
    let mut output = compile_endpoints(&paths?)
        .await
        .context("parsing endpoints")?;
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
}
