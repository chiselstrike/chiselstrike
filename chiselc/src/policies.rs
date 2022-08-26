use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};

use boa::{Context, JsString, JsValue};
use quine_mc_cluskey::Bool;
use swc_ecmascript::ast::{
    BinaryOp, Expr, Lit, MemberProp, Module, ModuleDecl, ModuleItem, Prop, PropName, PropOrSpread,
    Stmt, UnaryOp,
};

use crate::tools::analysis::control_flow::Idx;
use crate::tools::analysis::d_ir::{EnrichedRegion, EnrichedRegionInner};
use crate::tools::analysis::stmt_map::StmtMap;
use crate::tools::functions::ArrowFunction;

#[derive(Debug)]
pub struct Policies {
    pub read: Option<Policy>,
    pub write: Option<Policy>,
}

impl Policies {
    pub fn parse(module: &Module) -> Self {
        let mut read = None;
        let mut write = None;

        match &module.body[0] {
            ModuleItem::ModuleDecl(m) => match m {
                ModuleDecl::ExportDefaultExpr(e) => match &*e.expr {
                    Expr::Object(o) => {
                        for prop in &o.props {
                            match prop {
                                PropOrSpread::Prop(prop) => match &**prop {
                                    Prop::KeyValue(kv) => {
                                        let body = match &*kv.value {
                                            Expr::Arrow(arrow) => {
                                                let arrow_func = ArrowFunction::parse(arrow);
                                                Policy::from_arrow(&arrow_func)
                                            }
                                            _ => todo!(),
                                        };

                                        match &kv.key {
                                            PropName::Ident(id) => match &*id.sym {
                                                "read" => read.replace(body),
                                                "write" => write.replace(body),
                                                _ => todo!("unexpected policy rule"),
                                            },
                                            _ => todo!(),
                                        };
                                    }
                                    _ => todo!(),
                                },
                                _ => todo!(),
                            }
                        }
                    }
                    _ => todo!(),
                },
                _ => todo!(),
            },
            ModuleItem::Stmt(_) => todo!(),
        };

        Self { read, write }
    }
}

#[derive(Debug)]
pub struct Policy {
    pub actions: Actions,
    pub where_conds: Cond,
    pub predicates: Predicates,
}

