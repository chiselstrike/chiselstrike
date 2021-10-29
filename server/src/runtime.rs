// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::query::store::Store;
use crate::types::TypeSystem;
use once_cell::sync::OnceCell;
use tokio::sync::{Mutex, MutexGuard};

pub struct Runtime {
    pub store: Store,
    pub type_system: TypeSystem,
}

impl Runtime {
    pub fn new(store: Store, type_system: TypeSystem) -> Self {
        Self { store, type_system }
    }
}

pub fn set(rt: Runtime) {
    RUNTIME
        .set(Mutex::new(rt))
        .map_err(|_| ())
        .expect("Runtime is already initialized.");
}

pub async fn get() -> MutexGuard<'static, Runtime> {
    RUNTIME
        .get()
        .expect("Runtime is not yet initialized.")
        .lock()
        .await
}

static RUNTIME: OnceCell<Mutex<Runtime>> = OnceCell::new();
