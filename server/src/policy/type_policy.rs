use std::sync::Arc;

use boa_engine::prelude::JsObject;
use chiselc::policies::{Cond, Environment, FilterPolicy, Predicates};

#[derive(Debug, Clone)]
pub struct SkipFilter {
    pub skip_condition: Option<Cond>,
    pub predicates: Predicates,
    pub env: Arc<Environment>,
    pub ctx_param_name: String,
    pub entity_param_name: String,
}

#[derive(Debug, Clone)]
pub struct ReadPolicy {
    pub filter: Option<SkipFilter>,
    pub function: JsObject,
}

impl ReadPolicy {
    pub fn new(function: JsObject, policy: &FilterPolicy) -> Self {
        match policy {
            FilterPolicy::Optimized(policy) => {
                let entity_param_name = policy.params.get_positional_param_name(0).to_owned();
                let ctx_param_name = policy.params.get_positional_param_name(1).to_owned();
                let filter = Some(SkipFilter {
                    predicates: policy.predicates.clone(),
                    skip_condition: policy.skip_cond.clone(),
                    env: policy.env.clone(),
                    ctx_param_name,
                    entity_param_name,
                });
                Self { filter, function }
            }
            FilterPolicy::Js { .. } => todo!(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WritePolicy {
    pub function: JsObject,
}

impl WritePolicy {
    pub fn new(function: JsObject) -> Self {
        Self { function }
    }
}

#[derive(Debug, Clone)]
pub struct GeoLocPolicy {
    pub function: JsObject,
}

impl GeoLocPolicy {
    pub fn new(function: JsObject) -> Self {
        Self { function }
    }
}

#[derive(Debug, Clone)]
pub struct TransformPolicy {
    pub function: JsObject,
}

impl TransformPolicy {
    pub fn new(function: JsObject) -> Self {
        Self { function }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TypePolicy {
    pub read: Option<ReadPolicy>,
    pub create: Option<WritePolicy>,
    pub update: Option<WritePolicy>,
    pub geoloc: Option<GeoLocPolicy>,
    pub on_read: Option<TransformPolicy>,
    pub on_create: Option<TransformPolicy>,
    pub on_update: Option<TransformPolicy>,
}
