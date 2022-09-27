// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::{MetaService, QueryEngine};
use crate::policies::PolicySystem;
use crate::proto::chisel_rpc_server::{ChiselRpc, ChiselRpcServer};
use crate::proto::{
    ApplyRequest, ApplyResponse, DeleteRequest, DeleteResponse, DescribeRequest, DescribeResponse,
    FieldDefinition, LabelPolicyDefinition, PopulateRequest, PopulateResponse, StatusRequest,
    StatusResponse, TypeDefinition, VersionDefinition,
};
use crate::server::{self, Server};
use crate::types::TypeSystem;
use crate::version::{VersionInfo, VersionInit};
use crate::{apply, version};
use anyhow::{bail, ensure, Context, Result};
use deno_core::futures;
use futures::FutureExt;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::panic;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tonic::{Request, Response, Status};
use utils::{CancellableTaskHandle, TaskHandle};
use uuid::Uuid;

/// RPC service for Chisel server.
///
/// The RPC service provides a Protobuf-based interface for Chisel control
/// plane. For example, the service has RPC calls for managing types and
/// endpoints. The user-generated data plane endpoints are serviced with HTTP.
struct RpcService {
    /// Unique UUID identifying this RPC runtime.
    id: Uuid,
    server: Arc<Server>,
}

pub async fn spawn(
    server: Arc<Server>,
    listen_addr: SocketAddr,
) -> Result<(SocketAddr, TaskHandle<Result<()>>)> {
    let rpc_service = RpcService {
        id: Uuid::new_v4(),
        server,
    };
    let router = tonic::transport::Server::builder().add_service(ChiselRpcServer::new(rpc_service));

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    let listen_addr = listener.local_addr()?;
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    let task = tokio::task::spawn(async move {
        // TODO: implement graceful shutdown?
        router
            .serve_with_incoming(incoming)
            .await
            .context("Error while serving gRPC")?;
        Ok(())
    });
    Ok((listen_addr, TaskHandle(task)))
}

#[tonic::async_trait]
impl ChiselRpc for RpcService {
    /// Get Chisel server status.
    async fn get_status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let server_id = self.id.to_string();
        let message = "OK".to_string();
        Ok(Response::new(StatusResponse { server_id, message }))
    }

    /// Apply a new version of ChiselStrike
    async fn apply(
        &self,
        request: Request<ApplyRequest>,
    ) -> Result<Response<ApplyResponse>, Status> {
        apply(self.server.clone(), request.into_inner())
            .await
            .map(Response::new)
            .map_err(|e| Status::internal(format!("{:?}", e)))
    }

    /// Delete a version of ChiselStrike
    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        delete(&self.server, request.into_inner())
            .await
            .map(Response::new)
            .map_err(|e| Status::internal(format!("{:?}", e)))
    }

    async fn populate(
        &self,
        request: Request<PopulateRequest>,
    ) -> Result<Response<PopulateResponse>, Status> {
        populate(&self.server, request.into_inner())
            .await
            .map(Response::new)
            .map_err(|e| Status::internal(format!("{:?}", e)))
    }

    async fn describe(
        &self,
        _request: Request<DescribeRequest>,
    ) -> Result<Response<DescribeResponse>, Status> {
        Ok(Response::new(describe(&self.server)))
    }
}

fn describe(server: &Server) -> DescribeResponse {
    let versions = server.trunk.list_versions();

    let version_defs = versions
        .into_iter()
        .map(|version| {
            let mut type_defs = version
                .type_system
                .custom_types
                .values()
                .map(|entity| {
                    let field_defs = entity
                        .user_fields()
                        .map(|field| {
                            let field_type = version.type_system.get(&field.type_id).unwrap();
                            FieldDefinition {
                                name: field.name.to_owned(),
                                field_type: Some(field_type.into()),
                                labels: field.labels.clone(),
                                default_value: field.user_provided_default().clone(),
                                is_optional: field.is_optional,
                                is_unique: field.is_unique,
                            }
                        })
                        .collect();

                    TypeDefinition {
                        name: entity.name().to_string(),
                        field_defs,
                    }
                })
                .collect::<Vec<_>>();
            type_defs.sort_unstable_by(|x, y| x.name.cmp(&y.name));

            let mut label_policy_defs = version
                .policy_system
                .labels
                .keys()
                .map(|label| LabelPolicyDefinition {
                    label: label.clone(),
                })
                .collect::<Vec<_>>();
            label_policy_defs.sort_unstable_by(|x, y| x.label.cmp(&y.label));

            VersionDefinition {
                version_id: version.version_id.clone(),
                type_defs,
                label_policy_defs,
            }
        })
        .collect();

    DescribeResponse { version_defs }
}

