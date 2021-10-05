// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::{
    EndPointCreationRequest, FieldDefinition, StatusRequest, TypeDefinitionRequest,
    TypeExportRequest,
};
use graphql_parser::schema::{parse_schema, Definition, TypeDefinition};
use std::fs;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "chisel")]
enum Opt {
    /// Shows information about ChiselStrike server status.
    Status,
    Type {
        #[structopt(subcommand)]
        cmd: TypeCommand,
    },
    EndPoint {
        #[structopt(subcommand)]
        cmd: EndPointCommand,
    },
}

#[derive(StructOpt, Debug)]
enum TypeCommand {
    /// Import types to the type system.
    Import {
        /// Type definition input file.
        filename: String,
    },
    /// Export the type system.
    Export,
}

#[derive(StructOpt, Debug)]
enum EndPointCommand {
    Create { path: String, filename: String },
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

async fn create_endpoint(
    client: &mut ChiselRpcClient<tonic::transport::Channel>,
    path: String,
    filename: String,
) -> Result<()> {
    let code = fs::read_to_string(filename)?;
    let request = tonic::Request::new(EndPointCreationRequest { path, code });
    let response = client.create_end_point(request).await?.into_inner();
    println!("End point defined: {}", response.message);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let mut client = ChiselRpcClient::connect("http://localhost:50051").await?;
    match opt {
        Opt::Status => {
            let request = tonic::Request::new(StatusRequest {});
            let response = client.get_status(request).await?.into_inner();
            println!("Server status is {}", response.message);
        }
        Opt::EndPoint { cmd } => match cmd {
            EndPointCommand::Create { path, filename } => {
                create_endpoint(&mut client, path, filename).await?
            }
        },
        Opt::Type { cmd } => match cmd {
            TypeCommand::Import { filename } => {
                let schema = fs::read_to_string(filename)?;
                let type_system = parse_schema::<String>(&schema)?;
                for def in &type_system.definitions {
                    match def {
                        Definition::TypeDefinition(TypeDefinition::Object(obj_def)) => {
                            let mut field_defs = Vec::default();
                            for field_def in &obj_def.fields {
                                field_defs.push(FieldDefinition {
                                    name: field_def.name.to_owned(),
                                    field_type: format!("{}", field_def.field_type.to_owned()),
                                });
                            }
                            let request = tonic::Request::new(TypeDefinitionRequest {
                                name: obj_def.name.to_owned(),
                                field_defs,
                            });
                            let response = client.define_type(request).await?.into_inner();
                            println!("Type defined: {}", response.message);
                        }
                        def => {
                            println!("Ignoring type definition: {:?}", def);
                        }
                    }
                }
            }
            TypeCommand::Export => {
                let request = tonic::Request::new(TypeExportRequest {});
                let response = client.export_types(request).await?.into_inner();
                for def in response.type_defs {
                    println!("class {} {{", def.name);
                    for field in def.field_defs {
                        println!("  {}: {};", field.name, field.field_type);
                    }
                    println!("}}");
                }
            }
        },
    }
    Ok(())
}
