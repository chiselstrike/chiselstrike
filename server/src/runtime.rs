// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::store::Store;
use crate::types::TypeSystem;
use once_cell::sync::Lazy;
use tokio::sync::{MappedMutexGuard, Mutex, MutexGuard};

pub struct Runtime {
    pub store: Store,
    pub type_system: TypeSystem,
}

impl Runtime {
    pub fn new(store: Store, type_system: TypeSystem) -> Self {
        Self { store, type_system }
    }
}

pub async fn set(rt: Runtime) {
    let mut g = RUNTIME.lock().await;
    *g = Some(rt);
}

pub async fn get() -> MappedMutexGuard<'static, Runtime> {
    MutexGuard::map(RUNTIME.lock().await, |x| {
        x.as_mut().expect("Runtime is not yet initialized.")
    })
}

static RUNTIME: Lazy<Mutex<Option<Runtime>>> = Lazy::new(|| Mutex::new(None));
