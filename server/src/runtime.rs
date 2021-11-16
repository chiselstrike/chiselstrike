// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::policies::{FieldPolicies, LabelPolicies};
use crate::query::{MetaService, QueryEngine};
use crate::types::{ObjectType, TypeSystem};
use once_cell::sync::OnceCell;
use tokio::sync::{Mutex, MutexGuard};

pub struct Runtime {
    pub query_engine: QueryEngine,
    pub meta: MetaService,
    pub type_system: TypeSystem,
    pub policies: LabelPolicies,
}

impl Runtime {
    pub fn new(query_engine: QueryEngine, meta: MetaService, type_system: TypeSystem) -> Self {
        Self {
            query_engine,
            meta,
            type_system,
            policies: LabelPolicies::default(),
        }
    }

    /// Adds the current policies of ty to policies.
    pub fn get_policies(&self, ty: &ObjectType, policies: &mut FieldPolicies) {
        for fld in &ty.fields {
            for lbl in &fld.labels {
                if let Some(xform) = self.policies.get(lbl) {
                    policies.insert(fld.name.clone(), *xform);
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
