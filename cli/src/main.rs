pub mod parser;

use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::{FieldDefinition, StatusRequest, TypeDefinitionRequest};
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
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    match opt {
        Opt::Status => {
            let mut client = ChiselRpcClient::connect("http://localhost:50051").await?;
            let request = tonic::Request::new(StatusRequest {});
            let response = client.get_status(request).await?.into_inner();
            println!("Server status is {}", response.message);
        }
        Opt::Type { cmd } => match cmd {
            TypeCommand::Define { filename } => {
                let type_system = parser::parse(&fs::read_to_string(filename)?)?;
                let mut client = ChiselRpcClient::connect("http://localhost:50051").await?;
                for type_def in &type_system.defs {
                    let mut field_defs = Vec::default();
                    for field_def in &type_def.fields {
                        field_defs.push(FieldDefinition {
                            name: field_def.name.to_owned(),
                            r#type: field_def.ty.to_owned(),
                        });
                    }
                    let request = tonic::Request::new(TypeDefinitionRequest {
                        name: type_def.name.to_owned(),
                        field_defs: field_defs,
                    });
                    let response = client.define_type(request).await?.into_inner();
                    println!("Type defined: {}", response.message);
                }
            }
        },
    }
    Ok(())
}
