// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiInfo, RequestPath};
use crate::chisel;
use crate::datastore::{MetaService, QueryEngine};
use crate::deno;
use crate::deno::mutate_policies;
use crate::deno::remove_type_version;
use crate::deno::set_type_system;
use crate::policies::{Policies, VersionPolicy};
use crate::prefix_map::PrefixMap;
use crate::runtime;
use crate::server::CommandTrait;
use crate::server::CoordinatorChannel;
use crate::types::AuthOrNot::IsNotAuth;
use crate::types::{Field, NewField, NewObject, ObjectType, Type, TypeSystem, TypeSystemError};
use anyhow::{Context, Result};
use async_lock::Mutex;
use chisel::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use chisel::{
    ChiselApplyRequest, ChiselApplyResponse, ChiselDeleteRequest, ChiselDeleteResponse,
    DescribeRequest, DescribeResponse, PopulateRequest, PopulateResponse, RestartRequest,
    RestartResponse, StatusRequest, StatusResponse,
};
use futures::FutureExt;
use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};

fn validate_api_version(version: &str) -> Result<()> {
    anyhow::ensure!(
        version.is_ascii(),
        "api version cannot have non-ascii characters"
    );
    let v = regex::Regex::new(r"^[-_[[:alnum:]]]+$").unwrap();
    anyhow::ensure!(
        v.is_match(version),
        "api version can only be alphanumeric, _ or -"
    );
    Ok(())
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
    query_engine: Arc<QueryEngine>,
    routes: PrefixMap<String>, // For globally keeping track of routes
    commands: Vec<CoordinatorChannel>,
    policies: Policies,
    versions: BTreeSet<String>,
}

