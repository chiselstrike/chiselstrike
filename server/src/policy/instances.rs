#![allow(dead_code)]
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use boa_engine::prelude::JsObject;
use boa_engine::JsValue;
use paste::paste;

use crate::datastore::expr::Expr;
use crate::types::ObjectType;

use super::type_policy::{GeoLocPolicy, ReadPolicy, TransformPolicy, WritePolicy};
use super::{Action, Location, PolicyContext};

/// The PolicyEvalInstance contains instances of policy and cache valid for a type in a given
/// context.
pub struct PolicyEvalInstance {
    read: Option<ReadPolicyInstance>,
    create: Option<WritePolicyInstance>,
    update: Option<WritePolicyInstance>,
    on_read: Option<TransformPolicyInstance>,
    on_write: Option<TransformPolicyInstance>,
    geoloc: Option<GeoLocPolicyInstance>,
    /// Set of object marked dirty by this instance.
    dirty: HashSet<String>,
    ty: Arc<ObjectType>,
    chisel_ctx: JsValue,
}

/// generate a function that gets the action for the given policy
macro_rules! create_get_or_load_instance {
    ($policy:ident, $instance_ty:ty) => {
        paste! {
            pub fn [<get_or_load_ $policy _policy_instance>](&mut self, ctx: &PolicyContext) -> Result<Option<&mut $instance_ty>> {
                if let Some(ref mut instance) = self.$policy {
                    return Ok(Some(instance));
                }

                match ctx.engine.store.borrow().get(self.ty.name()) {
                    Some(crate::policy::type_policy::TypePolicy {
                        $policy: Some(policy),
                        ..
                    }) => {
                        let instance = $instance_ty::new(ctx, policy)?;
                        self.$policy.replace(instance);
                        Ok(self.$policy.as_mut())
                    }
                        _ => Ok(None),
                }
            }
        }
    };
}

impl PolicyEvalInstance {
    pub fn new(ctx: &PolicyContext, ty: Arc<ObjectType>) -> Self {
        let mut boa_ctx = ctx.engine.boa_ctx.borrow_mut();
        let chisel_ctx = ctx.request.to_js_value(&mut boa_ctx);
        Self {
            dirty: Default::default(),
            ty,
            read: None,
            create: None,
            update: None,
            on_read: None,
            on_write: None,
            geoloc: None,
            chisel_ctx,
        }
    }

    pub fn mark_dirty(&mut self, id: &str) {
        self.dirty.insert(id.to_owned());
    }

    pub fn is_dirty(&mut self, id: &str) -> bool {
        self.dirty.contains(id)
    }

    pub fn make_read_filter_expr(&mut self, ctx: &PolicyContext) -> Result<Option<&Expr>> {
        Ok(self
            .get_or_load_read_policy_instance(ctx)?
            .and_then(|p| p.get_fitler_expr()))
    }

    pub fn get_read_action(
        &mut self,
        ctx: &PolicyContext,
        val: &JsValue,
    ) -> Result<Option<Action>> {
        let chisel_ctx = self.chisel_ctx.clone();
        self.get_or_load_read_policy_instance(ctx)?
            .map(|p| p.get_action(ctx, val, &chisel_ctx))
            .transpose()
    }

    pub fn get_create_action(
        &mut self,
        ctx: &PolicyContext,
        val: &JsValue,
    ) -> Result<Option<Action>> {
        let chisel_ctx = self.chisel_ctx.clone();
        match self.get_read_action(ctx, val)? {
            Some(action) if action.is_restrictive() => Ok(Some(action)),
            _ => self
                .get_or_load_create_policy_instance(ctx)?
                .map(|p| p.get_action(ctx, val, &chisel_ctx))
                .transpose(),
        }
    }

    pub fn get_update_action(
        &mut self,
        ctx: &PolicyContext,
        val: &JsValue,
    ) -> Result<Option<Action>> {
        let chisel_ctx = self.chisel_ctx.clone();
        match self.get_read_action(ctx, val)? {
            Some(action) if action.is_restrictive() => Ok(Some(action)),
            _ => self
                .get_or_load_update_policy_instance(ctx)?
                .map(|p| p.get_action(ctx, val, &chisel_ctx))
                .transpose(),
        }
    }

    /// Applies the onRead transform to value.
    ///
    /// This mutates value! therefore value should be set as mutable.
    pub fn transform_on_read(&mut self, ctx: &PolicyContext, val: &JsValue) -> Result<()> {
        let chisel_ctx = self.chisel_ctx.clone();
        self.get_or_load_on_read_policy_instance(ctx)?
            .map(|p| p.transform(ctx, val, &chisel_ctx))
            .transpose()?;

        Ok(())
    }