impl Policy {
    fn from_arrow(arrow: &ArrowFunction) -> Self {
        let mut builder = RulesBuilder::new(&arrow.stmt_map);
        let actions = builder.infer_rules_from_region(&arrow.d_ir.region, Cond::True);
        let predicates = builder.predicates;
        let actions = actions.simplify(&predicates);
        let where_conds = generate_where_from_rules(&actions).unwrap();
        let where_conds = where_conds.simplify(&predicates);

        Self {
            actions,
            where_conds,
            predicates,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LogicOp {
    /// ==
    Eq,
    ///
    Neq,
    /// >
    Gt,
    /// >=
    Gte,
    /// <
    Lt,
    /// <=
    Lte,
    /// &&
    And,
    /// ||
    Or,
}

#[derive(Debug, Clone)]
pub enum Cond {
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Not(Box<Self>),
    /// Predicate identified by an id
    Predicate(usize),
    True,
    False,
}

impl Cond {
    fn simplify(&self, preds: &Predicates) -> Self {
        // FIXME: if there are too many predicates, we might need to use another algorithm.

        let mut mapping = Vec::new();
        let b = self.to_bool(preds, &mut mapping);
        // FIXME: why exactly is this method returning a vec?
        let simp = &dbg!(b.simplify())[0];

        Self::from_bool(simp, &mapping)
    }

    /// Transforms to a Bool expression, and attempts to evaluate predicates. Returns the Bool
    /// expression and the mapping from the Bool terms indexes to the predicateId
    fn to_bool(&self, preds: &Predicates, mapping: &mut Vec<usize>) -> Bool {
        match self {
            Cond::And(lhs, rhs) => Bool::And(vec![
                lhs.to_bool(preds, mapping),
                rhs.to_bool(preds, mapping),
            ]),
            Cond::Or(lhs, rhs) => Bool::Or(vec![
                lhs.to_bool(preds, mapping),
                rhs.to_bool(preds, mapping),
            ]),
            Cond::Not(c) => Bool::Not(Box::new(c.to_bool(preds, mapping))),
            Cond::Predicate(id) => {
                let predicate = preds.get(*id);
                match predicate {
                    Predicate::Lit(l) if l.to_boolean() => Bool::True,
                    Predicate::Lit(_) => Bool::False,
                    _ => {
                        let id = match mapping.iter().position(|i| i == id) {
                            Some(id) => id as u8,
                            None => {
                                mapping.push(*id);
                                mapping.len() as u8 - 1
                            }
                        };

                        Bool::Term(id)
                    }
                }
            }
            Cond::True => Bool::True,
            Cond::False => Bool::False,
        }
    }

    fn from_bool(b: &Bool, mapping: &Vec<usize>) -> Self {
        match b {
            Bool::True => Cond::True,
            Bool::False => Cond::False,
            Bool::Term(i) => Cond::Predicate(mapping[*i as usize]),
            Bool::And(it) => Cond::And(
                Box::new(Self::from_bool(&it[0], mapping)),
                Box::new(Self::from_bool(&it[1], mapping)),
            ),
            Bool::Or(it) => Cond::Or(
                Box::new(Self::from_bool(&it[0], mapping)),
                Box::new(Self::from_bool(&it[1], mapping)),
            ),
            Bool::Not(b) => Cond::Not(Box::new(Self::from_bool(b, mapping))),
        }
    }
}

#[derive(Clone, Debug)]
enum Predicate {
    Bin {
        op: LogicOp,
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    Not(Box<Self>),
    Lit(JsValue),
    Var(Var),
}

#[derive(Clone, Debug)]
enum Var {
    Ident(String),
    Member(Box<Var>, String),
}

#[derive(Debug, Default)]
pub struct Values {
    values: serde_json::Map<String, serde_json::Value>,
}

impl Values {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_json(values: serde_json::Map<String, serde_json::Value>) -> Self {
        Self { values }
    }

    fn get(&self, var: &Var) -> Option<&serde_json::Value> {
        match var {
            Var::Ident(val) => self.values.get(val),
            Var::Member(var, val) => self.get(var).and_then(|o| o.get(val)),
        }
    }
}

fn json_js_value(json: &serde_json::Value) -> JsValue {
    match json {
        serde_json::Value::Null => JsValue::Null,
        serde_json::Value::Bool(b) => JsValue::Boolean(*b),
        serde_json::Value::Number(n) if n.is_u64() => JsValue::Integer(n.as_u64().unwrap() as i32),
        serde_json::Value::Number(n) if n.is_i64() => JsValue::Integer(n.as_i64().unwrap() as i32),
        serde_json::Value::Number(n) if n.is_f64() => JsValue::Rational(n.as_f64().unwrap() as f64),
        serde_json::Value::String(s) => JsValue::String(JsString::new(s)),
        serde_json::Value::Array(_) => todo!(),
        serde_json::Value::Object(_) => todo!(),
        _ => unreachable!(),
    }
}

impl Predicate {
    fn is_lit(&self) -> bool {
        match self {
            Self::Lit(_) => true,
            _ => false,
        }
    }

    fn is_var(&self) -> bool {
        match self {
            Self::Var(_) => true,
            _ => false,
        }
    }

    fn as_lit(&self) -> Option<&JsValue> {
        match self {
            Self::Lit(ref l) => Some(l),
            _ => None,
        }
    }

    fn eval_bin_lit(op: LogicOp, lhs: &JsValue, rhs: &JsValue, ctx: &mut Context) -> Self {
        match op {
            LogicOp::Eq => Predicate::Lit(JsValue::Boolean(lhs == rhs)),
            LogicOp::Neq => Predicate::Lit(JsValue::Boolean(lhs != rhs)),
            LogicOp::Gt => Predicate::Lit(JsValue::Boolean(lhs.gt(rhs, ctx).unwrap())),
            LogicOp::Gte => Predicate::Lit(JsValue::Boolean(lhs.ge(rhs, ctx).unwrap())),
            LogicOp::Lt => Predicate::Lit(JsValue::Boolean(lhs.lt(rhs, ctx).unwrap())),
            LogicOp::Lte => Predicate::Lit(JsValue::Boolean(lhs.le(rhs, ctx).unwrap())),
            LogicOp::And => todo!(),
            LogicOp::Or => todo!(),
        }
    }

    pub fn eval(&self, ctx: &mut Context) -> Self {
        match self {
            Predicate::Bin { op, lhs, rhs } if lhs.is_lit() && rhs.is_lit() => {
                Self::eval_bin_lit(*op, lhs.as_lit().unwrap(), rhs.as_lit().unwrap(), ctx)
            }
            Predicate::Bin { lhs, rhs, .. } if lhs.is_var() || rhs.is_var() => self.clone(),
            Predicate::Bin { op, lhs, rhs } => {
                let new_pred = Predicate::Bin {
                    op: *op,
                    lhs: Box::new(lhs.eval(ctx)),
                    rhs: Box::new(rhs.eval(ctx)),
                };

                new_pred.eval(ctx)
            }
            Predicate::Not(p) if p.is_lit() => {
                Predicate::Lit(p.as_lit().unwrap().as_boolean().unwrap().into())
            }
            Predicate::Not(p) if !p.is_var() => {
                let p_eval = p.eval(ctx);
                match p_eval {
                    Predicate::Lit(l) => Predicate::Lit(JsValue::Boolean(l.not(ctx).unwrap())),
                    _ => Predicate::Not(Box::new(p_eval)),
                }
            }
            _ => self.clone(),
        }
    }

    fn substitute(&self, subs: &Values) -> Self {
        match self {
            Predicate::Bin { op, lhs, rhs } => Self::Bin {
                op: *op,
                lhs: Box::new(lhs.substitute(subs)),
                rhs: Box::new(rhs.substitute(subs)),
            },
            Predicate::Not(p) => Self::Not(Box::new(p.substitute(subs))),
            Predicate::Lit(_) => self.clone(),
            Predicate::Var(ref val) => match subs.get(val) {
                Some(val) => Self::Lit(json_js_value(val)),
                None => self.clone(),
            },
        }
    }

    fn parse_expr(expr: &Expr) -> Self {
        match expr {
            Expr::Unary(u) => match u.op {
                UnaryOp::Bang => Self::Not(Box::new(Self::parse_expr(&u.arg))),
                _ => panic!("unsupported op: {}", u.op),
            },
            Expr::Bin(bin) => {
                let op = match bin.op {
                    BinaryOp::EqEq => LogicOp::Eq,
                    BinaryOp::NotEq => LogicOp::Neq,
                    BinaryOp::Lt => LogicOp::Lt,
                    BinaryOp::LtEq => LogicOp::Lte,
                    BinaryOp::Gt => LogicOp::Gt,
                    BinaryOp::GtEq => LogicOp::Gte,
                    BinaryOp::LogicalOr => LogicOp::Or,
                    BinaryOp::LogicalAnd => LogicOp::And,
                    _ => panic!("unssuported binary operator {}", bin.op),
                };
                Self::Bin {
                    op,
                    lhs: Box::new(Self::parse_expr(&bin.left)),
                    rhs: Box::new(Self::parse_expr(&bin.right)),
                }
            }
            Expr::Lit(lit) => match lit {
                Lit::Str(s) => Self::Lit(JsValue::String(JsString::new(&s.value))),
                Lit::Bool(b) => Self::Lit(JsValue::Boolean(b.value)),
                Lit::Null(_) => Self::Lit(JsValue::Null),
                Lit::Num(n) => Self::Lit(JsValue::Rational(n.value)),
                Lit::BigInt(_) => todo!(),
                Lit::Regex(_) => todo!(),
                Lit::JSXText(_) => todo!(),
            },
            Expr::Ident(s) => Self::Var(Var::Ident(s.sym.to_string())),
            Expr::Paren(_) => todo!(),
            Expr::Member(m) => match Self::parse_expr(&m.obj) {
                Predicate::Var(v) => match &m.prop {
                    MemberProp::Ident(id) => {
                        Self::Var(Var::Member(Box::new(v), id.sym.to_string()))
                    }
                    _ => panic!("invalid member expression"),
                },
                _ => panic!("invalid member expression"),
            },
            _ => panic!("unsupported expr"),
        }
    }
}

#[derive(Clone, Default)]
pub struct Actions {
    actions: HashMap<Action, Cond>,
}

impl fmt::Debug for Actions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (p, rule) in self.actions.iter() {
            writeln!(f, "{p:?} => {rule:?}")?;
        }

        Ok(())
    }
}

type PredicateId = usize;

#[derive(Default)]
pub struct Predicates(Vec<Predicate>);

impl fmt::Debug for Predicates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.0.iter().enumerate() {
            writeln!(f, "{i} => {p:?}")?;
        }

        Ok(())
    }
}

