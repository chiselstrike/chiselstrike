pub mod types;

use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{StatusRequest, StatusResponse, TypeDefinitionRequest, TypeDefinitionResponse};
use std::sync::Arc;
use structopt::StructOpt;
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};
use types::{Type, TypeSystem};

#[derive(StructOpt, Debug)]
#[structopt(name = "chiseld")]
struct Opt {
    /// Server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    listen_addr: String,
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

/// RPC service for Chisel server.
///
/// The RPC service provides a Protobuf-based interface for Chisel control
/// plane. For example, the service has RPC calls for managing types and
/// endpoints. The user-generated data plane endpoints are serviced with REST.
#[derive(Debug)]
pub struct RpcService {
    type_system: Arc<Mutex<TypeSystem>>,
}

impl RpcService {
    pub fn new(type_system: Arc<Mutex<TypeSystem>>) -> Self {
        RpcService {
            type_system: type_system,
        }
    }
}

#[tonic::async_trait]
impl ChiselRpc for RpcService {
    /// Get Chisel server status.
    async fn get_status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let response = chisel::StatusResponse {
            message: "OK".to_string(),
        };
        Ok(Response::new(response))
    }

    /// Define a type.
    async fn define_type(
        &self,
        request: Request<TypeDefinitionRequest>,
    ) -> Result<Response<TypeDefinitionResponse>, Status> {
        let mut type_system = self.type_system.lock().await;
        let name = request.into_inner().name;
        type_system.define_type(Type {
            name: name.to_owned(),
        });
        let response = chisel::TypeDefinitionResponse { message: name };
        Ok(Response::new(response))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    let ts = Arc::new(Mutex::new(TypeSystem::new()));
    let chisel = RpcService::new(ts);
    Server::builder()
        .add_service(ChiselRpcServer::new(chisel))
        .serve(opt.listen_addr.parse()?)
        .await?;

    Ok(())
}
