use std::borrow::Cow;

use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, PropertyAccess, Value};
use crate::deno::ChiselRequestContext;
use crate::types::{Type, TypeSystem};
use anyhow::{bail, Result};
use chiselc::parse::ParserContext;
use chiselc::policies::{Cond, PolicyEvalContext, Predicate, Predicates, Values, Var};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TypePolicy {
    policies: chiselc::policies::Policies,
    source: String,
}

struct PolicyTypeSystem<'a> {
    version: String,
    ts: &'a TypeSystem,
}

enum PolicyType<'a> {
    Type(&'a TypeSystem, Type),
    ReqContext,
    String,
    Object,
}

impl<'a> chiselc::policies::Type<'a> for PolicyType<'a> {
    fn get_field_ty(&self, field_name: &str) -> Option<Box<dyn chiselc::policies::Type<'a> + 'a>> {
        match self {
            PolicyType::Type(ts, ty) => match ty {
                Type::Entity(e) => {
                    let field = e.get_field(field_name)?;
                    let field_ty = ts.get(&field.type_id).unwrap();

                    Some(Box::new(PolicyType::Type(ts, field_ty)))
                }
                _ => None,
            },
            PolicyType::ReqContext => {
                // TODO: this is not robust at all! need to find a better way.
                match field_name {
                    "userId" => Some(Box::new(PolicyType::String)),
                    "method" => Some(Box::new(PolicyType::String)),
                    "path" => Some(Box::new(PolicyType::String)),
                    "apiVersion" => Some(Box::new(PolicyType::String)),
                    "headers" => Some(Box::new(PolicyType::Object)),
                    _ => None,
                }
            }
            PolicyType::String | PolicyType::Object => None,
        }
    }

    fn name(&self) -> Cow<str> {
        match self {
            PolicyType::Type(_, ty) => Cow::Owned(ty.name()),
            PolicyType::ReqContext => Cow::Borrowed("ReqContext"),
            PolicyType::String => Cow::Borrowed("string"),
            PolicyType::Object => Cow::Borrowed("object"),
        }
    }
}

impl<'a> chiselc::policies::TypeSystem for PolicyTypeSystem<'a> {
    fn get_type<'b>(&'b self, name: &str) -> Box<dyn chiselc::policies::Type<'b> + 'b> {
        if name == "ReqContext" {
            Box::new(PolicyType::ReqContext)
        } else {
            Box::new(PolicyType::Type(
                self.ts,
                self.ts.lookup_type(name, &self.version).unwrap(),
            ))
        }
    }
}

struct ReadFilterEvalCtx<'a> {
    chisel_ctx: &'a ChiselRequestContext,
    entity_param_name: &'a str,
    context_param_name: &'a str,
    predicates: &'a Predicates,
    where_conds: &'a Cond,
    eval_ctx: &'a mut PolicyEvalContext,
}

impl<'a> ReadFilterEvalCtx<'a> {
    fn eval(self) -> Result<Expr> {
        let mut values = Values::new();
        let ctx_json = serde_json::to_value(self.chisel_ctx)?;
        values.insert(self.context_param_name.into(), ctx_json);
        let predicates = self.predicates.substitute(&values);
        let predicates = predicates.eval(self.eval_ctx);
        let where_conds = self.where_conds.simplify(&predicates);

        self.cond_to_expr(&where_conds, &predicates)
    }

    fn cond_to_expr(&self, cond: &Cond, preds: &Predicates) -> Result<Expr> {
        let val = match cond {
            Cond::And(left, right) => {
                let right = self.cond_to_expr(right, preds)?;
                let left = self.cond_to_expr(left, preds)?;
                Expr::Binary(BinaryExpr {
                    left: Box::new(left),
                    op: BinaryOp::And,
                    right: Box::new(right),
                })
            }
            Cond::Or(left, right) => {
                let right = self.cond_to_expr(right, preds)?;
                let left = self.cond_to_expr(left, preds)?;
                Expr::Binary(BinaryExpr {
                    left: Box::new(left),
                    op: BinaryOp::Or,
                    right: Box::new(right),
                })
            }
            Cond::Not(cond) => Expr::Not(Box::new(self.cond_to_expr(cond, preds)?)),
            Cond::Predicate(id) => {
                let predicate = preds.get(*id);
                self.predicate_to_expr(predicate)?
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

    fn predicate_to_expr(&self, pred: &Predicate) -> Result<Expr> {
        let val = match pred {
            Predicate::Bin { op, lhs, rhs } => {
                let left = Box::new(self.predicate_to_expr(lhs)?);
                let right = Box::new(self.predicate_to_expr(rhs)?);
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
                        Var::Ident(n) if n == self.entity_param_name => {
                            let property_chain = Expr::Parameter { position: 0 };
                            Expr::Property(PropertyAccess {
                                property: prop.to_string(),
                                object: Box::new(property_chain),
                            })
                        }
                        other => bail!("unknow variable: `{other:?}`"),
                    }
                }
            },
        };

        Ok(val)
    }
}

impl TypePolicy {
    pub fn from_policy_code(code: String, ts: &TypeSystem, version: String) -> Result<Self> {
        let ctx = ParserContext::new();
        let module = ctx.parse(code.clone(), false)?;
        let ts = PolicyTypeSystem { version, ts };
        let policies = chiselc::policies::Policies::parse(&module, &ts);
        dbg!(Ok(Self {
            policies,
            source: code
        }))
    }

    pub fn compute_read_filter(
        &self,
        chisel_ctx: &ChiselRequestContext,
        eval_ctx: &mut PolicyEvalContext,
    ) -> Result<Option<Expr>> {
        match &self.policies.read {
            Some(
                p @ chiselc::policies::Policy {
                    where_conds: Some(where_conds),
                    predicates,
                    ..
                },
            ) => {
                let entity_param_name = p.params().get_positional_param_name(0);
                let context_param_name = p.params().get_positional_param_name(1);

                let ctx = ReadFilterEvalCtx {
                    chisel_ctx,
                    entity_param_name,
                    context_param_name,
                    predicates,
                    where_conds,
                    eval_ctx,
                };

                ctx.eval().map(Some)
            }
            _ => Ok(None),
        }
    }
}
