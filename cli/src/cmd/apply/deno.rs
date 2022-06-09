// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::IndexCandidate;
use crate::cmd::apply::chiselc_output;
use crate::cmd::apply::parse_indexes;
use anyhow::{anyhow, Context, Result};
use compile::compile_ts_code as swc_compile;
use endpoint_tsc::compile_endpoints;
use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) async fn apply(
    endpoints: &[PathBuf],
    entities: &[String],
    types_string: &str,
    optimize: bool,
    auto_index: bool,
) -> Result<(HashMap<String, String>, Vec<IndexCandidate>)> {
    let mut endpoints_req = HashMap::new();
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
        let code = output.remove(path).unwrap();
        let code = types_string.to_owned() + &code;
        let code = if optimize {
            chiselc_output(code, "js", entities)?
        } else {
            swc_compile(code)?
        };
        endpoints_req.insert(f.display().to_string(), code.clone());
        if auto_index {
            let mut indexes = parse_indexes(code.clone(), entities)?;
            index_candidates_req.append(&mut indexes);
        }
    }
    Ok((endpoints_req, index_candidates_req))
}