impl Predicates {
    fn new() -> Self {
        Self::default()
    }

    fn insert(&mut self, predicate: Predicate) -> PredicateId {
        let id = self.0.len();
        self.0.push(predicate);
        id
    }

    fn get(&self, id: PredicateId) -> &Predicate {
        self.0.get(id).expect("invalid predicate id!")
    }

    /// Applies substitutions to all predicates
    pub fn substitute(&self, subs: &Values) -> Self {
        let substitued = self
            .0
            .iter()
            .map(|s| s.substitute(subs))
            .collect::<Vec<_>>();
        Self(substitued)
    }

    pub fn eval(&self, ctx: &mut Context) -> Self {
        Self(self.0.iter().map(|p| p.eval(ctx)).collect())
    }
}

impl Deref for Actions {
    type Target = HashMap<Action, Cond>;

    fn deref(&self) -> &Self::Target {
        &self.actions
    }
}

impl DerefMut for Actions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.actions
    }
}

impl Actions {
    fn new() -> Self {
        Self::default()
    }

    fn merge(&self, other: &Self) -> Self {
        let mut out = Actions::default();
        for policy in ACTIONS {
            match (self.get(policy), other.get(policy)) {
                (Some(lhs), Some(rhs)) => {
                    let cond = Cond::Or(Box::new(lhs.clone()), Box::new(rhs.clone()));
                    out.insert(*policy, cond);
                }
                (Some(cond), _) | (_, Some(cond)) => {
                    out.insert(*policy, cond.clone());
                }
                _ => (),
            }
        }

        out
    }

