// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::nursery::Nursery;
use crate::version::{Version, VersionJob};
use anyhow::Result;
use futures::stream::StreamExt;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use utils::{CancellableTaskHandle, TaskHandle};

/// Manager of versions (branches).
///
/// The trunk keeps track of the active [`Version`]s and monitors the version tasks.
pub struct Trunk {
    versions: RwLock<HashMap<String, TrunkVersion>>,
    nursery: Nursery<CancellableTaskHandle<Result<()>>>,
}

#[derive(Clone)]
pub struct TrunkVersion {
    pub version: Arc<Version>,
    /// NOTE: this sender cannot be stored in the `Version`, because the version terminates only
    /// after this sender (and its clones) are dropped. If the sender was in `Version`, it would
    /// never get dropped and the version would never terminate.
    pub job_tx: mpsc::Sender<VersionJob>,
}

impl Trunk {
    pub fn list_versions(&self) -> Vec<Arc<Version>> {
        self.versions.read().values().map(|v| v.version.clone()).collect()
    }

    pub fn list_trunk_versions(&self) -> Vec<TrunkVersion> {
        self.versions.read().values().cloned().collect()
    }

    pub fn get_trunk_version(&self, version_id: &str) -> Option<TrunkVersion> {
        self.versions.read().get(version_id).cloned()
    }

    pub fn get_version(&self, version_id: &str) -> Option<Arc<Version>> {
        self.versions.read().get(version_id).map(|v| v.version.clone())
    }

    // Adds a new version to the trunk.
    // `job_tx` is the channel that will receive all jobs for this version from now on, and `task`
    // is the task that runs the version.
    pub fn add_version(
        &self,
        version: Arc<Version>,
        job_tx: mpsc::Sender<VersionJob>,
        task: CancellableTaskHandle<Result<()>>,
    ) {
        let version_id = version.version_id.clone();
        self.versions.write().insert(version_id, TrunkVersion { version, job_tx });
        self.nursery.nurse(task);
    }

    pub fn remove_version(&self, version_id: &str) -> Option<Arc<Version>> {
        self.versions.write()
            .remove(version_id)
            .map(|trunk_version| trunk_version.version)
        // if there is still a task in `self.nursery` for this version, we just leave it alone. it
        // should terminate on its own when its `mpsc::Sender<VersionJob>` is dropped.
    }
}

pub async fn spawn() -> Result<(Trunk, TaskHandle<Result<()>>)> {
    let (nursery, mut nursery_stream) = Nursery::new();
    let trunk = Trunk {
        versions: RwLock::new(HashMap::new()),
        nursery,
    };

    let task = TaskHandle(tokio::task::spawn(async move  {
        while let Some(result) = nursery_stream.next().await {
            match result {
                Some(Ok(_)) => {}, // task terminated successfully
                Some(Err(err)) => return Err(err),
                None => {}, // task was cancelled
            }
        }
        Ok(())
    }));

    Ok((trunk, task))
}
