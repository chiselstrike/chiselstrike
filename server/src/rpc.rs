// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::{ApiInfo, RequestPath};
use crate::apply::{self, ApplyResult};
use crate::datastore::{MetaService, QueryEngine};
use crate::deno::endpoint_path_from_source_path;
use crate::deno::mutate_policies;
use crate::deno::remove_type_version;
use crate::deno::set_type_system;
use crate::deno::{self, set_version_type_policies};
use crate::internal::mark_ready;
use crate::policies::Policies;
use crate::prefix_map::PrefixMap;
use crate::proto::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use crate::proto::{
    self, ChiselApplyRequest, ChiselApplyResponse, ChiselDeleteRequest, ChiselDeleteResponse,
    DescribeRequest, DescribeResponse, PopulateRequest, PopulateResponse, RestartRequest,
    RestartResponse, StatusRequest, StatusResponse,
};
use crate::runtime;
use crate::server::CommandTrait;
use crate::server::CoordinatorChannel;
use crate::types::{Entity, TypeSystem};
use anyhow::{Context, Result};
use async_lock::Mutex;
use deno_core::futures;
use deno_core::url::Url;
use futures::FutureExt;
use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};
use utils::without_extension;
use uuid::Uuid;

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
pub struct GlobalRpcState {
    /// Unique UUID identifying this RPC runtime.
    id: Uuid,
    type_system: TypeSystem,
    meta: MetaService,
    query_engine: Arc<QueryEngine>,
    sources: PrefixMap<String>, // For globally keeping track of routes
    commands: Vec<CoordinatorChannel>,
    policies: Policies,
    versions: BTreeSet<String>,
}

#[derive(Clone)]
pub struct InitState {
    pub sources: PrefixMap<String>,
    pub policies: Policies,
    pub type_system: TypeSystem,
}

