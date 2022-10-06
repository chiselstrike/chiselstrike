use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{bail, Result};
use boa_engine::object::JsMap;
use boa_engine::prelude::JsObject;
use boa_engine::{JsString, JsValue};
use chiselc::parse::ParserContext;
use chiselc::policies::{Cond, Environment, PolicyName, Predicate, Predicates, Var};
use serde_json::Value as JsonValue;

use super::interpreter::{self, InterpreterContext, JsonResolver};
use super::store::PolicyStore;
use super::type_policy::{GeoLocPolicy, ReadPolicy, TransformPolicy, TypePolicy, WritePolicy};
use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, PropertyAccess, Value};

#[derive(Default)]
pub struct PolicyEngine {
    pub boa_ctx: Rc<RefCell<boa_engine::Context>>,
    pub store: RefCell<PolicyStore>,
}

pub trait ChiselRequestContext {
    fn method(&self) -> &str;
    fn path(&self) -> &str;
    fn headers(&self) -> Box<dyn Iterator<Item = (&str, &str)> + '_>;
    fn user_id(&self) -> Option<&str>;

    // TODO: need to find a way around using json here.
    fn to_value(&self) -> JsonValue {
        serde_json::json!({
            "method": self.method(),
            "path": self.path(),
            "headers": self.headers().collect::<HashMap<_, _>>(),
            "user_id": self.user_id(),
        })
    }

    fn to_js_value(&self, ctx: &mut boa_engine::Context) -> JsValue {
        let map = JsMap::new(ctx);

        map.set(JsString::from("method"), JsString::from(self.method()), ctx)
            .unwrap();
        map.set(JsString::from("path"), JsString::from(self.path()), ctx)
            .unwrap();

        let user_id = match self.user_id() {
            Some(val) => JsValue::String(JsString::from(val)),
            None => JsValue::Null,
        };
        map.set(JsString::from("user_id"), user_id, ctx).unwrap();

        let headers = JsMap::new(ctx);
        for (key, val) in self.headers() {
            headers
                .set(JsString::new(key), JsString::new(val), ctx)
                .unwrap();
        }

        map.set(JsString::from("headers"), JsObject::from(headers), ctx)
            .unwrap();

        JsValue::Object(JsObject::from(map))
    }
}

#[allow(dead_code)]
impl PolicyEngine {
    pub fn new(store: PolicyStore) -> Self {
        Self {
            boa_ctx: Default::default(),
            store: store.into(),
        }
    }

    pub fn with_store_mut(&self, f: impl FnOnce(&mut PolicyStore)) {
        let mut store = self.store.borrow_mut();
        f(&mut store);
    }

    pub fn get_policy(&self, ty: &str) -> Option<Ref<TypePolicy>> {
        let store = self.store.borrow();
        // this is a trick to get an Option<Ref<T>> from a Option<&T>
        if store.get(ty).is_some() {
            Some(Ref::map(store, |s| s.get(ty).unwrap()))
        } else {
            None
        }
    }

    pub fn register_policy_from_code(&self, ty_name: String, code: &[u8]) -> anyhow::Result<()> {
        let ctx = ParserContext::new();
        let module = ctx.parse(std::str::from_utf8(code).unwrap().to_owned(), false)?;
        let policies = chiselc::policies::Policies::parse(&module)?;
        let mut type_policy = TypePolicy::default();
        for (name, policy) in policies.iter() {
            let function = self.compile_function(policy.code())?;
            match name {
                PolicyName::Read => {
                    let policy = policy.as_filter().unwrap();
                    let policy = ReadPolicy::new(function, policy);
                    type_policy.read.replace(policy);
                }
                PolicyName::Create => {
                    let policy = WritePolicy::new(function);
                    type_policy.create.replace(policy);
                }
                PolicyName::Update => {
                    let policy = WritePolicy::new(function);
                    type_policy.update.replace(policy);
                }
                PolicyName::OnRead => {
                    let policy = TransformPolicy::new(function);
                    type_policy.on_read.replace(policy);
                }
                PolicyName::OnSave => {
                    let policy = TransformPolicy::new(function);
                    type_policy.on_save.replace(policy);
                }
                PolicyName::GeoLoc => {
                    let policy = GeoLocPolicy::new(function);
                    type_policy.geoloc.replace(policy);
                }
            }
        }

        self.store.borrow_mut().insert(ty_name, type_policy);

        Ok(())
    }