    fn simplify(self, preds: &Predicates) -> Self {
        Self {
            actions: self
                .actions
                .into_iter()
                .map(|(a, c)| (a, c.simplify(preds)))
                .collect(),
        }
    }
}

struct RulesBuilder<'a> {
    stmt_map: &'a StmtMap<'a>,
    predicates: Predicates,
}

#[derive(PartialEq, Debug, Eq, Hash, Clone, Copy)]
pub enum Action {
    Allow,
    Log,
    Deny,
    Skip,
}

const ACTIONS: &[Action] = &[Action::Allow, Action::Skip, Action::Deny, Action::Log];

impl<'a> RulesBuilder<'a> {
    fn new(stmt_map: &'a StmtMap<'a>) -> Self {
        Self {
            stmt_map,
            predicates: Predicates::new(),
        }
    }

    fn extract_cond_from_test(&mut self, region: &EnrichedRegion) -> Cond {
        match &*region.inner {
            EnrichedRegionInner::Basic(stmts) => {
                assert_eq!(
                    stmts.len(),
                    1,
                    "test region should contain a unique expression statement"
                );

                match self.stmt_map[stmts[0]].stmt {
                    Stmt::If(stmt) => {
                        let predicate = Predicate::parse_expr(&stmt.test);
                        let id = self.predicates.insert(predicate);
                        Cond::Predicate(id)
                    }
                    _ => unreachable!("expected if statement"),
                }
            }
            _ => unreachable!(),
        }
    }

    fn infer_rules_from_region(&mut self, region: &EnrichedRegion, cond: Cond) -> Actions {
        match &*region.inner {
            EnrichedRegionInner::Cond { test, cons, alt } => {
                let test_cond = Box::new(self.extract_cond_from_test(test));

                let cons_cond = Cond::And(test_cond.clone(), Box::new(cond.clone()));
                let cons_rules = self.infer_rules_from_region(cons, cons_cond);

                let alt_cond = Cond::And(
                    Box::new(Cond::Not(test_cond.clone())),
                    Box::new(cond.clone()),
                );
                let alt_rules = self.infer_rules_from_region(alt, alt_cond);

                cons_rules.merge(&alt_rules)
            }
            EnrichedRegionInner::Seq { .. } => {
                todo!()
            }
            EnrichedRegionInner::Basic(b) => self.infer_basic_block(b, cond),
        }
    }

    fn infer_basic_block(&mut self, b: &[Idx], cond: Cond) -> Actions {
        let mut rules = Actions::new();

        if b.is_empty() {
            rules.insert(Action::Deny, cond);
        } else if b.len() == 1 {
            match self.stmt_map[b[0]].stmt {
                Stmt::Return(ret) => match &ret.arg {
                    Some(arg) => match &**arg {
                        Expr::Member(m) => {
                            match &*m.obj {
                                Expr::Ident(id) if &*id.sym == "Action" => (),
                                _ => panic!("invalid return expression"),
                            };

                            match &m.prop {
                                MemberProp::Ident(id) => {
                                    let policy = match &*id.sym {
                                        "Allow" => Action::Allow,
                                        "Skip" => Action::Skip,
                                        "Deny" => Action::Deny,
                                        "Log" => Action::Log,
                                        _ => panic!("invalid return expression"),
                                    };

                                    rules.insert(policy, cond);
                                }
                                _ => panic!("invalid return expression"),
                            }
                        }
                        _ => panic!("invalid return expression"),
                    },
                    None => panic!("missing return arguments!"),
                },
                _ => panic!("expected return statement"),
            }
        } else {
            panic!("unsupported multiline basic block")
        }

        rules
    }
}

fn generate_where_from_rules(actions: &Actions) -> Option<Cond> {
    actions
        .get(&Action::Skip)
        .cloned()
        .map(|c| Cond::Not(Box::new(c)))
}
