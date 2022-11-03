use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{bail, Result};
use boa_engine::prelude::JsObject;
use boa_engine::property::Attribute;
use boa_engine::{JsString, JsValue};
use chiselc::policies::{Cond, Environment, LogicOp, PolicyName, Predicate, Predicates, Var};
use serde_json::Value as JsonValue;

use super::debug::debug;
use super::interpreter::{self, InterpreterContext, JsonResolver};
use super::store::PolicyStore;
use super::type_policy::{GeoLocPolicy, ReadPolicy, TransformPolicy, TypePolicy, WritePolicy};
use super::utils::json_to_js_value;
use super::Action;
use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, PropertyAccess, Value};

pub struct PolicyEngine {
    /// The boa context, used for evaluating JS with boa.
    pub boa_ctx: Rc<RefCell<boa_engine::Context>>,
    /// The policy store, mapping entity names to type policies.
    pub policies: RefCell<PolicyStore>,
}

/// Represents the request context that is being passed as a parameter to the policies
// TODO(marin): This is a temporary trait until I figure out how this data should be passed around,
// and what shape it will have.
pub trait ChiselRequestContext {
    fn method(&self) -> &str;
    fn path(&self) -> &str;
    fn headers(&self) -> Box<dyn Iterator<Item = (&str, &str)> + '_>;
    fn user_id(&self) -> Option<&str>;
    fn token(&self) -> Option<&JsonValue>;

    // TODO: need to find a way around using json here.
    fn to_value(&self) -> JsonValue {
        serde_json::json!({
            "method": self.method(),
            "path": self.path(),
            "headers": self.headers().collect::<HashMap<_, _>>(),
            "user_id": self.user_id(),
            "token": self.token(),
        })
    }

    fn to_js_value(&self, ctx: &mut boa_engine::Context) -> JsValue {
        let map = JsObject::empty();

        map.set("method", self.method(), false, ctx).unwrap();
        map.set("path", self.path(), false, ctx).unwrap();

        let user_id = match self.user_id() {
            Some(val) => JsValue::String(JsString::from(val)),
            None => JsValue::Null,
        };
        map.set("userId", user_id, false, ctx).unwrap();

        let headers = JsObject::empty();
        for (key, val) in self.headers() {
            headers.set(key, val, false, ctx).unwrap();
        }

        map.set("headers", headers, false, ctx).unwrap();
        let token = match self.token() {
            Some(tok) => json_to_js_value(ctx, tok),
            None => JsValue::Null,
        };

        map.set("token", token, false, ctx).unwrap();

        JsValue::Object(map)
    }
}

impl PolicyEngine {
    pub fn new() -> Result<Self> {
        let mut context = boa_engine::Context::default();
        let action = Action::js_value(&mut context)?;
        context.register_global_property("Action", action, Attribute::all());
        context.register_global_function("debug", 0, debug);
        Ok(Self {
            boa_ctx: Rc::new(RefCell::new(context)),
            policies: Default::default(),
        })
    }

    /// Returns the type policy for the given entity_name.
    pub fn get_policy(&self, entity_name: &str) -> Option<Ref<TypePolicy>> {
        let store = self.policies.borrow();
        // this is a trick to get an Option<Ref<T>> from a Option<&T>
        if store.get(entity_name).is_some() {
            Some(Ref::map(store, |s| s.get(entity_name).unwrap()))
        } else {
            None
        }
    }

