// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::version::{Version, VersionJob};
use anyhow::Result;
use futures::future;
use futures::stream::{FuturesUnordered, Stream};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use tokio::sync::mpsc;
use utils::{CancellableTaskHandle, TaskHandle};

/// Manager of versions (branches).
///
/// The trunk keeps track of the active [`Version`]s and monitors the version tasks.
pub struct Trunk {
    state: Arc<RwLock<TrunkState>>,
}

#[derive(Default)]
struct TrunkState {
    versions: HashMap<String, TrunkVersion>,
    tasks: FuturesUnordered<CancellableTaskHandle<Result<()>>>,
    waker: Option<Waker>,
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
        let state = self.state.read();
        state.versions.values().map(|v| v.version.clone()).collect()
    }

    pub fn list_trunk_versions(&self) -> Vec<TrunkVersion> {
        let state = self.state.read();
        state.versions.values().cloned().collect()
    }

    pub fn get_trunk_version(&self, version_id: &str) -> Option<TrunkVersion> {
        let state = self.state.read();
        state.versions.get(version_id).cloned()
    }

    pub fn get_version(&self, version_id: &str) -> Option<Arc<Version>> {
        let state = self.state.read();
        state.versions.get(version_id).map(|v| v.version.clone())
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
        let mut state = self.state.write();
        let version_id = version.version_id.clone();
        state
            .versions
            .insert(version_id, TrunkVersion { version, job_tx });
        state.tasks.push(task);
        // we added the task to `state.tasks`, but we need to explicitly wake up the task that
        // polls `state.tasks`, otherwise we won't get notifications from the newly added task (see
        // documentation of `FuturesUnordered` for details)
        if let Some(waker) = state.waker.take() {
            waker.wake()
        }
    }

    pub fn remove_version(&self, version_id: &str) -> Option<Arc<Version>> {
        let mut state = self.state.write();
        // if there is still a task in `state.tasks` for this version, we just leave it alone. it
        // should terminate on its own when its `mpsc::Sender<VersionJob>` is dropped.
        state
            .versions
            .remove(version_id)
            .map(|trunk_version| trunk_version.version)
    }
}

pub async fn spawn() -> Result<(Trunk, TaskHandle<Result<()>>)> {
    let state = Arc::new(RwLock::new(TrunkState::default()));
    let trunk = Trunk {
        state: state.clone(),
    };
    let fut = future::poll_fn(move |cx| poll(&mut state.write(), cx));
    let task = TaskHandle(tokio::task::spawn(fut));
    Ok((trunk, task))
}

// Polls the trunk for completion.
fn poll(state: &mut TrunkState, cx: &mut Context) -> Poll<Result<()>> {
    while let Poll::Ready(Some(task_res)) = Pin::new(&mut state.tasks).poll_next(cx) {
        if let Some(Err(task_err)) = task_res {
            return Poll::Ready(Err(task_err));
        }
    }
    state.waker = Some(cx.waker().clone());
    Poll::Pending
}
