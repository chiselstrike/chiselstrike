#[macro_use]
extern crate log;

pub mod api;
pub mod types;

use api::ApiService;
use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{StatusRequest, StatusResponse, TypeDefinitionRequest, TypeDefinitionResponse};
use convert_case::{Case, Casing};
use serde_json::json;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};
use types::{Type, TypeSystem};

#[derive(StructOpt, Debug)]
#[structopt(name = "chiseld")]
struct Opt {
    /// API server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:3000")]
    api_listen_addr: String,
    /// RPC server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    rpc_listen_addr: String,
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

/// RPC service for Chisel server.
///
/// The RPC service provides a Protobuf-based interface for Chisel control
/// plane. For example, the service has RPC calls for managing types and
/// endpoints. The user-generated data plane endpoints are serviced with REST.
pub struct RpcService {
    api: Arc<Mutex<ApiService>>,
    type_system: Arc<Mutex<TypeSystem>>,
}

impl RpcService {
    pub fn new(api: Arc<Mutex<ApiService>>, type_system: Arc<Mutex<TypeSystem>>) -> Self {
        RpcService {
            api,
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
        let path = format!("/{}", name.to_case(Case::Snake));
        info!("Registered endpoint: '{}'", path);
        self.api.lock().await.get(
            &path,
            Box::new(|| {
                // Let's return an empty array because we don't do storage yet.
                let result = json!([]);
                result.to_string()
            }),
        );
        let response = chisel::TypeDefinitionResponse { message: name };
        Ok(Response::new(response))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    let api_addr = opt.api_listen_addr.parse()?;
    let api = Arc::new(Mutex::new(ApiService::new()));
    let ts = Arc::new(Mutex::new(TypeSystem::new()));
    let rpc = RpcService::new(api.clone(), ts);
    let rpc_addr = opt.rpc_listen_addr.parse()?;
    let rpc_task = tokio::spawn(async move {
        Server::builder()
            .add_service(ChiselRpcServer::new(rpc))
            .serve(rpc_addr)
            .await
    });
    let api_task = api::spawn(api.clone(), api_addr);
    let _ = tokio::try_join!(rpc_task, api_task)?;
    Ok(())
}