    // Given a entity_name, and some policy code, compiles the policy code, and store it in the
    // engine store, with entity_name as key.
    pub fn register_policy_from_code(
        &self,
        entity_name: String,
        code: &[u8],
    ) -> anyhow::Result<()> {
        let policies = chiselc::policies::Policies::parse_code(code)?;
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
                PolicyName::OnCreate => {
                    let policy = TransformPolicy::new(function);
                    type_policy.on_create.replace(policy);
                }
                PolicyName::OnUpdate => {
                    let policy = TransformPolicy::new(function);
                    type_policy.on_update.replace(policy);
                }
                PolicyName::GeoLoc => {
                    let policy = GeoLocPolicy::new(function);
                    type_policy.geoloc.replace(policy);
                }
            }
        }

        self.policies.borrow_mut().insert(entity_name, type_policy);

        Ok(())
    }

    /// Takes a ReadPolicy and ChiselRequestContext, and partially evaluate the read policy to
    /// extract its skip condition, then translate that into an Expr that the data layer can append
    /// to queries.
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

    /// Given some JS code representing a function, compiles the functions, and returns the
    /// resulting JsObject. This function can later be called.
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

    /// Takes a JsObject representing a function, and a list of arguments, and call the function
    /// with these arguments.
    pub fn call(&self, function: JsObject, args: &[JsValue]) -> anyhow::Result<JsValue> {
        let mut ctx = self.boa_ctx.borrow_mut();
        function
            .call(&JsValue::Null, args, &mut ctx)
            .map_err(|e| boa_err_to_anyhow(e, &mut ctx))
    }
}

