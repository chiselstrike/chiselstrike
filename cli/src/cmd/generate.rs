use crate::proto::chisel_rpc_client::ChiselRpcClient;
use crate::proto::{type_msg::TypeEnum, DescribeRequest};
use anyhow::{anyhow, Result};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

pub(crate) async fn cmd_generate(
    server_url: String,
    output: PathBuf,
    version: String,
) -> Result<()> {
    let output = File::create(output)?;
    let mut client = ChiselRpcClient::connect(server_url).await?;
    let request = tonic::Request::new(DescribeRequest {});
    let response = execute!(client.describe(request).await);
    let version_def = response
        .version_defs
        .iter()
        .find(|def| def.version_id == version);
    if let Some(version_def) = version_def {
        for def in &version_def.type_defs {
            writeln!(&output, "export type {} = {{", def.name)?;
            writeln!(&output, "    id: string;")?;
            for field in &def.field_defs {
                let field_type = field.field_type()?;
                writeln!(
                    &output,
                    "    {}{}: {}{};",
                    field.name,
                    if field.is_optional { "?" } else { "" },
                    field_type,
                    field
                        .default_value
                        .as_ref()
                        .map(|d| if matches!(field_type, TypeEnum::String(_)) {
                            format!(r#" = "{}""#, d)
                        } else {
                            format!(" = {}", d)
                        })
                        .unwrap_or_else(|| "".into()),
                )?;
            }
            writeln!(&output, "}}")?;
        }
    }
    writeln!(&output, "export type Results<T> = {{")?;
    writeln!(&output, "    next_page: string;")?;
    writeln!(&output, "    prev_page: string;")?;
    writeln!(&output, "    results: T[];")?;
    writeln!(&output, "}}")?;
    Ok(())
}
