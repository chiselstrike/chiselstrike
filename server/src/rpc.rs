// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::deno;
use crate::policies::{LabelPolicies, Policy};
use crate::query::QueryError;
use crate::runtime;
use crate::types::{Field, ObjectType, TypeSystem, TypeSystemError};
use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{
    AddTypeRequest, AddTypeResponse, EndPointCreationRequest, EndPointCreationResponse,
    EndPointRemoveRequest, EndPointRemoveResponse, PolicyUpdateRequest, PolicyUpdateResponse,
    RestartRequest, RestartResponse, StatusRequest, StatusResponse, TypeExportRequest,
    TypeExportResponse,
};
use convert_case::{Case, Casing};
use futures::FutureExt;
use log::debug;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};
use yaml_rust::YamlLoader;

pub mod chisel {
    tonic::include_proto!("chisel");
}

// First, guarantees that a single RPC command is executing throught the lock that goes over a
// static instance of this.
//
// But also for things like type, we need to have a copy of the current view of the system so that
// we can validate changes against. We don't want to wait until the executors error out on adding
// types (especially because they may error out in different ways due to ordering).
//
// Policies and endpoints are stateless so we don't need a global copy.
pub struct GlobalRpcState {
    type_system: TypeSystem,
}

impl GlobalRpcState {
    pub fn new(type_system: TypeSystem) -> Self {
        Self { type_system }
    }
}

/// RPC service for Chisel server.
///
/// The RPC service provides a Protobuf-based interface for Chisel control
/// plane. For example, the service has RPC calls for managing types and
/// endpoints. The user-generated data plane endpoints are serviced with REST.
pub struct RpcService {
    state: Arc<Mutex<GlobalRpcState>>,
}

impl RpcService {
    pub fn new(state: Arc<Mutex<GlobalRpcState>>) -> Self {
        Self { state }
    }

    async fn create_js_endpoint(&self, path: &str, code: String) {
        deno::define_endpoint(path, code).await;
        let func = {
            let path = path.to_owned();
            move |req| deno::run_js(path.clone(), req).boxed_local()
        };

        let runtime = &mut runtime::get().await;
        runtime.api.lock().await.add_route(path, Box::new(func));
    }
}

impl From<QueryError> for Status {
    fn from(err: QueryError) -> Self {
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
        let mut state = self.state.lock().await;
        let type_system = &mut state.type_system;

        let runtime = &mut runtime::get().await;

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
        match type_system.add_type(ty.to_owned()) {
            Ok(_) => {
                // FIXME: Consistency between metadata and backing store updates.
                let meta = &runtime.meta;
                meta.insert(ty.clone()).await?;
                let query_engine = &runtime.query_engine;
                query_engine.create_table(ty).await?;
            }
            Err(TypeSystemError::TypeAlreadyExists) => {
                type_system.replace_type(ty)?;
            }
            Err(e) => return Err(e.into()),
        }

        runtime.type_system.update(type_system);
        let response = chisel::AddTypeResponse { message: name };
        Ok(Response::new(response))
    }

    async fn export_types(
        &self,
        _request: tonic::Request<TypeExportRequest>,
    ) -> Result<tonic::Response<TypeExportResponse>, tonic::Status> {
        let state = self.state.lock().await;
        let type_system = &state.type_system;

        let mut type_defs = vec![];
        use itertools::Itertools;
        for ty in type_system
            .types
            .values()
            .sorted_by(|x, y| x.name.cmp(&y.name))
        {
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

    async fn remove_end_point(
        &self,
        request: Request<EndPointRemoveRequest>,
    ) -> Result<Response<EndPointRemoveResponse>, Status> {
        let request = request.into_inner();

        let regex = regex::Regex::new(&request.path.unwrap_or_else(|| ".*".into()))
            .map_err(|x| Status::internal(format!("invalid regex: {}", x)))?;

        let runtime = &mut runtime::get().await;
        let removed = runtime.api.lock().await.remove_routes(regex) as i32;

        let response = EndPointRemoveResponse { removed };
        Ok(Response::new(response))
    }

    async fn restart(
        &self,
        _request: tonic::Request<RestartRequest>,
    ) -> Result<tonic::Response<RestartResponse>, tonic::Status> {
        let ok = nix::sys::signal::raise(nix::sys::signal::Signal::SIGHUP).is_ok();
        Ok(Response::new(RestartResponse { ok }))
    }

    async fn policy_update(
        &self,
        request: tonic::Request<PolicyUpdateRequest>,
    ) -> Result<tonic::Response<PolicyUpdateResponse>, tonic::Status> {
        let request = request.into_inner();

        let docs = YamlLoader::load_from_str(&request.policy_config)
            .map_err(|err| Status::internal(format!("{}", err)))?;

        // FIXME: only transform implemented
        let mut policies = LabelPolicies::default();

        for config in docs.iter() {
            for label in config["labels"].as_vec().get_or_insert(&[].into()).iter() {
                let name = label["name"].as_str().ok_or_else(|| {
                    Status::internal(format!(
                        "couldn't parse yaml: label without a name: {:?}",
                        label
                    ))
                })?;
                debug!("Applying policy for label {:?}", name);

                match label["transform"].as_str() {
                    Some("anonymize") => {
                        let pattern = label["except_uri"].as_str().unwrap_or("^$"); // ^$ never matches; each path has at least a '/' in it.
                        policies.insert(
                            name.to_owned(),
                            Policy {
                                transform: crate::policies::anonymize,
                                except_uri: regex::Regex::new(pattern).map_err(|e| {
                                    Status::internal(format!(
                                        "error '{}' parsing regular expression '{}'",
                                        e, pattern
                                    ))
                                })?,
                            },
                        );
                        Ok(())
                    }
                    Some(x) => Err(Status::internal(format!(
                        "unknown transform: {} for label {}",
                        x, name
                    ))),
                    None => Ok(()),
                }?;
            }
            for _endpoint in config["endpoints"].as_vec().iter() {
                log::info!("endpoint behavior not yet implemented");
            }
        }

        runtime::get().await.policies = policies;

        // FIXME: return number of effective changes? Probably depends on how we implement
        // terraform-like workflow (x added, y removed, z modified)
        Ok(Response::new(PolicyUpdateResponse {
            message: "ok".to_owned(),
        }))
    }
}

pub fn spawn(
    rpc: RpcService,
    addr: SocketAddr,
    start_wait: impl core::future::Future<Output = ()> + Send + 'static,
    shutdown: impl core::future::Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    tokio::task::spawn(async move {
        start_wait.await;

        let ret = Server::builder()
            .add_service(ChiselRpcServer::new(rpc))
            .serve_with_shutdown(addr, shutdown)
            .await;
        info!("Tonic shutdown");
        ret?;
        Ok(())
    })
}
