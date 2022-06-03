// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::{EndPointCreationRequest, IndexCandidate};
use crate::cmd::apply::chiselc_output;
use crate::cmd::apply::parse_indexes;
use crate::project::Endpoint;
use anyhow::{anyhow, Context, Result};
use compile::compile_ts_code as swc_compile;
use endpoint_tsc::compile_endpoints;

pub(crate) async fn apply(
    endpoints: &[Endpoint],
    entities: &[String],
    types_string: &str,
    optimize: bool,
    auto_index: bool,
) -> Result<(Vec<EndPointCreationRequest>, Vec<IndexCandidate>)> {
    let mut endpoints_req = vec![];
    let mut index_candidates_req = vec![];
    let paths: Result<Vec<_>> = endpoints
        .iter()
        .map(|f| {
            f.file_path
                .to_str()
                .ok_or_else(|| anyhow!("Path is not UTF8"))
        })
        .collect();
    let mut output = compile_endpoints(&paths?)
        .await
        .context("parsing endpoints")?;
    for f in endpoints.iter() {
        let path = f.file_path.to_str().unwrap();
        let code = output.remove(path).unwrap();
        let code = types_string.to_owned() + &code;
        let code = if optimize {
            chiselc_output(code, "js", entities)?
        } else {
            swc_compile(code)?
        };
        endpoints_req.push(EndPointCreationRequest {
            path: f.name.clone(),
            code: code.clone(),
        });
        if auto_index {
            let mut indexes = parse_indexes(code.clone(), entities)?;
            index_candidates_req.append(&mut indexes);
        }
    }
    Ok((endpoints_req, index_candidates_req))
}
