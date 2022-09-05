use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{bail, Result};
use chiselc::policies::{Cond, Policy, PolicyEvalContext, Predicate, Predicates, Values, Var};

use super::store::Store;
use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, PropertyAccess, Value};
use crate::deno::ChiselRequestContext;

#[derive(Default)]
pub struct PolicyEngine {
    eval_context: Rc<RefCell<PolicyEvalContext>>,
    store: Store,
}

/// An evaluation context instance for a given type, in a given request context.
/// This instance allows to build the filter expression, or to test the filters against an entity
/// instance.
pub struct PolicyEvalInstance {
    predicates: Predicates,
    where_conds: Option<Cond>,
    entity_param_name: String,
}

impl PolicyEvalInstance {
    fn new(
        chisel_ctx: &ChiselRequestContext,
        read_policy: &Policy,
        eval_ctx: Rc<RefCell<PolicyEvalContext>>,
    ) -> Result<Self> {
        let entity_param_name = read_policy.params().get_positional_param_name(0).to_owned();
        let ctx_param_name = read_policy.params().get_positional_param_name(1).to_owned();

        let mut env = Values::new();
        let ctx_json = serde_json::to_value(chisel_ctx)?;
        env.insert(ctx_param_name.clone(), ctx_json);

        let predicates = read_policy.predicates.substitute(&env);
        let mut eval_ctx = eval_ctx.borrow_mut();
        let predicates = predicates.eval(&mut eval_ctx);

        let where_conds = read_policy
            .where_conds
            .as_ref()
            .map(|conds| conds.simplify(&predicates));

        Ok(Self {
            predicates,
            where_conds,
            entity_param_name,
        })
    }

    pub fn make_read_filter_expr(&self) -> Result<Option<Expr>> {
        self.where_conds
            .as_ref()
            .map(|w| Self::cond_to_expr(w, &self.predicates, &self.entity_param_name))
            .transpose()
    }

    fn cond_to_expr(cond: &Cond, preds: &Predicates, entity_param_name: &str) -> Result<Expr> {
        let val = match cond {
            Cond::And(left, right) => {
                let right = Self::cond_to_expr(right, preds, entity_param_name)?;
                let left = Self::cond_to_expr(left, preds, entity_param_name)?;
                Expr::Binary(BinaryExpr {
                    left: Box::new(left),
                    op: BinaryOp::And,
                    right: Box::new(right),
                })
            }
            Cond::Or(left, right) => {
                let right = Self::cond_to_expr(right, preds, entity_param_name)?;
                let left = Self::cond_to_expr(left, preds, entity_param_name)?;
                Expr::Binary(BinaryExpr {
                    left: Box::new(left),
                    op: BinaryOp::Or,
                    right: Box::new(right),
                })
            }
            Cond::Not(cond) => Expr::Not(Box::new(Self::cond_to_expr(
                cond,
                preds,
                entity_param_name,
            )?)),
            Cond::Predicate(id) => {
                let predicate = preds.get(*id);
                Self::predicate_to_expr(predicate, entity_param_name)?
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

    fn predicate_to_expr(pred: &Predicate, entity_param_name: &str) -> Result<Expr> {
        let val = match pred {
            Predicate::Bin { op, lhs, rhs } => {
                let left = Box::new(Self::predicate_to_expr(lhs, entity_param_name)?);
                let right = Box::new(Self::predicate_to_expr(rhs, entity_param_name)?);
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
            Predicate::Var(var) => match var {
                Var::Ident(id) => bail!("unknow variable: `{id}`"),
                Var::Member(obj, prop) => {
                    match &**obj {
                        // at this point, the only unresolved variables should be our entities, and we
                        // have statically verified that the correct fields are being accessed.
                        Var::Ident(n) if n == entity_param_name => {
                            let property_chain = Expr::Parameter { position: 0 };
                            Expr::Property(PropertyAccess {
                                property: prop.to_string(),
                                object: Box::new(property_chain),
                            })
                        } //make_property_chain()?,
                        other => bail!("unknow variable: `{other:?}`"),
                    }
                }
            },
        };

        Ok(val)
    }
}

impl PolicyEngine {
    pub fn new(store: Store) -> Self {
        Self {
            eval_context: Default::default(),
            store,
        }
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Create an evaluation environment instance with the given parameters
    pub fn instantiate(
        &self,
        ty_name: &str,
        version: &str,
        ctx: &ChiselRequestContext,
    ) -> Result<Option<PolicyEvalInstance>> {
        match self.store.get_policy(version, ty_name) {
            Some(policy) => match &policy.policies.read {
                Some(read_policy) => {
                    let instance =
                        PolicyEvalInstance::new(ctx, read_policy, self.eval_context.clone())?;
                    Ok(Some(instance))
                }
                None => Ok(None),
            },
            None => Ok(None),
        }
    }
}
