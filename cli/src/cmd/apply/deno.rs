// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::EndPointCreationRequest;
use crate::cmd::apply::chiselc_output;
use crate::project::Endpoint;
use anyhow::{anyhow, Context, Result};
use compile::compile_ts_code as swc_compile;
use endpoint_tsc::compile_endpoints;

pub(crate) async fn apply(
    endpoints: &[Endpoint],
    entities: &[String],
    types_string: &str,
    use_chiselc: bool,
) -> Result<Vec<EndPointCreationRequest>> {
    let mut endpoints_req = vec![];
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
        let code = if use_chiselc {
            chiselc_output(code, entities)?
        } else {
            swc_compile(code)?
        };
        endpoints_req.push(EndPointCreationRequest {
            path: f.name.clone(),
            code,
        });
    }
    Ok(endpoints_req)
}
