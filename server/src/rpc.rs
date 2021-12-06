// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::RoutePaths;
use crate::deno;
use crate::policies::{LabelPolicies, Policy};
use crate::query::{MetaService, QueryEngine};
use crate::runtime;
use crate::server::CommandTrait;
use crate::server::CoordinatorChannel;
use crate::types::{Field, ObjectType, TypeSystem, TypeSystemError};
use anyhow::Result;
use async_mutex::Mutex;
use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{
    ChiselApplyRequest, ChiselApplyResponse, RestartRequest, RestartResponse, StatusRequest,
    StatusResponse, TypeExportRequest, TypeExportResponse,
};
use convert_case::{Case, Casing};
use futures::FutureExt;
use log::debug;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};
use yaml_rust::YamlLoader;

pub(crate) mod chisel {
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
pub(crate) struct GlobalRpcState {
    type_system: TypeSystem,
    meta: MetaService,
    query_engine: QueryEngine,
    routes: RoutePaths, // For globally keeping track of routes
    commands: Vec<CoordinatorChannel>,
}

impl GlobalRpcState {
    pub(crate) async fn new(
        meta: MetaService,
        query_engine: QueryEngine,
        commands: Vec<CoordinatorChannel>,
    ) -> anyhow::Result<Self> {
        let type_system = meta.load_type_system().await?;

        Ok(Self {
            type_system,
            meta,
            query_engine,
            commands,
            routes: RoutePaths::default(),
        })
    }

    async fn send_command<F>(&self, closure: Box<F>) -> Result<()>
    where
        F: Clone + CommandTrait,
    {
        for cmd in &self.commands {
            cmd.send(closure.clone()).await?;
        }
        Ok(())
    }
}

macro_rules! send_command {
    ( $code:block ) => {{
        Box::new({ move || async move { $code }.boxed_local() })
    }};
}

/// RPC service for Chisel server.
///
/// The RPC service provides a Protobuf-based interface for Chisel control
/// plane. For example, the service has RPC calls for managing types and
/// endpoints. The user-generated data plane endpoints are serviced with REST.
pub(crate) struct RpcService {
    state: Arc<Mutex<GlobalRpcState>>,
}

impl RpcService {
    pub(crate) fn new(state: Arc<Mutex<GlobalRpcState>>) -> Self {
        Self { state }
    }

    /// Apply a new version of ChiselStrike
    async fn apply_aux(
        &self,
        request: Request<ChiselApplyRequest>,
    ) -> anyhow::Result<Response<ChiselApplyResponse>> {
        let mut state = self.state.lock().await;
        let apply_request = request.into_inner();

        let mut type_names = vec![];
        let mut endpoint_routes = vec![];
        let mut labels = vec![];

        for type_def in apply_request.types {
            let name = type_def.name;
            type_names.push(name.clone());
            let snake_case_name = name.to_case(Case::Snake);

            let mut fields = Vec::new();
            for field in type_def.field_defs {
                let ty = state.type_system.lookup_type(&field.field_type)?;
                fields.push(Field {
                    name: field.name.clone(),
                    type_: ty,
                    labels: field.labels,
                    default: field.default_value,
                    is_optional: field.is_optional,
                });
            }
            let ty = Arc::new(ObjectType {
                name: name.to_owned(),
                fields,
                backing_table: snake_case_name.clone(),
            });

            match state.type_system.add_type(ty.clone()) {
                Ok(_) => {
                    // FIXME: Consistency between metadata and backing store updates.
                    let meta = &state.meta;
                    meta.insert(ty.clone()).await?;

                    let query_engine = &state.query_engine;
                    query_engine.create_table(&ty).await?;

                    let cmd = send_command!({ deno::define_type(&ty) });
                    state.send_command(cmd).await?;
                }
                Err(TypeSystemError::TypeAlreadyExists) => {
                    state.type_system.replace_type(ty)?;
                }
                Err(e) => anyhow::bail!(e),
            }
        }

        let regex = regex::Regex::new(".*").unwrap();
        state.routes.remove_routes(regex.clone());

        for endpoint in &apply_request.endpoints {
            let path = format!("/{}", endpoint.path).to_owned();

            let func = Box::new({
                let path = path.clone();
                move |req| deno::run_js(path.clone(), req).boxed_local()
            });
            endpoint_routes.push((path.clone(), func.clone(), endpoint.code.clone()));
            state.routes.add_route(&path, func);
        }

        // FIXME: only transform implemented
        let mut policies = LabelPolicies::default();

        for policy in apply_request.policies {
            let docs = YamlLoader::load_from_str(&policy.policy_config)?;
            for config in docs.iter() {
                for label in config["labels"].as_vec().get_or_insert(&[].into()).iter() {
                    let name = label["name"].as_str().ok_or_else(|| {
                        anyhow::anyhow!("couldn't parse yaml: label without a name: {:?}", label)
                    })?;

                    labels.push(name.to_owned());
                    debug!("Applying policy for label {:?}", name);

                    match label["transform"].as_str() {
                        Some("anonymize") => {
                            let pattern = label["except_uri"].as_str().unwrap_or("^$"); // ^$ never matches; each path has at least a '/' in it.
                            policies.insert(
                                name.to_owned(),
                                Policy {
                                    transform: crate::policies::anonymize,
                                    except_uri: regex::Regex::new(pattern)?,
                                },
                            );
                        }
                        Some(x) => {
                            anyhow::bail!("unknown transform: {} for label {}", x, name);
                        }
                        None => {}
                    };
                }
                for _endpoint in config["endpoints"].as_vec().iter() {
                    log::info!("endpoint behavior not yet implemented");
                }
            }
        }

        let endpoints = endpoint_routes.clone();
        let type_system = state.type_system.clone();

        let cmd = send_command!({
            let runtime = &mut runtime::get().await;
            runtime.type_system.update(&type_system);
            runtime.policies = policies;

            let mut api = runtime.api.lock().await;
            api.remove_routes(regex);
            crate::auth::init(&mut *api);

            for (path, func, code) in endpoints {
                deno::define_endpoint(path.clone(), code).await?;
                api.add_route(&path, func);
            }
            Ok(())
        });
        state.send_command(cmd).await?;

        // FIXME: return number of effective changes? Probably depends on how we implement
        // terraform-like workflow (x added, y removed, z modified)
        Ok(Response::new(ChiselApplyResponse {
            types: type_names,
            endpoints: endpoint_routes.iter().map(|x| x.0.clone()).collect(),
            labels,
        }))
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

    /// Apply a new version of ChiselStrike
    async fn apply(
        &self,
        request: Request<ChiselApplyRequest>,
    ) -> Result<Response<ChiselApplyResponse>, Status> {
        self.apply_aux(request)
            .await
            .map_err(|e| Status::internal(format!("{}", e)))
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
                    default_value: field.default.clone(),
                    is_optional: field.is_optional,
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

    async fn restart(
        &self,
        _request: tonic::Request<RestartRequest>,
    ) -> Result<tonic::Response<RestartResponse>, tonic::Status> {
        let ok = nix::sys::signal::raise(nix::sys::signal::Signal::SIGHUP).is_ok();
        Ok(Response::new(RestartResponse { ok }))
    }
}

pub(crate) fn spawn(
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
