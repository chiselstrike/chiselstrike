// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::policies::{FieldPolicies, LabelPolicies};
use crate::query::{MetaService, QueryEngine};
use crate::types::{ObjectType, TypeSystem};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};

pub struct Runtime {
    pub api: Arc<Mutex<ApiService>>,
    pub query_engine: QueryEngine,
    pub meta: MetaService,
    pub type_system: TypeSystem,
    pub policies: LabelPolicies,
}

impl Runtime {
    pub fn new(
        api: Arc<Mutex<ApiService>>,
        query_engine: QueryEngine,
        meta: MetaService,
        type_system: TypeSystem,
    ) -> Self {
        Self {
            api,
            query_engine,
            meta,
            type_system,
            policies: LabelPolicies::default(),
        }
    }

    /// Adds the current policies of ty to policies.
    pub fn get_policies(&self, ty: &ObjectType, policies: &mut FieldPolicies, current_path: &str) {
        for fld in &ty.fields {
            for lbl in &fld.labels {
                if let Some(p) = self.policies.get(lbl) {
                    if !p.except_uri.is_match(current_path) {
                        policies.insert(fld.name.clone(), p.transform);
                    }
                }
            }
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