impl GlobalRpcState {
    pub async fn new(
        meta: MetaService,
        init: InitState,
        query_engine: QueryEngine,
        commands: Vec<CoordinatorChannel>,
    ) -> Result<Self> {
        let InitState {
            sources,
            policies,
            type_system,
        } = init;

        let mut versions = BTreeSet::new();
        for v in type_system.versions.keys() {
            versions.insert(v.to_owned());
        }
        for (p, _) in sources.iter() {
            let rp = RequestPath::try_from(p).unwrap();
            versions.insert(rp.api_version().to_owned());
        }

        Ok(Self {
            id: Uuid::new_v4(),
            type_system,
            meta,
            query_engine: Arc::new(query_engine),
            commands,
            sources,
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
pub struct RpcService {
    state: Arc<Mutex<GlobalRpcState>>,
}

impl RpcService {
    pub fn new(state: Arc<Mutex<GlobalRpcState>>) -> Self {
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
        let to_remove: Vec<&Entity> = version_types.custom_types.iter().map(|x| x.1).collect();

        let meta = &state.meta;
        let mut transaction = meta.begin_transaction().await?;

        meta.delete_policy_version(&mut transaction, &api_version)
            .await?;

        for ty in to_remove.iter() {
            meta.remove_type(&mut transaction, ty).await?;
        }

        MetaService::commit_transaction(transaction).await?;

        let query_engine = &state.query_engine;
        let mut transaction = query_engine.begin_transaction().await?;
        for ty in to_remove.into_iter() {
            query_engine.drop_table(&mut transaction, ty).await?;
        }
        QueryEngine::commit_transaction(transaction).await?;

        let prefix = format!("/{}/", api_version);
        state.sources.remove_prefix(&prefix);
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

        let response = proto::PopulateResponse {
            msg: "OK".to_string(),
        };

        Ok(Response::new(response))
    }
    /// Apply a new version of ChiselStrike
    async fn apply_aux(
        &self,
        request: Request<ChiselApplyRequest>,
    ) -> Result<Response<ChiselApplyResponse>> {
        let mut apply_request = request.into_inner();
        let api_version = apply_request.version.clone();
        validate_api_version(&api_version)?;

        let api_version_tag = apply_request.version_tag.clone();
        let app_name = apply_request.app_name.clone();

        let mut state = self.state.lock().await;
        let api_info = ApiInfo::new(app_name, api_version_tag);

        let mut endpoint_paths = vec![];
        let mut event_handler_paths = vec![];
        let mut sources = HashMap::new();
        for (path, code) in apply_request.sources.drain() {
            if Url::parse(&path).is_ok() {
                sources.insert(path, code.clone());
                continue;
            }

            sources.insert(format!("/{}/{}", api_version, path), code.clone());
            let path = without_extension(&path);
            if let Some(path) = path
                .strip_prefix("routes/")
                .or_else(|| path.strip_prefix("endpoints/"))
            {
                let path = format!("/{}/{}", api_version, path);
                endpoint_paths.push(path);
            }
            if let Some(path) = path.strip_prefix("events/") {
                let path = format!("/{}/{}", api_version, path);
                event_handler_paths.push(path);
            }
        }
        endpoint_paths.sort_unstable();
        event_handler_paths.sort_unstable();

        // Do this before any permanent changes to any of the databases. Otherwise
        // we end up with bad code commited to the meta database and will fail to load
        // chiseld next time, as it tries to replenish the endpoints
        let endpoints = sources.clone();
        let cmd = send_command!({
            deno::compile_endpoints(endpoints).await?;
            Ok(())
        });
        state
            .send_command(cmd)
            .await
            .context("Could not apply the provided code")?;

        anyhow::ensure!(
            "__chiselstrike" != &api_version,
            "__chiselstrike is a reserved version name"
        );

        // so that an empty apply removes the version.
        // We'll add it back as soon as we notice this is not empty
        state.versions.remove(&api_version);

        let ApplyResult {
            type_names_user_order,
            labels,
            version_policy,
            type_policies,
        } = {
            // help the borrow checker figure out that the borrows below are safe
            let state: &mut GlobalRpcState = &mut state;
            apply::apply(
                &state.query_engine,
                &state.meta,
                &mut state.type_system,
                &mut state.policies,
                &apply_request,
                api_version.clone(),
                &api_info,
            )
            .await?
        };

        let prefix = format!("/{}/", api_version);
        state.sources.remove_prefix(&prefix);

        for (path, code) in &sources {
            state.sources.insert(path.into(), code.clone());
        }

        state.meta.persist_sources(&state.sources).await?;

        let types_global = state.type_system.clone();

        if !endpoint_paths.is_empty() || types_global.get_version(&api_version).is_ok() {
            state.versions.insert(api_version.clone());
        }

        let endpoints_for_cmd = endpoint_paths.clone();
        let event_handlers_for_cmd = event_handler_paths.clone();
        let cmd = send_command!({
            {
                set_type_system(types_global.clone()).await;
                let pol_version = api_version.clone();
                mutate_policies(move |policies| {
                    policies.versions.insert(pol_version, version_policy);
                })
                .await;

                set_version_type_policies(api_version.clone(), type_policies).await;

                let runtime = runtime::get();
                runtime.api.remove_routes(&prefix);

                for path in &endpoints_for_cmd {
                    let func = Arc::new({
                        let path = path.clone();
                        move |req| deno::run_js(path.clone(), req).boxed_local()
                    });
                    runtime.api.add_route(path.into(), func);
                }
                for path in &event_handlers_for_cmd {
                    let func = Arc::new({
                        let path = path.clone();
                        move |key: Option<Vec<u8>>, value: Option<Vec<u8>>| {
                            deno::run_js_event(path.clone(), key, value).boxed_local()
                        }
                    });
                    runtime.api.add_event_handler(path.into(), func);
                }
                runtime.api.update_api_info(&api_version, api_info);
            }
            for path in endpoints_for_cmd {
                deno::activate_endpoint(&path).await?;
            }
            for path in event_handlers_for_cmd {
                deno::activate_event_handler(&path).await?;
            }
            Ok(())
        });
        // FIXME: activate_event_handlers()
        state.send_command(cmd).await?;

        // FIXME: return number of effective changes? Probably depends on how we implement
        // terraform-like workflow (x added, y removed, z modified)
        Ok(Response::new(ChiselApplyResponse {
            types: type_names_user_order,
            endpoints: endpoint_paths,
            labels,
            event_handlers: event_handler_paths,
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
        let server_id = {
            let state = self.state.lock().await;
            state.id.to_string()
        };
        let response = proto::StatusResponse {
            server_id,
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
                        let field_type = state.type_system.get(&field.type_id).unwrap();
                        field_defs.push(proto::FieldDefinition {
                            name: field.name.to_owned(),
                            field_type: Some(field_type.into()),
                            labels: field.labels.clone(),
                            default_value: field.user_provided_default().clone(),
                            is_optional: field.is_optional,
                            is_unique: field.is_unique,
                        });
                    }
                    let type_def = proto::TypeDefinition {
                        name: ty.name().to_string(),
                        field_defs,
                    };
                    type_defs.push(type_def);
                }
            }
            let mut endpoint_defs = vec![];
            let version_path_str = format!("/{}/", api_version);
            for (path, _) in state.sources.iter() {
                let dir_name = path.split('/').nth(2);
                if dir_name != Some("routes") || dir_name != Some("endpoints") {
                    continue;
                }
                let path = endpoint_path_from_source_path(path);
                if path.starts_with(&version_path_str) {
                    endpoint_defs.push(proto::EndpointDefinition {
                        path: path.to_string(),
                    });
                }
            }
            let mut label_policy_defs = vec![];
            if let Some(policies) = state.policies.versions.get(api_version) {
                for label in policies.labels.keys() {
                    label_policy_defs.push(proto::LabelPolicyDefinition {
                        label: label.clone(),
                    });
                }
            }
            version_defs.push(proto::VersionDefinition {
                version: api_version.to_string(),
                type_defs,
                endpoint_defs,
                label_policy_defs,
            });
        }

        let response = proto::DescribeResponse { version_defs };
        Ok(Response::new(response))
    }

    async fn restart(
        &self,
        _request: tonic::Request<RestartRequest>,
    ) -> Result<tonic::Response<RestartResponse>, tonic::Status> {
        let server_id = {
            let state = self.state.lock().await;
            state.id.to_string()
        };
        let ok = nix::sys::signal::raise(nix::sys::signal::Signal::SIGUSR1).is_ok();
        Ok(Response::new(RestartResponse { server_id, ok }))
    }
}

pub fn spawn(
    rpc: RpcService,
    addr: SocketAddr,
    start_wait: impl core::future::Future<Output = ()> + Send + 'static,
    shutdown: impl core::future::Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<Result<()>> {
    tokio::task::spawn(async move {
        start_wait.await;
        mark_ready();

        let ret = Server::builder()
            .add_service(ChiselRpcServer::new(rpc))
            .serve_with_shutdown(addr, shutdown)
            .await;
        debug!("Tonic shutdown");
        ret?;
        Ok(())
    })
}