    pub fn eval_read_policy_expr(
        &self,
        policy: &ReadPolicy,
        chisel_ctx: &dyn ChiselRequestContext,
    ) -> Result<Option<Expr>> {
        match policy.filter {
            Some(ref filter) => {
                let chisel_ctx = chisel_ctx.to_value();
                let resolver = JsonResolver {
                    name: &policy.ctx_param_name,
                    value: &chisel_ctx,
                };

                let mut context = InterpreterContext {
                    env: &policy.env,
                    resolver: &resolver,
                    boa: &mut self.boa_ctx.borrow_mut(),
                };

                let predicates = policy
                    .predicates
                    .map(|p| interpreter::eval(p, &mut context));
                let cond = filter.simplify(&predicates);
                cond_to_expr(&cond, &predicates, &policy.entity_param_name, &policy.env).map(Some)
            }
            None => Ok(None),
        }
    }

    fn compile_function(&self, code: &[u8]) -> Result<JsObject> {
        Ok(self
            .boa_ctx
            .borrow_mut()
            .eval(code)
            .unwrap()
            .as_object()
            .unwrap()
            .clone())
    }

    pub fn call(&self, function: JsObject, args: &[JsValue]) -> anyhow::Result<JsValue> {
        function
            .call(&JsValue::Null, args, &mut self.boa_ctx.borrow_mut())
            .map_err(boa_err_to_anyhow)
    }
}

fn boa_err_to_anyhow(_e: JsValue) -> anyhow::Error {
    todo!()
}

fn cond_to_expr(
    cond: &Cond,
    preds: &Predicates,
    entity_param_name: &str,
    env: &Environment,
) -> Result<Expr> {
    let val = match cond {
        Cond::And(left, right) => {
            let right = cond_to_expr(right, preds, entity_param_name, env)?;
            let left = cond_to_expr(left, preds, entity_param_name, env)?;
            Expr::Binary(BinaryExpr {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            })
        }
        Cond::Or(left, right) => {
            let right = cond_to_expr(right, preds, entity_param_name, env)?;
            let left = cond_to_expr(left, preds, entity_param_name, env)?;
            Expr::Binary(BinaryExpr {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            })
        }
        Cond::Not(cond) => Expr::Not(Box::new(cond_to_expr(cond, preds, entity_param_name, env)?)),
        Cond::Predicate(id) => {
            let predicate = preds.get(*id);
            predicate_to_expr(predicate, entity_param_name, env)?
        }
        Cond::True => Expr::Value {
            value: Value::Bool(true),
        },
        Cond::False => Expr::Value {
            value: Value::Bool(false),
        },
    };

    Ok(val)
}

fn predicate_to_expr(pred: &Predicate, entity_param_name: &str, env: &Environment) -> Result<Expr> {
    let val = match pred {
        Predicate::Bin { op, lhs, rhs } => {
            let left = Box::new(predicate_to_expr(lhs, entity_param_name, env)?);
            let right = Box::new(predicate_to_expr(rhs, entity_param_name, env)?);
            Expr::Binary(BinaryExpr {
                op: BinaryOp::from(*op),
                left,
                right,
            })
        }
        Predicate::Not(_) => todo!(),
        Predicate::Lit(val) => Expr::Value {
            value: Value::from(val),
        },
        Predicate::Var(var) => {
            let var = env.get(*var);
            match var {
                Var::Ident(id) => bail!("unknown variable: `{id}`"),
                Var::Member(obj, prop) => {
                    let obj = env.get(*obj);
                    match obj {
                        Var::Ident(n) if n == entity_param_name => {
                            let property_chain = Expr::Parameter { position: 0 };
                            Expr::Property(PropertyAccess {
                                property: prop.to_string(),
                                object: Box::new(property_chain),
                            })
                        }
                        other => bail!("unknown variable: `{other:?}`"),
                    }
                }
            }
        }
    };

    Ok(val)
}
