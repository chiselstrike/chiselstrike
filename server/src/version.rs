// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiRequestResponse;
use crate::policies::PolicySystem;
use crate::server::Server;
use crate::types::TypeSystem;
use crate::worker::{self, WorkerInit};
use anyhow::{Result, bail};
use futures::stream::{FuturesUnordered, TryStreamExt};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task;
use tokio::sync::{mpsc, oneshot};
use utils::{TaskHandle, CancellableTaskHandle};

pub struct VersionInit {
    pub version_id: String,
    pub info: VersionInfo,
    pub server: Arc<Server>,
    pub modules: Arc<HashMap<String, String>>,
    pub type_system: Arc<TypeSystem>,
    pub policy_system: Arc<PolicySystem>,
    pub worker_count: usize,
    /// We will signal you on this channel when all workers in the version are ready to accept
    /// requests.
    pub ready_tx: oneshot::Sender<()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub name: String,
    pub tag: String,
}

pub struct Version {
    pub version_id: String,
    pub info: VersionInfo,
    pub type_system: Arc<TypeSystem>,
    pub policy_system: Arc<PolicySystem>,
    /// A request sent to this channel will be processed by a worker in this version.
    pub request_tx: mpsc::Sender<ApiRequestResponse>,
}

pub async fn spawn(init: VersionInit) -> Result<(Arc<Version>, CancellableTaskHandle<Result<()>>)> {
    let (request_tx, request_rx) = mpsc::channel(1);
    let version = Arc::new(Version {
        version_id: init.version_id.clone(),
        info: init.info.clone(),
        type_system: init.type_system.clone(),
        policy_system: init.policy_system.clone(),
        request_tx,
    });
    let task = CancellableTaskHandle(task::spawn(run(init, version.clone(), request_rx)));
    Ok((version, task))
}

async fn run(
    init: VersionInit,
    version: Arc<Version>,
    mut request_rx: mpsc::Receiver<ApiRequestResponse>,
) -> Result<()> {
    let ready_rxs = FuturesUnordered::new();
    let mut request_txs = Vec::new();
    let worker_handles = FuturesUnordered::new();

    // spawn all workers for this version
    for worker_idx in 0..init.worker_count {
        let (ready_tx, ready_rx) = oneshot::channel();
        let (request_tx, request_rx) = async_channel::bounded(1);
        let worker_handle = worker::spawn(WorkerInit {
            worker_idx,
            server: init.server.clone(),
            version: version.clone(),
            modules: init.modules.clone(),
            ready_tx,
            request_rx,
        }).await?;

        ready_rxs.push(ready_rx);
        request_txs.push(request_tx);
        worker_handles.push(worker_handle);
    }

    let ready_tx = init.ready_tx;
    let version_id = version.version_id.clone();
    let ready_task = TaskHandle(task::spawn(async move {
        // signal that the version is ready once all workers are ready.
        // if some worker drops its `ready_tx`, we ignore the error and never signal that the
        // version is ready. the worker will most likely return an error from its `worker_handle`
        // anyway, so it is better if we propagate _that_ error
        if ready_rxs.try_collect::<()>().await.is_ok() {
            let _ = ready_tx.send(());
            info!("Version {:?} is ready", version_id);
        }
        Ok(())
    }));

    let version_id = version.version_id.clone();
    let request_task = TaskHandle(task::spawn(async move {
        // distribute requests among workers in a round-robin fashion
        let mut worker_i = 0;
        while let Some(request) = request_rx.recv().await {
            if request_txs[worker_i].send(request).await.is_err() {
                bail!("Worker {:?} {} is unable to accept requests", version_id, worker_i);
            }
            worker_i = (worker_i + 1) % request_txs.len();
        }
        Ok(())
    }));

    let join_task = TaskHandle(task::spawn(async move {
        // join all spawned workers
        worker_handles.try_collect::<()>().await
    }));

    tokio::try_join!(ready_task, request_task, join_task)?;
    Ok(())
}
