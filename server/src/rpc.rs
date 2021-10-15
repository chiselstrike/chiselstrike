// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::deno;
use crate::store::{Store, StoreError};
use crate::types::{Field, ObjectType, TypeSystem, TypeSystemError};
use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{
    AddTypeRequest, AddTypeResponse, EndPointCreationRequest, EndPointCreationResponse,
    RemoveTypeRequest, RemoveTypeResponse, StatusRequest, StatusResponse, TypeExportRequest,
    TypeExportResponse,
};
use convert_case::{Case, Casing};
use futures::FutureExt;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};

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
    store: Box<Store>,
}

impl RpcService {
    pub fn new(
        api: Arc<Mutex<ApiService>>,
        type_system: Arc<Mutex<TypeSystem>>,
        store: Box<Store>,
    ) -> Self {
        RpcService {
            api,
            type_system,
            store,
        }
    }

    async fn create_js_endpoint(&self, path: &str, code: String) {
        let func = {
            let path = path.to_owned();
            move |req| deno::run_js(path.clone(), code.clone(), req).boxed_local()
        };
        self.api.lock().await.get(path, Box::new(func));
    }

    pub async fn define_type_endpoints(&self, path: &str) {
        info!("Registered endpoint: '{}'", path);
        // Let's return an empty array because we don't do storage yet.
        let result = json!([]);
        let code = format!(
            "function chisel(req) {{ return new Response(\"{}\"); }}",
            result
        );
        self.create_js_endpoint(path, code).await;
    }
}

impl From<StoreError> for Status {
    fn from(err: StoreError) -> Self {
        Status::internal(format!("{}", err))
    }
}

impl From<TypeSystemError> for Status {
    fn from(err: TypeSystemError) -> Self {
        Status::internal(format!("{}", err))
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

    /// Add a type.
    async fn add_type(
        &self,
        request: Request<AddTypeRequest>,
    ) -> Result<Response<AddTypeResponse>, Status> {
        let mut type_system = self.type_system.lock().await;
        let type_def = request.into_inner();
        let name = type_def.name;
        let snake_case_name = name.to_case(Case::Snake);
        let mut fields = Vec::new();
        for field in type_def.field_defs {
            let ty = type_system.lookup_type(&field.field_type)?;
            fields.push(Field {
                name: field.name.clone(),
                type_: ty,
                labels: field.labels,
            });
        }
        let ty = ObjectType {
            name: name.to_owned(),
            fields,
            backing_table: snake_case_name.clone(),
        };
        type_system.define_type(ty.to_owned())?;
        let store = &self.store;
        store.insert(ty).await?;
        let path = format!("/{}", snake_case_name);
        self.define_type_endpoints(&path).await;
        self.define_type_endpoints(&name).await;
        let response = chisel::AddTypeResponse { message: name };
        Ok(Response::new(response))
    }

    async fn remove_type(
        &self,
        request: tonic::Request<RemoveTypeRequest>,
    ) -> Result<tonic::Response<RemoveTypeResponse>, tonic::Status> {
        let mut type_system = self.type_system.lock().await;
        let request = request.into_inner();
        let name = request.type_name;
        type_system.remove_type(&name)?;
        let store = &self.store;
        store.remove(&name).await?;
        let response = chisel::RemoveTypeResponse {};
        Ok(Response::new(response))
    }

    async fn export_types(
        &self,
        _request: tonic::Request<TypeExportRequest>,
    ) -> Result<tonic::Response<TypeExportResponse>, tonic::Status> {
        let type_system = self.type_system.lock().await;
        let mut type_defs = vec![];
        for ty in type_system.types.values() {
            let mut field_defs = vec![];
            for field in &ty.fields {
                field_defs.push(chisel::FieldDefinition {
                    name: field.name.to_owned(),
                    field_type: field.type_.name().to_string(),
                    labels: field.labels.clone(),
                });
            }
            let type_def = chisel::TypeDefinition {
                name: ty.name.to_string(),
                field_defs,
            };
            type_defs.push(type_def);
        }
        let response = chisel::TypeExportResponse { type_defs };
        Ok(Response::new(response))
    }

    async fn create_end_point(
        &self,
        request: tonic::Request<EndPointCreationRequest>,
    ) -> Result<tonic::Response<EndPointCreationResponse>, tonic::Status> {
        let request = request.into_inner();
        let path = format!("/{}", request.path);
        let code = request.code;
        self.create_js_endpoint(&path, code).await;
        let response = EndPointCreationResponse { message: path };
        Ok(Response::new(response))
    }
}

pub fn spawn(
    rpc: RpcService,
    addr: SocketAddr,
    shutdown: impl core::future::Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<Result<(), tonic::transport::Error>> {
    tokio::task::spawn_local(async move {
        let ret = Server::builder()
            .add_service(ChiselRpcServer::new(rpc))
            .serve_with_shutdown(addr, shutdown)
            .await;
        info!("Tonic shutdown");
        ret
    })
}
