use boa_engine::JsValue;
use chiselc::policies::{Environment, LogicOp, Predicate, Var, VarId};
use serde_json::Value as JsonValue;

use super::utils::json_to_js_value;

pub trait VarResolver {
    fn resolve(&self, env: &Environment, var: &Var) -> Option<JsonValue>;
}

#[derive(Debug)]
pub struct JsonResolver<'a> {
    pub name: &'a str,
    pub value: &'a JsonValue,
}

// TODO: this could be better: we don't *really* need to convert to JSON first...
impl JsonResolver<'_> {
    fn get_value(&self, env: &Environment, var: &Var) -> Option<&JsonValue> {
        match var {
            Var::Ident(ref s) if self.name == s => Some(self.value),
            Var::Member(obj, prop) => {
                let obj = env.get(*obj);
                let value = self.get_value(env, obj)?;
                // return null so we don't mistake that for an impossiblity to evaluate:
                // we could go down the property chain, but then found nothing.
                value.get(prop).or(Some(&JsonValue::Null))
            }
            _ => None,
        }
    }
}

impl VarResolver for JsonResolver<'_> {
    fn resolve(&self, env: &Environment, var: &Var) -> Option<JsonValue> {
        self.get_value(env, var).cloned()
    }
}

pub struct InterpreterContext<'a> {
    pub env: &'a Environment,
    pub resolver: &'a dyn VarResolver,
    pub boa: &'a mut boa_engine::Context,
}

impl InterpreterContext<'_> {
    fn var_to_value(&self, id: VarId) -> Option<JsonValue> {
        let var = self.env.get(id);
        self.resolver.resolve(self.env, var)
    }
}

pub fn eval(predicate: &Predicate, ctx: &mut InterpreterContext) -> Predicate {
    match predicate {
        Predicate::Bin { op, lhs, rhs } => {
            let lhs = eval(lhs, ctx);
            let rhs = eval(rhs, ctx);
            let lhs_val = maybe_js_value(ctx.boa, &lhs);
            let rhs_val = maybe_js_value(ctx.boa, &rhs);
            match lhs_val.zip(rhs_val) {
                Some((lhs, rhs)) => eval_bin_lit(ctx.boa, *op, &lhs, &rhs),
                None => Predicate::Bin {
                    op: *op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            }
        }
        Predicate::Not(p) => {
            let p_eval = eval(p, ctx);
            match maybe_js_value(ctx.boa, &p_eval) {
                Some(value) => Predicate::Lit(JsonValue::Bool(value.not(ctx.boa).unwrap())),
                None => Predicate::Not(Box::new(p_eval)),
            }
        }
        Predicate::Var(i) => match ctx.var_to_value(*i) {
            Some(value) => Predicate::Lit(value),
            None => predicate.clone(),
        },
        _ => predicate.clone(),
    }
}

fn eval_bin_lit(
    boa: &mut boa_engine::Context,
    op: LogicOp,
    lhs: &JsValue,
    rhs: &JsValue,
) -> Predicate {
    let value = match op {
        LogicOp::Eq => JsonValue::Bool(lhs.equals(rhs, boa).unwrap()),
        LogicOp::Neq => JsonValue::Bool(!lhs.equals(rhs, boa).unwrap()),
        LogicOp::Gt => JsonValue::Bool(lhs.gt(rhs, boa).unwrap()),
        LogicOp::Gte => JsonValue::Bool(lhs.ge(rhs, boa).unwrap()),
        LogicOp::Lt => JsonValue::Bool(lhs.lt(rhs, boa).unwrap()),
        LogicOp::Lte => JsonValue::Bool(lhs.le(rhs, boa).unwrap()),
        LogicOp::And => JsonValue::Bool(lhs.as_boolean().unwrap() && rhs.as_boolean().unwrap()),
        LogicOp::Or => JsonValue::Bool(lhs.as_boolean().unwrap() || rhs.as_boolean().unwrap()),
    };

    Predicate::Lit(value)
}

pub fn maybe_js_value(boa: &mut boa_engine::Context, predicate: &Predicate) -> Option<JsValue> {
    match predicate {
        Predicate::Lit(val) => Some(json_to_js_value(boa, val)),
        _ => None,
    }
}
