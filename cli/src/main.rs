use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::{FieldDefinition, StatusRequest, TypeDefinitionRequest, TypeExportRequest};
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
}

#[derive(StructOpt, Debug)]
enum TypeCommand {
    /// Define types in the type system.
    Define {
        /// Type definition input file.
        filename: String,
    },
    // Export the type system.
    Export,
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    let mut client = ChiselRpcClient::connect("http://localhost:50051").await?;
    match opt {
        Opt::Status => {
            let request = tonic::Request::new(StatusRequest {});
            let response = client.get_status(request).await?.into_inner();
            println!("Server status is {}", response.message);
        }
        Opt::Type { cmd } => match cmd {
            TypeCommand::Define { filename } => {
                let schema = fs::read_to_string(filename)?;
                let type_system = parse_schema::<String>(&schema)?;
                for def in &type_system.definitions {
                    match def {
                        Definition::TypeDefinition(TypeDefinition::Object(obj_def)) => {
                            let mut field_defs = Vec::default();
                            for field_def in &obj_def.fields {
                                field_defs.push(FieldDefinition {
                                    name: field_def.name.to_owned(),
                                    r#type: format!("{}", field_def.field_type.to_owned()),
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
                    println!("}}");
                }
            }
        },
    }
    Ok(())
}
