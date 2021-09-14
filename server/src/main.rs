use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{StatusRequest, StatusResponse};
use structopt::StructOpt;
use tonic::{transport::Server, Request, Response, Status};

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
#[derive(Debug, Default)]
pub struct RpcService {}

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    let chisel = RpcService::default();

    Server::builder()
        .add_service(ChiselRpcServer::new(chisel))
        .serve(opt.listen_addr.parse()?)
        .await?;

    Ok(())
}
