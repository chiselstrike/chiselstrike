// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiService;
use crate::policies::{FieldPolicies, Kind, Policies};
use crate::query::{MetaService, QueryEngine};
use crate::rcmut::RcMut;
use crate::secrets::SecretManager;
use crate::types::{ObjectType, TypeSystem};
use derive_new::new;
use once_cell::sync::OnceCell;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(new)]
pub(crate) struct Runtime {
    pub(crate) api: Rc<ApiService>,
    pub(crate) query_engine: Rc<QueryEngine>,
    pub(crate) meta: Rc<MetaService>,
    pub(crate) type_system: TypeSystem,
    pub(crate) policies: Policies,
    pub(crate) secrets: SecretManager,
}

impl Runtime {
    /// Adds the current policies on ty's fields to policies.
    pub(crate) fn add_field_policies(
        &self,
        ty: &ObjectType,
        policies: &mut FieldPolicies,
        current_path: &str,
    ) {
        if let Some(version) = self.policies.versions.get(&ty.api_version) {
            for fld in ty.user_fields() {
                for lbl in &fld.labels {
                    if let Some(p) = version.labels.get(lbl) {
                        if !p.except_uri.is_match(current_path) {
                            match p.kind {
                                Kind::Transform(f) => {
                                    policies.transforms.insert(fld.name.clone(), f);
                                }
                                Kind::MatchLogin => {
                                    policies.match_login.insert(fld.name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

thread_local!(static RUNTIME: OnceCell<Rc<RefCell<Runtime>>> = OnceCell::new());

pub(crate) fn set(rt: Runtime) {
    RUNTIME.with(|x| {
        x.set(Rc::new(RefCell::new(rt)))
            .map_err(|_| ())
            .expect("Runtime is already initialized.");
    })
}

pub(crate) fn get() -> RcMut<Runtime> {
    RUNTIME.with(|x| {
        let rc = x.get().expect("Runtime is not yet initialized.").clone();
        RcMut::new(rc)
    })
}