    /// Applies the onRead transform to value.
    ///
    /// This mutates value! therefore value should be set as mutable.
    pub fn transform_on_write(&mut self, ctx: &PolicyContext, val: &JsValue) -> Result<()> {
        let chisel_ctx = self.chisel_ctx.clone();
        self.get_or_load_on_write_policy_instance(ctx)?
            .map(|p| p.transform(ctx, val, &chisel_ctx))
            .transpose()?;

        Ok(())
    }

    pub fn geo_loc(&mut self, ctx: &PolicyContext, val: &JsValue) -> Result<Option<Location>> {
        let chisel_ctx = self.chisel_ctx.clone();
        self.get_or_load_geoloc_policy_instance(ctx)?
            .map(|p| p.geo_loc(ctx, val, &chisel_ctx))
            .transpose()
    }

    create_get_or_load_instance!(read, ReadPolicyInstance);
    create_get_or_load_instance!(create, WritePolicyInstance);
    create_get_or_load_instance!(update, WritePolicyInstance);
    create_get_or_load_instance!(on_read, TransformPolicyInstance);
    create_get_or_load_instance!(on_write, TransformPolicyInstance);
    create_get_or_load_instance!(geoloc, GeoLocPolicyInstance);
}

/// Trait implemented by types that have a filter funtion that return an action.
pub trait Filter {
    fn filter_function(&self) -> JsObject;

    fn get_action(
        &self,
        ctx: &PolicyContext,
        value: &JsValue,
        chisel_ctx: &JsValue,
    ) -> Result<Action> {
        let result = ctx
            .engine
            .call(self.filter_function(), &[value.clone(), chisel_ctx.clone()])?;
        match result {
            JsValue::Integer(action) => action.try_into(),
            val => anyhow::bail!("invalid action: {val:?}"),
        }
    }
}

pub struct ReadPolicyInstance {
    function: JsObject,
    expr: Option<Expr>,
}

impl Filter for ReadPolicyInstance {
    fn filter_function(&self) -> JsObject {
        self.function.clone()
    }
}

impl ReadPolicyInstance {
    pub fn new(ctx: &PolicyContext, policy: &ReadPolicy) -> Result<Self> {
        let expr = ctx.engine.eval_read_policy_expr(policy, &*ctx.request)?;
        Ok(Self {
            function: policy.function.clone(),
            expr,
        })
    }

    /// Returns the filter Expr for that Filter.
    pub fn get_fitler_expr(&self) -> Option<&Expr> {
        self.expr.as_ref()
    }
}

pub struct WritePolicyInstance {
    function: JsObject,
}

impl WritePolicyInstance {
    // ctx is just here to help with codegen
    pub fn new(_ctx: &PolicyContext, policy: &WritePolicy) -> Result<Self> {
        Ok(Self {
            function: policy.function.clone(),
        })
    }
}

impl Filter for WritePolicyInstance {
    fn filter_function(&self) -> JsObject {
        self.function.clone()
    }
}

pub struct TransformPolicyInstance {
    // object containing the transform js function
    function: JsObject,
}

impl TransformPolicyInstance {
    /// applies the transform to value.
    pub fn transform(
        &self,
        ctx: &PolicyContext,
        value: &JsValue,
        chisel_ctx: &JsValue,
    ) -> Result<()> {
        ctx.engine
            .call(self.function.clone(), &[value.clone(), chisel_ctx.clone()])?;

        Ok(())
    }

    pub fn new(_ctx: &PolicyContext, p: &TransformPolicy) -> Result<Self> {
        Ok(Self {
            function: p.function.clone(),
        })
    }
}

pub struct GeoLocPolicyInstance {
    // object containing the transform js function
    function: JsObject,
}

impl GeoLocPolicyInstance {
    pub fn new(_ctx: &PolicyContext, p: &GeoLocPolicy) -> Result<Self> {
        Ok(Self {
            function: p.function.clone(),
        })
    }

    pub fn geo_loc(
        &mut self,
        ctx: &PolicyContext,
        value: &JsValue,
        chisel_ctx: &JsValue,
    ) -> Result<Location> {
        let result = ctx
            .engine
            .call(self.function.clone(), &[value.clone(), chisel_ctx.clone()])?;

        match result {
            JsValue::String(ref s) => Location::from_str(s),
            _ => anyhow::bail!("Expected geolocation to return a string."),
        }
    }
}