async fn apply(server: Arc<Server>, request: ApplyRequest) -> Result<ApplyResponse> {
    let version_id = validate_version_id(&request.version_id)?;
    let info = VersionInfo {
        name: request.app_name.clone(),
        tag: request.version_tag.clone(),
    };

    let modules = request
        .modules
        .iter()
        .map(|m| (m.url.clone(), m.code.clone()))
        .collect::<HashMap<_, _>>();
    let modules = Arc::new(modules);
    validate_modules(
        server.clone(),
        version_id.clone(),
        info.clone(),
        modules.clone(),
    )
    .await
    .context("The provided code does not seem to work")?;

    let result = {
        let mut type_systems = server.type_systems.lock().await;
        let type_system = type_systems
            .entry(version_id.clone())
            .or_insert_with(|| TypeSystem::new(server.builtin_types.clone(), version_id.clone()));

        // NOTE: there is a race condition, because we migrate the database to the new schema, while
        // there might be workers that still assume the old schema
        apply::apply(
            server.clone(),
            &request,
            type_system,
            version_id.clone(),
            &info,
            &modules,
        )
        .await?
    };

    let (ready_tx, ready_rx) = oneshot::channel();
    let init = VersionInit {
        version_id,
        info,
        server: server.clone(),
        modules,
        type_system: Arc::new(result.type_system),
        policy_system: Arc::new(result.policy_system),
        worker_count: server.opt.worker_threads,
        ready_tx,
        is_canary: false,
    };

    let (version, job_tx, mut version_task) = version::spawn(init).await?;
    wait_until_ready(&mut version_task, ready_rx)
        .await
        .context(
            "The version did not start up correcly, but the database has already been modified",
        )?;
    server.trunk.add_version(version, job_tx, version_task);

    // try to update the secrets, so that if the user edited `.env`, the updated version will see
    // the new secrets immediately (this is in particular importance for tests).
    //
    // if an error happens, let's just ignore it: the `refresh_secrets()` task, which is
    // responsible for periodic updating of the secrets, will show the error
    let _: Result<()> = server::update_secrets(&server).await;

    Ok(ApplyResponse {
        types: result.type_names_user_order,
        labels: result.labels,
        event_handlers: Vec::new(),
    })
}

async fn validate_modules(
    server: Arc<Server>,
    version_id: String,
    info: VersionInfo,
    modules: Arc<HashMap<String, String>>,
) -> Result<()> {
    let type_system = TypeSystem::new(server.builtin_types.clone(), version_id.clone());
    let policy_system = PolicySystem::default();

    let (ready_tx, ready_rx) = oneshot::channel();
    let init = VersionInit {
        version_id,
        info,
        server: server.clone(),
        modules,
        type_system: Arc::new(type_system),
        policy_system: Arc::new(policy_system),
        worker_count: 1,
        ready_tx,
        is_canary: true,
    };

    let (_version, _job_tx, mut version_task) = version::spawn(init).await?;
    wait_until_ready(&mut version_task, ready_rx).await?;
    Ok(())
}

async fn wait_until_ready(
    mut version_task: &mut CancellableTaskHandle<Result<()>>,
    ready_rx: oneshot::Receiver<()>,
) -> Result<()> {
    use futures::future::Fuse;
    let mut ready_rx = ready_rx.fuse();
    let mut timeout = Fuse::terminated();
    loop {
        tokio::select! {
            res = &mut version_task => match res {
                Some(Ok(_)) => bail!("Version task terminated before the version was ready"),
                Some(Err(err)) => return Err(err.context("Could not apply the provided code")),
                None => bail!("Version task was cancelled"),
            },
            res = &mut ready_rx => match res {
                Ok(_) => return Ok(()),
                Err(_) => {
                    // the version has dropped its `ready_tx`, so it will never become ready. we
                    // give it some time to return an error before timing out
                    timeout = Box::pin(tokio::time::sleep(Duration::from_millis(100))).fuse();
                },
            },
            () = &mut timeout =>
                bail!("Version did not become ready"),
        }
    }
}

async fn delete(server: &Server, request: DeleteRequest) -> Result<DeleteResponse> {
    let version = match server.trunk.remove_version(&request.version_id) {
        Some(version) => version,
        None => bail!("Version {:?} does not exist", request.version_id),
    };

    // TODO: we should perhaps wait until the version is drained of pending requests before we
    // start modifying the database?

    let entities_to_remove: Vec<_> = version.type_system.custom_types.values().collect();

    let meta = &server.meta_service;
    let mut transaction = meta.begin_transaction().await?;
    meta.delete_policy_version(&mut transaction, &version.version_id)
        .await?;
    for &entity in entities_to_remove.iter() {
        meta.remove_type(&mut transaction, entity).await?;
    }
    MetaService::commit_transaction(transaction).await?;

    let query_engine = &server.query_engine;
    let mut transaction = query_engine.begin_transaction().await?;
    for &entity in entities_to_remove.iter() {
        query_engine.drop_table(&mut transaction, entity).await?;
    }
    QueryEngine::commit_transaction(transaction).await?;

    let message = format!("Deleted {:?}", version.version_id);
    Ok(DeleteResponse { message })
}

async fn populate(server: &Server, request: PopulateRequest) -> Result<PopulateResponse> {
    let to_version = server
        .trunk
        .get_version(&request.to_version_id)
        .context(format!(
            "To-version {:?} does not exist",
            request.to_version_id
        ))?;
    let from_version = server
        .trunk
        .get_version(&request.from_version_id)
        .context(format!(
            "From-version {:?} does not exist",
            request.from_version_id
        ))?;

    TypeSystem::populate_types(
        &server.query_engine,
        &to_version.type_system,
        &from_version.type_system,
    )
    .await?;

    let message = "OK".to_string();
    Ok(PopulateResponse { message })
}

fn validate_version_id(version_id: &str) -> Result<String> {
    ensure!(
        version_id != "__chiselstrike",
        "Version {:?} is special and cannot be used",
        version_id,
    );

    let regex = regex::Regex::new(r"^[-_[[:alnum:]]]+$").unwrap();
    ensure!(
        regex.is_match(version_id),
        "Version can only be alphanumeric, _ or -"
    );
    Ok(version_id.into())
}
