// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::version::Version;
use anyhow::Result;
use futures::future;
use futures::stream::{FuturesUnordered, Stream};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use utils::{TaskHandle, CancellableTaskHandle};

pub struct Trunk {
    state: Arc<RwLock<TrunkState>>,
}

#[derive(Default)]
struct TrunkState {
    versions: HashMap<String, Arc<Version>>,
    tasks: FuturesUnordered<CancellableTaskHandle<Result<()>>>,
    waker: Option<Waker>,
}

impl Trunk {
    pub fn list_versions(&self) -> Vec<Arc<Version>> {
        let state = self.state.read();
        state.versions.values().cloned().collect()
    }

    pub fn get_version(&self, version_id: &str) -> Option<Arc<Version>> {
        let state = self.state.read();
        state.versions.get(version_id).cloned()
    }

    pub fn add_version(&self, version: Arc<Version>, task: CancellableTaskHandle<Result<()>>) {
        let mut state = self.state.write();
        state.versions.insert(version.version_id.clone(), version);
        state.tasks.push(task);
        // we added the task to the `tasks: FuturesUnordered`, but we need to explicitly wake up
        // the task that polls `tasks`, otherwise we won't get notifications from the newly added
        // task (see documentation of `FuturesUnordered` for details)
        state.waker.take().map(|waker| waker.wake());
    }

    pub fn remove_version(&self, version_id: &str) -> Option<Arc<Version>> {
        let mut state = self.state.write();
        // if there is still a task for this version, we just leave it alone. it will terminate on
        // its own when all `Arc<Version>`s are dropped
        state.versions.remove(version_id)
    }
}

pub async fn spawn() -> Result<(Trunk, TaskHandle<Result<()>>)> {
    let state = Arc::new(RwLock::new(TrunkState::default()));
    let trunk = Trunk { state: state.clone() };
    let fut = future::poll_fn(move |cx| poll(&mut state.write(), cx));
    let task = TaskHandle(tokio::task::spawn(fut));
    Ok((trunk, task))
}

fn poll(state: &mut TrunkState, cx: &mut Context) -> Poll<Result<()>> {
    while let Poll::Ready(Some(task_res)) = Pin::new(&mut state.tasks).poll_next(cx) {
        if let Some(Err(task_err)) = task_res {
            return Poll::Ready(Err(task_err));
        }
    }
    state.waker = Some(cx.waker().clone());
    Poll::Pending
}
