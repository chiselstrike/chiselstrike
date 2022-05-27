// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::chisel::EndPointCreationRequest;
use crate::cmd::apply::chiselc_output;
use crate::cmd::apply::output_to_string;
use crate::project::read_to_string;
use crate::project::Endpoint;
use anyhow::{Context, Result};
use compile::compile_ts_code as swc_compile;
use endpoint_tsc::compile_endpoint;

pub(crate) async fn apply<S: ToString + std::fmt::Display>(
    version: &S,
    endpoints: &[Endpoint],
    entities: &[String],
    types_string: &str,
    use_chiselc: bool,
) -> Result<Vec<EndPointCreationRequest>> {
    let mut endpoints_req = vec![];
    for f in endpoints.iter() {
        let ext = f.file_path.extension().unwrap().to_str().unwrap();
        let path = f.file_path.to_str().unwrap();

        let code = if ext == "ts" {
            let mut code = compile_endpoint(path)
                .await
                .with_context(|| format!("parsing endpoint /{}/{}", version, f.name))?;
            code.remove(path).unwrap()
        } else {
            read_to_string(&f.file_path)?
        };

        let code = types_string.to_owned() + &code;
        let code = if use_chiselc {
            let output = chiselc_output(code, entities)?;
            output_to_string(&output).unwrap()
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
