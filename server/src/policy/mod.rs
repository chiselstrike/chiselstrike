#![allow(dead_code)]
use std::cell::{RefCell, RefMut};
use std::collections::{hash_map::Entry, HashMap};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Result};

use crate::datastore::value::{EntityMap, EntityValue};
use crate::types::ObjectType;

use self::engine::{ChiselRequestContext, PolicyEngine};
use self::instances::PolicyEvalInstance;
use self::utils::{entity_map_to_js_value, js_value_to_entity_value};

pub mod engine;
mod instances;
mod interpreter;
pub mod store;
pub mod type_policy;
mod utils;

pub struct PolicyContext {
    pub cache: PolicyInstancesCache,
    pub engine: Rc<PolicyEngine>,
    pub request: Rc<dyn ChiselRequestContext>,
}

impl PolicyContext {
    pub fn new(engine: Rc<PolicyEngine>, request: Rc<dyn ChiselRequestContext>) -> Self {
        let cache = PolicyInstancesCache::default();
        Self {
            cache,
            engine,
            request,
        }
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("could not write `{}` to disk: Permission denied", .0.name())]
    WritePermissionDenied(Arc<ObjectType>),
    #[error("could not Read `{}`: Permission denied", .0.name())]
    ReadPermissionDenied(Arc<ObjectType>),
    #[error("could not write `{}`: Entity is dirty: it was transformed by a policy.", .0.name())]
    DirtyEntity(Arc<ObjectType>),
}

#[derive(Debug)]
#[repr(u8)]
pub enum Action {
    Allow = 0,
    Deny = 1,
    Skip = 2,
    Log = 3,
}

impl TryFrom<i32> for Action {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Allow),
            1 => Ok(Self::Deny),
            2 => Ok(Self::Skip),
            3 => Ok(Self::Log),
            _ => bail!("invalid Action"),
        }
    }
}

impl Action {
    pub fn is_restrictive(&self) -> bool {
        match self {
            Action::Deny | Action::Skip => true,
            Action::Allow | Action::Log => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Location {
    UsEast1,
    UsWest,
    London,
    Germany,
}

impl FromStr for Location {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "us-east-1" => Ok(Self::UsEast1),
            "us-west" => Ok(Self::UsWest),
            "london" => Ok(Self::London),
            "germany" => Ok(Self::Germany),
            other => bail!("unknown region {other}"),
        }
    }
}
#[derive(Default)]
pub struct PolicyInstancesCache {
    inner: RefCell<HashMap<String, PolicyEvalInstance>>,
}

impl PolicyInstancesCache {
    pub fn get_or_create_policy_instance(
        &self,
        ctx: &PolicyContext,
        ty: &Arc<ObjectType>,
    ) -> RefMut<PolicyEvalInstance> {
        let inner = self.inner.borrow_mut();
        RefMut::map(inner, |inner| match inner.entry(ty.name().to_owned()) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let instance = PolicyEvalInstance::new(ctx, ty.clone());
                e.insert(instance)
            }
        })
    }
}
