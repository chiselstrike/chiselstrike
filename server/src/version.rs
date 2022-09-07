// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::http::HttpRequestResponse;
use crate::kafka::KafkaEvent;
use crate::policies::PolicySystem;
use crate::server::Server;
use crate::types::TypeSystem;
use crate::worker::{self, WorkerInit};
use anyhow::{bail, Result};
use futures::stream::{FuturesUnordered, TryStreamExt};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::task;
use utils::{CancellableTaskHandle, TaskHandle};

pub struct VersionInit {
    pub version_id: String,
    pub info: VersionInfo,
    pub server: Arc<Server>,
    /// Module map (see `ModuleLoader`).
    pub modules: Arc<HashMap<String, String>>,
    pub type_system: Arc<TypeSystem>,
    pub policy_system: Arc<PolicySystem>,
    pub worker_count: usize,
    /// We will signal you on this channel when all workers in the version are ready to accept
    /// jobs.
    pub ready_tx: oneshot::Sender<()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub name: String,
    pub tag: String,
}

/// Instance of a version of the user's code.
///
/// There might be multiple versions running in the same server, but they are almost completely
/// independent. There might also be multiple JS runtimes (workers) running code for the version,
/// sharing the same instance of this object.
pub struct Version {
    pub version_id: String,
    pub info: VersionInfo,
    pub type_system: Arc<TypeSystem>,
    pub policy_system: Arc<PolicySystem>,
}

/// A job that should be handled by a version (more precisely, by one of the workers in the
/// version).
#[derive(Debug)]
pub enum VersionJob {
    Http(HttpRequestResponse),
    Kafka(KafkaEvent),
}

pub async fn spawn(
    init: VersionInit,
) -> Result<(
    Arc<Version>,
    mpsc::Sender<VersionJob>,
    CancellableTaskHandle<Result<()>>,
)> {
    let (job_tx, job_rx) = mpsc::channel(1);
    let version = Arc::new(Version {
        version_id: init.version_id.clone(),
        info: init.info.clone(),
        type_system: init.type_system.clone(),
        policy_system: init.policy_system.clone(),
    });
    let task = CancellableTaskHandle(task::spawn(run(init, version.clone(), job_rx)));
    Ok((version, job_tx, task))
}

async fn run(
    init: VersionInit,
    version: Arc<Version>,
    mut job_rx: mpsc::Receiver<VersionJob>,
) -> Result<()> {
    let worker_ready_rxs = FuturesUnordered::new();
    let mut worker_job_txs = Vec::new();
    let worker_handles = FuturesUnordered::new();

    // spawn all workers for this version
    for worker_idx in 0..init.worker_count {
        let (worker_ready_tx, worker_ready_rx) = oneshot::channel();
        let (worker_job_tx, worker_job_rx) = mpsc::channel(1);
        let worker_handle = worker::spawn(WorkerInit {
            worker_idx,
            server: init.server.clone(),
            version: version.clone(),
            modules: init.modules.clone(),
            ready_tx: worker_ready_tx,
            job_rx: worker_job_rx,
        })
        .await?;

        worker_ready_rxs.push(worker_ready_rx);
        worker_job_txs.push(worker_job_tx);
        worker_handles.push(worker_handle);
    }

    let ready_tx = init.ready_tx;
    let version_id = version.version_id.clone();
    let ready_task = TaskHandle(task::spawn(async move {
        // signal that the version is ready once all workers are ready.
        // if some worker drops its `ready_tx`, we ignore the error and never signal that the
        // version is ready. the worker will most likely return an error from its `worker_handle`
        // anyway, so it is better if we propagate _that_ error
        if worker_ready_rxs.try_collect::<()>().await.is_ok() {
            let _ = ready_tx.send(());
            info!("Version {:?} is ready", version_id);
        }
        Ok(())
    }));

    let version_id = version.version_id.clone();
    let job_task = TaskHandle(task::spawn(async move {
        // distribute jobs among workers in a round-robin fashion
        // TODO: we should perhaps be more clever than round-robin
        let mut worker_i = 0;
        while let Some(job) = job_rx.recv().await {
            if worker_job_txs[worker_i].send(job).await.is_err() {
                bail!(
                    "Worker {:?} {} is unable to accept jobs",
                    version_id,
                    worker_i
                );
            }
            worker_i = (worker_i + 1) % worker_job_txs.len();
        }
        Ok(())
    }));

    let join_task = TaskHandle(task::spawn(async move {
        // join all spawned workers
        worker_handles.try_collect::<()>().await
    }));

    tokio::try_join!(ready_task, job_task, join_task)?;
    Ok(())
}