impl GlobalRpcState {
    pub(crate) async fn new(
        meta: MetaService,
        query_engine: QueryEngine,
        commands: Vec<CoordinatorChannel>,
    ) -> Result<Self> {
        let type_system = meta.load_type_system().await?;
        let routes = meta.load_endpoints().await?;
        let policies = meta.load_policies().await?;

        let mut versions = BTreeSet::new();
        for v in type_system.versions.keys() {
            versions.insert(v.to_owned());
        }
        for (p, _) in routes.iter() {
            let rp = RequestPath::try_from(p.to_str().unwrap()).unwrap();
            versions.insert(rp.api_version().to_owned());
        }

        Ok(Self {
            type_system,
            meta,
            query_engine: Arc::new(query_engine),
            commands,
            routes,
            policies,
            versions,
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

    /// Delete a new version of ChiselStrike
    async fn delete_aux(
        &self,
        request: Request<ChiselDeleteRequest>,
    ) -> Result<Response<ChiselDeleteResponse>> {
        let mut state = self.state.lock().await;
        let apply_request = request.into_inner();
        let api_version = apply_request.version;

        anyhow::ensure!(
            "__chiselstrike" != &api_version,
            "__chiselstrike is a reserved version name"
        );
        state.versions.remove(&api_version);

        let version_types = state.type_system.get_version(&api_version)?;
        let to_remove: Vec<&Arc<ObjectType>> =
            version_types.custom_types.iter().map(|x| x.1).collect();

        let meta = &state.meta;
        let mut transaction = meta.start_transaction().await?;

        meta.delete_policy_version(&mut transaction, &api_version)
            .await?;

        for ty in to_remove.iter() {
            meta.remove_type(&mut transaction, ty).await?;
        }

        MetaService::commit_transaction(transaction).await?;

        let query_engine = &state.query_engine;
        let mut transaction = query_engine.start_transaction().await?;
        for ty in to_remove.into_iter() {
            query_engine.drop_table(&mut transaction, ty).await?;
        }
        QueryEngine::commit_transaction(transaction).await?;

        let prefix: PathBuf = format!("/{}/", api_version).into();
        state.routes.remove_prefix(&prefix);
        state.type_system.versions.remove(&api_version);
        state.policies.versions.remove(&api_version);

        let version = api_version.clone();

        let cmd = send_command!({
            remove_type_version(&version).await;

            mutate_policies(move |policies| {
                policies.versions.remove(&version);
            })
            .await;

            let runtime = runtime::get();
            runtime.api.remove_routes(&prefix);
            Ok(())
        });
        state.send_command(cmd).await?;

        Ok(Response::new(ChiselDeleteResponse {
            result: format!("deleted {}", api_version),
        }))
    }

    async fn populate_aux(
        &self,
        request: Request<PopulateRequest>,
    ) -> Result<Response<PopulateResponse>> {
        let request = request.into_inner();

        let to = request.to_version.clone();
        let from = request.from_version.clone();

        let state = self.state.lock().await;

        state
            .type_system
            .populate_types(state.query_engine.clone(), &to, &from)
            .await?;

        let response = chisel::PopulateResponse {
            msg: "OK".to_string(),
        };

        Ok(Response::new(response))
    }
    /// Apply a new version of ChiselStrike
    async fn apply_aux(
        &self,
        request: Request<ChiselApplyRequest>,
    ) -> Result<Response<ChiselApplyResponse>> {
        let apply_request = request.into_inner();
        let api_version = apply_request.version;
        validate_api_version(&api_version)?;

        let api_version_tag = apply_request.version_tag;
        let app_name = apply_request.app_name;

        let mut state = self.state.lock().await;
        let api_info = ApiInfo::new(app_name, api_version_tag);

        let mut endpoint_routes = vec![];
        for endpoint in apply_request.endpoints {
            let path = format!("/{}/{}", api_version, endpoint.path);
            endpoint_routes.push((path, endpoint.code));
        }

        // Do this before any permanent changes to any of the databases. Otherwise
        // we end up with bad code commited to the meta database and will fail to load
        // chiseld next time, as it tries to replenish the endpoints
        for (path, code) in &endpoint_routes {
            let cmd_path = path.clone();
            let code = code.clone();
            let cmd = send_command!({
                deno::compile_endpoint(cmd_path, code).await?;
                Ok(())
            });
            state
                .send_command(cmd)
                .await
                .with_context(|| format!("parsing endpoint {}", path))?;
        }

        anyhow::ensure!(
            "__chiselstrike" != &api_version,
            "__chiselstrike is a reserved version name"
        );

        // so that an empty apply removes the version.
        // We'll add it back as soon as we notice this is not empty
        state.versions.remove(&api_version);

        let mut type_names = BTreeSet::new();
        let mut type_names_user_order = vec![];

        for tdef in apply_request.types.iter() {
            type_names.insert(tdef.name.clone());
            type_names_user_order.push(tdef.name.clone());
        }

        let mut to_remove = vec![];
        let mut to_insert = vec![];
        let mut to_update = vec![];

        state.type_system.get_version_mut(&api_version);
        let version_types = state.type_system.get_version(&api_version)?; // End mutable state borrow from above.

        for (existing, removed) in version_types.custom_types.iter() {
            if type_names.get(existing).is_none() {
                to_remove.push(removed.clone());
            }
        }

        anyhow::ensure!(
            apply_request.policies.len() <= 1,
            "Currently only one policy file supported"
        );

        let policy_str = apply_request
            .policies
            .get(0)
            .map(|x| x.policy_config.as_ref())
            .unwrap_or("");

        let policy = VersionPolicy::from_yaml(policy_str)?;

        if !to_remove.is_empty() && !apply_request.allow_type_deletion {
            anyhow::bail!("Trying to remove types from type file. This will delete the underlying data associated with this type. To proceed, apply again with --allow-type-deletion");
        }

        let mut decorators = BTreeSet::default();
        let mut new_types = HashMap::<String, Arc<ObjectType>>::default();

        // No changes are made to the type system in this loop. We re-read the database after we
        // apply the changes, and this way we don't have to deal with the case of succeding to
        // apply a type, but failing the next
        for type_def in apply_request.types {
            let name = type_def.name;
            if state.type_system.lookup_builtin_type(&name).is_ok() {
                anyhow::bail!("custom type expected, got `{}` instead", name);
            }

            let mut fields = Vec::new();
            for field in type_def.field_defs {
                for label in &field.labels {
                    decorators.insert(label.clone());
                }

                let field_ty = match state.type_system.lookup_builtin_type(&field.field_type) {
                    Ok(ty) => ty,
                    Err(_) => match new_types.get(&field.field_type) {
                        Some(ty) => Type::Object(ty.clone()),
                        None => anyhow::bail!(
                            "field type `{}` is neither a built-in nor a custom type",
                            &field.field_type
                        ),
                    },
                };

                fields.push(Field::new(
                    NewField::new(&field.name, field_ty, &api_version)?,
                    field.labels,
                    field.default_value,
                    field.is_optional,
                    field.is_unique,
                ));
            }

            let ty = Arc::new(ObjectType::new(
                NewObject::new(&name, &api_version),
                fields,
                IsNotAuth,
            )?);
            new_types.insert(name.to_owned(), ty.clone());

            match version_types.lookup_custom_type(&name) {
                Ok(old_type) => {
                    let delta = TypeSystem::generate_type_delta(&old_type, ty)?;
                    to_update.push((old_type.clone(), delta));
                }
                Err(TypeSystemError::NoSuchType(_) | TypeSystemError::NoSuchVersion(_)) => {
                    to_insert.push(ty.clone());
                }
                Err(e) => anyhow::bail!(e),
            }
        }

        let meta = &state.meta;
        let mut transaction = meta.start_transaction().await?;

        meta.persist_policy_version(&mut transaction, &api_version, policy_str)
            .await?;

        meta.persist_api_info(&mut transaction, &api_version, &api_info)
            .await?;

        for ty in to_insert.iter() {
            // FIXME: Consistency between metadata and backing store updates.
            meta.insert_type(&mut transaction, ty).await?;
        }

        for (old, delta) in to_update.iter() {
            meta.update_type(&mut transaction, old, delta.clone())
                .await?;
        }

        for ty in to_remove.iter() {
            meta.remove_type(&mut transaction, ty).await?;
        }

        MetaService::commit_transaction(transaction).await?;

        let labels: Vec<String> = policy.labels.keys().map(|x| x.to_owned()).collect();
        let type_system = meta.load_type_system().await?;

        // Reload the type system so that we have new ids
        state.type_system = type_system;
        state
            .policies
            .versions
            .insert(api_version.to_owned(), policy.clone());

        // Update the data database after all the metdata is up2date.
        // We will not get a single transaction because in the general case those things
        // could be in totally different databases. However, some foreign relations would force
        // us to update some subset of them together. FIXME: revisit this when we support relations
        let query_engine = &state.query_engine;
        let mut transaction = query_engine.start_transaction().await?;
        for ty in to_insert.into_iter() {
            query_engine.create_table(&mut transaction, &ty).await?;
        }

        for ty in to_remove.into_iter() {
            query_engine.drop_table(&mut transaction, &ty).await?;
        }

        for (old, delta) in to_update.into_iter() {
            query_engine
                .alter_table(&mut transaction, &old, delta)
                .await?;
        }
        QueryEngine::commit_transaction(transaction).await?;

        let prefix: PathBuf = format!("/{}/", api_version).into();
        state.routes.remove_prefix(&prefix);

        for (path, code) in &endpoint_routes {
            state.routes.insert(path.into(), code.clone());
        }

        state.meta.persist_endpoints(&state.routes).await?;

        let endpoints = endpoint_routes.clone();
        let types_global = state.type_system.clone();

        if !endpoints.is_empty() || types_global.get_version(&api_version).is_ok() {
            state.versions.insert(api_version.clone());
        }

        let cmd = send_command!({
            {
                set_type_system(types_global.clone()).await;
                let pol_version = api_version.clone();
                mutate_policies(move |policies| {
                    policies.versions.insert(pol_version, policy);
                })
                .await;

                let runtime = runtime::get();
                runtime.api.remove_routes(&prefix);

                for (path, _) in &endpoints {
                    let func = Arc::new({
                        let path = path.clone();
                        move |req| deno::run_js(path.clone(), req).boxed_local()
                    });
                    runtime.api.add_route(path.into(), func);
                }
                runtime.api.update_api_info(&api_version, api_info);
            }
            for (path, _) in endpoints {
                deno::activate_endpoint(&path).await?;
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
            .map_err(|e| Status::internal(format!("{:?}", e)))
    }

    /// Delete a version of ChiselStrike
    async fn delete(
        &self,
        request: Request<ChiselDeleteRequest>,
    ) -> Result<Response<ChiselDeleteResponse>, Status> {
        self.delete_aux(request)
            .await
            .map_err(|e| Status::internal(format!("{:?}", e)))
    }

    async fn populate(
        &self,
        request: Request<PopulateRequest>,
    ) -> Result<Response<PopulateResponse>, Status> {
        self.populate_aux(request)
            .await
            .map_err(|e| Status::internal(format!("{:?}", e)))
    }

    async fn describe(
        &self,
        _request: tonic::Request<DescribeRequest>,
    ) -> Result<tonic::Response<DescribeResponse>, tonic::Status> {
        let state = self.state.lock().await;

        let mut version_defs = vec![];
        for api_version in state.versions.iter() {
            let mut type_defs = vec![];
            if let Some(version_types) = state.type_system.versions.get(api_version) {
                use itertools::Itertools;
                for ty in version_types
                    .custom_types
                    .values()
                    .sorted_by(|x, y| x.name().cmp(y.name()))
                {
                    let mut field_defs = vec![];
                    for field in ty.user_fields() {
                        field_defs.push(chisel::FieldDefinition {
                            name: field.name.to_owned(),
                            field_type: field.type_.name().to_string(),
                            labels: field.labels.clone(),
                            default_value: field.user_provided_default().clone(),
                            is_optional: field.is_optional,
                            is_unique: field.is_unique,
                        });
                    }
                    let type_def = chisel::TypeDefinition {
                        name: ty.name().to_string(),
                        field_defs,
                    };
                    type_defs.push(type_def);
                }
            }
            let mut endpoint_defs = vec![];
            let version_path_str = format!("/{}/", api_version);
            for (path, _) in state.routes.iter() {
                if path.starts_with(&version_path_str) {
                    endpoint_defs.push(chisel::EndpointDefinition {
                        path: path.display().to_string(),
                    });
                }
            }
            let mut label_policy_defs = vec![];
            if let Some(policies) = state.policies.versions.get(api_version) {
                for label in policies.labels.keys() {
                    label_policy_defs.push(chisel::LabelPolicyDefinition {
                        label: label.clone(),
                    });
                }
            }
            version_defs.push(chisel::VersionDefinition {
                version: api_version.to_string(),
                type_defs,
                endpoint_defs,
                label_policy_defs,
            });
        }

        let response = chisel::DescribeResponse { version_defs };
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
) -> tokio::task::JoinHandle<Result<()>> {
    tokio::task::spawn(async move {
        start_wait.await;

        let ret = Server::builder()
            .add_service(ChiselRpcServer::new(rpc))
            .serve_with_shutdown(addr, shutdown)
            .await;
        debug!("Tonic shutdown");
        ret?;
        Ok(())
    })
}
