// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::query::{MetaService, QueryEngine};
use crate::types::TypeSystem;
use once_cell::sync::OnceCell;
use tokio::sync::{Mutex, MutexGuard};

pub struct Runtime {
    pub query_engine: QueryEngine,
    pub meta: MetaService,
    pub type_system: TypeSystem,
}

impl Runtime {
    pub fn new(query_engine: QueryEngine, meta: MetaService, type_system: TypeSystem) -> Self {
        Self {
            query_engine,
            meta,
            type_system,
        }
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
