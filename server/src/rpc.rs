// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::RoutePaths;
use crate::deno;
use crate::policies::{Policies, Policy};
use crate::query::{MetaService, QueryEngine};
use crate::runtime;
use crate::server::CommandTrait;
use crate::server::CoordinatorChannel;
use crate::types::{Field, NewObject, ObjectType, TypeSystem, TypeSystemError};
use anyhow::Result;
use async_mutex::Mutex;
use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{
    ChiselApplyRequest, ChiselApplyResponse, RestartRequest, RestartResponse, StatusRequest,
    StatusResponse, TypeExportRequest, TypeExportResponse,
};
use futures::FutureExt;
use log::debug;
use std::collections::BTreeSet;
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
        let routes = meta.load_endpoints().await?;

        Ok(Self {
            type_system,
            meta,
            query_engine,
            commands,
            routes,
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

        let mut type_names = BTreeSet::new();
        let mut type_names_user_order = vec![];
        let mut endpoint_routes = vec![];
        let mut labels = vec![];

        for tdef in apply_request.types.iter() {
            type_names.insert(tdef.name.clone());
            type_names_user_order.push(tdef.name.clone());
        }

        let mut to_remove = vec![];
        let mut to_insert = vec![];
        let mut to_update = vec![];

        for (existing, removed) in state.type_system.types.iter() {
            if type_names.get(existing).is_none() {
                to_remove.push(removed.clone());
            }
        }

        if !to_remove.is_empty() && !apply_request.allow_type_deletion {
            anyhow::bail!("Trying to remove types from type file. This will delete the underlying data associated with this type. To proceed, apply again with --allow-type-deletion");
        }

        // No changes are made to the type system in this loop. We re-read the database after we
        // apply the changes, and this way we don't have to deal with the case of succeding to
        // apply a type, but failing the next
        for type_def in apply_request.types {
            let name = type_def.name;

            let mut fields = Vec::new();
            for field in type_def.field_defs {
                let ty = state.type_system.lookup_type(&field.field_type)?;
                fields.push(Field {
                    id: None,
                    name: field.name.clone(),
                    type_: ty,
                    labels: field.labels,
                    default: field.default_value,
                    is_optional: field.is_optional,
                });
            }
            let ty = Arc::new(ObjectType::new(NewObject::new(&name), fields));

            match state.type_system.lookup_object_type(&name) {
                Ok(old_type) => {
                    let delta = state.type_system.replace_type(&old_type, ty)?;
                    to_update.push((old_type.clone(), delta));
                }
                Err(TypeSystemError::NoSuchType(_)) => {
                    to_insert.push(ty.clone());
                }
                Err(e) => anyhow::bail!(e),
            }
        }

        let meta = &state.meta;
        let mut transaction = meta.start_transaction().await?;

        for ty in to_insert.iter() {
            // FIXME: Consistency between metadata and backing store updates.
            meta.insert_type(&mut transaction, ty).await?;
        }

        for (old, delta) in to_update.into_iter() {
            meta.update_type(&mut transaction, &old, delta).await?;
        }

        for ty in to_remove.iter() {
            meta.remove_type(&mut transaction, ty).await?;
        }
        MetaService::commit_transaction(transaction).await?;

        // Reload the type system so that we have new ids
        state.type_system = meta.load_type_system().await?;

        // Update the data database after all the metdata is up2date.
        // We will not get a single transaction because in the general case those things
        // could be in totally different databases. However, some foreign relations would force
        // us to update some subset of them together. FIXME: revisit this when we support relations
        let query_engine = &state.query_engine;
        for ty in to_insert.into_iter() {
            query_engine.create_table(&ty).await?;
        }

        for ty in to_remove.into_iter() {
            query_engine.drop_table(&ty).await?;
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
            state.routes.add_route(&path, &endpoint.code, func);
        }

        state.meta.persist_endpoints(&state.routes).await?;

        let mut policies = Policies::new();

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
                            policies.labels.insert(
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
                for endpoint in config["endpoints"]
                    .as_vec()
                    .get_or_insert(&[].into())
                    .iter()
                {
                    if let Some(true) = endpoint["must_login"].as_bool() {
                        if let Some(name) = endpoint["name"].as_str() {
                            policies.authorize.insert(name.into());
                        }
                    }
                }
            }
        }

        let endpoints = endpoint_routes.clone();
        let type_system = state.type_system.clone();

        let cmd = send_command!({
            let runtime = &mut runtime::get().await;
            runtime.type_system.update(&type_system);

            deno::flush_types()?;
            for (_, ty) in runtime.type_system.types.iter() {
                deno::define_type(ty)?;
            }
            runtime.policies = policies;

            let mut api = runtime.api.lock().await;
            api.remove_routes(regex);
            crate::auth::init(&mut *api);

            for (path, func, code) in endpoints {
                deno::define_endpoint(path.clone(), code.clone()).await?;
                api.add_route(&path, code, func);
            }
            Ok(())
        });
        state.send_command(cmd).await?;

        // FIXME: return number of effective changes? Probably depends on how we implement
        // terraform-like workflow (x added, y removed, z modified)
        Ok(Response::new(ChiselApplyResponse {
            types: type_names_user_order,
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
            .sorted_by(|x, y| x.name().cmp(y.name()))
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
                name: ty.name().to_string(),
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