pub fn boa_err_to_anyhow(e: JsValue, ctx: &mut boa_engine::Context) -> anyhow::Error {
    let e = e.as_object().unwrap();
    if !e.is_error() {
        return anyhow::anyhow!("unknown error");
    }

    let msg = e.get("message", ctx).unwrap();
    anyhow::anyhow!("{}", msg.as_string().unwrap().to_string())
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
            BinaryExpr::and(left, right)
        }
        Cond::Or(left, right) => {
            let right = cond_to_expr(right, preds, entity_param_name, env)?;
            let left = cond_to_expr(left, preds, entity_param_name, env)?;
            BinaryExpr::or(left, right)
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

impl From<LogicOp> for BinaryOp {
    fn from(op: LogicOp) -> Self {
        match op {
            LogicOp::Eq => BinaryOp::Eq,
            LogicOp::Neq => BinaryOp::NotEq,
            LogicOp::Gt => BinaryOp::Gt,
            LogicOp::Gte => BinaryOp::GtEq,
            LogicOp::Lt => BinaryOp::Lt,
            LogicOp::Lte => BinaryOp::LtEq,
            LogicOp::And => BinaryOp::And,
            LogicOp::Or => BinaryOp::Or,
        }
    }
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

#[cfg(test)]
mod test {
    use serde_json::Value as JsonValue;

    use super::*;

    impl ChiselRequestContext for JsonValue {
        fn method(&self) -> &str {
            self["method"].as_str().unwrap()
        }

        fn path(&self) -> &str {
            self["path"].as_str().unwrap()
        }

        fn headers(&self) -> Box<dyn Iterator<Item = (&str, &str)> + '_> {
            Box::new(
                self["headers"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str().unwrap())),
            )
        }

        fn user_id(&self) -> Option<&str> {
            self.get("userId")?.as_str()
        }

        fn token(&self) -> Option<&JsonValue> {
            None
        }
    }

    #[test]
    fn register_code_ok() {
        let code = br#"
            export default {
                read: (entity, ctx) => {
                    return Action.Allow;
                },
                create: (entity, ctx) => {
                    return Action.Allow;
                },
                update: (entity, ctx) => {
                    return Action.Allow;
                },
                onRead: (entity, ctx) => {
                    return entity;
                },
                onCreate: (entity, ctx) => {
                    return entity;
                },
                onUpdate: (entity, ctx) => {
                    return entity;
                },
                geoLoc: (entity, ctx) => {
                    return "us-east1";
                },
            }
        "#;

        let engine = PolicyEngine::new().unwrap();
        engine
            .register_policy_from_code("Person".to_string(), code)
            .unwrap();

        let policy = engine.get_policy("Person").unwrap();

        assert!(policy.read.is_some());
        assert!(policy.create.is_some());
        assert!(policy.update.is_some());
        assert!(policy.on_read.is_some());
        assert!(policy.on_create.is_some());
        assert!(policy.on_update.is_some());
        assert!(policy.geoloc.is_some());
    }

    #[test]
    fn bad_code() {
        let code = br#"
            export defult {
                read entity, ctx) = {
                    retrn Action.Allow;
                }
            }
        "#;

        let engine = PolicyEngine::new().unwrap();
        let res = engine.register_policy_from_code("Person".into(), code);

        assert!(res.is_err());
    }

    #[test]
    fn filter_all_expr() {
        let code = br#"
            export default {
                read: (person, ctx) => {
                    if (ctx.method == "GET") {
                        return Action.Skip;
                    } else {
                        return Action.Allow
                    }
                }
            }
        "#;

        let engine = PolicyEngine::new().unwrap();
        engine
            .register_policy_from_code("Person".to_string(), code)
            .unwrap();
        let policy = engine.get_policy("Person").unwrap();

        let ctx = serde_json::json! ({
            "headers": {},
            "method": "GET",
            "path": "/hello",
        });

        let expr = engine
            .eval_read_policy_expr(policy.read.as_ref().unwrap(), &ctx)
            .unwrap()
            .unwrap();

        assert_eq!(
            expr,
            Expr::Value {
                value: Value::Bool(false)
            }
        )
    }

    #[test]
    fn filter_cond_expr() {
        let code = br#"
            export default {
                read: (person, ctx) => {
                    if (person.name == "marin" && ctx.method == "GET") {
                        return Action.Skip;
                    } else {
                        return Action.Allow
                    }
                }
            }
        "#;

        let engine = PolicyEngine::new().unwrap();
        engine
            .register_policy_from_code("Person".to_string(), code)
            .unwrap();
        let policy = engine.get_policy("Person").unwrap();

        let ctx = serde_json::json!({
            "headers": {},
            "method": "GET",
            "path": "/hello",
        });

        let expr = engine
            .eval_read_policy_expr(policy.read.as_ref().unwrap(), &ctx)
            .unwrap()
            .unwrap();
        // if name == "marin", skip, in other words, take name != "marin"
        let expected = Expr::Not(
            BinaryExpr::and(
                BinaryExpr::eq(
                    Expr::Property(PropertyAccess {
                        property: "name".into(),
                        object: Expr::Parameter { position: 0 }.into(),
                    }),
                    Expr::Value {
                        value: Value::String("marin".into()),
                    },
                ),
                Expr::Value {
                    value: Value::Bool(true),
                },
            )
            .into(),
        );

        assert_eq!(expr, expected);
    }

    #[test]
    fn take_all_expr() {
        let code = br#"
            export default {
                read: (person, ctx) => {
                    if (person.name == "marin" && ctx.method == "GET") {
                        return Action.Deny;
                    } else {
                        return Action.Allow
                    }
                }
            }
        "#;

        let engine = PolicyEngine::new().unwrap();
        engine
            .register_policy_from_code("Person".to_string(), code)
            .unwrap();
        let policy = engine.get_policy("Person").unwrap();

        let ctx = serde_json::json!({
            "headers": {},
            "method": "GET",
            "path": "/hello",
        });

        let result = engine
            .eval_read_policy_expr(policy.read.as_ref().unwrap(), &ctx)
            .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn take_complex_skip() {
        // we take if (!(person.name == "marin" && ctx.method == "GET") && person.age == 42) ||
        // (!(person.name == "marin" && ctx.method == "GET") && !(person.age == 42) && !(person.name == "Jim" && person.age < 178))
        //
        // We note:
        // A = person.name == "marin" && ctx.method == "GET"
        // B = person.age == 42
        // C = person.name == "Jim" && person.age < 178
        //
        // the previous expression can be rewritten:
        // (!A && B) || (!A && !B && !C) simplifies to => (!A && !C) || (!A && B)
        let code = br#"
            export default {
                read: (person, ctx) => {
                    if (person.name == "marin" && ctx.method == "GET") {
                        return Action.Skip;
                    }

                    if (person.age == 42) {
                        return Action.Allow;
                    }

                    if (person.name == "Jim" && person.age < 178) {
                        return Action.Skip;
                    }
                }
            }
        "#;

        let engine = PolicyEngine::new().unwrap();
        engine
            .register_policy_from_code("Person".to_string(), code)
            .unwrap();
        let policy = engine.get_policy("Person").unwrap();

        let ctx = serde_json::json!({
            "headers": {},
            "method": "GET",
            "path": "/hello",
        });

        let expr = engine
            .eval_read_policy_expr(policy.read.as_ref().unwrap(), &ctx)
            .unwrap()
            .unwrap();

        // sorry for what's next... :()
        // p1 = !(name == "marin" && true) = !A
        let p1 = Expr::Not(
            BinaryExpr::and(
                BinaryExpr::eq(
                    Expr::Property(PropertyAccess {
                        property: "name".into(),
                        object: Expr::Parameter { position: 0 }.into(),
                    }),
                    Expr::Value {
                        value: Value::String("marin".into()),
                    },
                ),
                Expr::Value {
                    value: Value::Bool(true),
                },
            )
            .into(),
        );

        // p2 = !(name == Jim && age < 178) = !C
        let p2 = Expr::Not(
            BinaryExpr::and(
                BinaryExpr::eq(
                    Expr::Property(PropertyAccess {
                        property: "name".into(),
                        object: Expr::Parameter { position: 0 }.into(),
                    }),
                    Expr::Value {
                        value: Value::String("Jim".into()),
                    },
                ),
                BinaryExpr::lt(
                    Expr::Property(PropertyAccess {
                        property: "age".into(),
                        object: Expr::Parameter { position: 0 }.into(),
                    }),
                    Expr::Value {
                        value: Value::F64(178.0),
                    },
                ),
            )
            .into(),
        );

        // p3 = p1 && p2 = !A && !C
        let p3 = BinaryExpr::and(p1, p2);

        // p4 = (age == 42) && !(name == "marin" && true) = !A && B
        let p4 = BinaryExpr::and(
            BinaryExpr::eq(
                Expr::Property(PropertyAccess {
                    property: "age".into(),
                    object: Expr::Parameter { position: 0 }.into(),
                }),
                Expr::Value {
                    value: Value::F64(42.0),
                },
            ),
            Expr::Not(
                BinaryExpr::and(
                    BinaryExpr::eq(
                        Expr::Property(PropertyAccess {
                            property: "name".into(),
                            object: Expr::Parameter { position: 0 }.into(),
                        }),
                        Expr::Value {
                            value: Value::String("marin".into()),
                        },
                    ),
                    Expr::Value {
                        value: Value::Bool(true),
                    },
                )
                .into(),
            ),
        );

        // expected = p3 || p4 = (!A && !C) || (!A && B) : SUCCESS this is what we wanted!
        let expected = BinaryExpr::or(p3, p4);

        assert_eq!(expected, expr);
    }

    #[test]
    fn read_header_ctx_value_expr() {
        let code = br#"
            export default {
                read: (person, ctx) => {
                    if (ctx.headers["val"] == "foobar") {
                        return Action.Skip;
                    }
                }
            }
        "#;

        let engine = PolicyEngine::new().unwrap();
        engine
            .register_policy_from_code("Person".to_string(), code)
            .unwrap();
        let policy = engine.get_policy("Person").unwrap();

        let ctx = serde_json::json!({
            "headers": {
                "val": "foobar"
            },
            "method": "GET",
            "path": "/hello",
        });

        let expr = engine
            .eval_read_policy_expr(policy.read.as_ref().unwrap(), &ctx)
            .unwrap()
            .unwrap();

        let expected = Expr::Value {
            value: Value::Bool(false),
        };

        assert_eq!(expr, expected);
    }
}
