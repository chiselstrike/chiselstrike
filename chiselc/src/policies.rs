// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use quine_mc_cluskey::Bool;
use serde_json::Value;
use swc_common::sync::Lrc;
use swc_common::SourceMap;
use swc_ecmascript::ast::{
    ArrowExpr, BinaryOp, Expr, ExprStmt, Ident, Lit, MemberProp, Module, ModuleDecl, ModuleItem,
    Prop, PropName, PropOrSpread, Stmt, UnaryOp,
};

use crate::parse::{emit, ParserContext};
use crate::rewrite::Target;
use crate::tools::analysis::control_flow::Idx;
use crate::tools::analysis::region::Region;
use crate::tools::analysis::stmt_map::StmtMap;
use crate::tools::functions::ArrowFunction;

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PolicyName {
    Read,
    Create,
    Update,
    OnRead,
    OnCreate,
    OnUpdate,
    GeoLoc,
}

#[derive(Debug, Clone)]
pub enum Policy {
    Filter(FilterPolicy),
    Transform(TransformPolicy),
}

impl Policy {
    pub fn as_filter(&self) -> Option<&FilterPolicy> {
        match self {
            Policy::Filter(ref f) => Some(f),
            Policy::Transform(_) => None,
        }
    }

    pub fn as_transform(&self) -> Option<&TransformPolicy> {
        match self {
            Policy::Transform(ref t) => Some(t),
            Policy::Filter(_) => None,
        }
    }

    pub fn code(&self) -> &[u8] {
        match self {
            Policy::Filter(ref f) => &f.js_code,
            Policy::Transform(ref t) => &t.js_code,
        }
    }
}

impl From<FilterPolicy> for Policy {
    fn from(p: FilterPolicy) -> Self {
        Self::Filter(p)
    }
}

impl From<TransformPolicy> for Policy {
    fn from(p: TransformPolicy) -> Self {
        Self::Transform(p)
    }
}

impl FromStr for PolicyName {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read" => Ok(Self::Read),
            "create" => Ok(Self::Create),
            "update" => Ok(Self::Update),
            "onRead" => Ok(Self::OnRead),
            "onCreate" => Ok(Self::OnCreate),
            "onUpdate" => Ok(Self::OnUpdate),
            "geoLoc" => Ok(Self::GeoLoc),
            other => bail!("unknown policy `{other}`"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Policies {
    policies: HashMap<PolicyName, Policy>,
}

impl Policies {
    pub fn parse_code(code: &[u8]) -> Result<Self> {
        let ctx = ParserContext::new();
        let module = ctx.parse(
            std::str::from_utf8(code)
                .context("the provided code is not valid UTF-8")?
                .to_owned(),
            false,
        )?;
        Self::parse(&module, ctx.sm)
    }

    fn parse(module: &Module, sm: Lrc<SourceMap>) -> Result<Self> {
        let mut policies = HashMap::new();

        for module in &module.body {
            match module {
                ModuleItem::ModuleDecl(m) => match m {
                    ModuleDecl::ExportDefaultExpr(e) => match &*e.expr {
                        Expr::Object(o) => {
                            for prop in &o.props {
                                match prop {
                                    PropOrSpread::Prop(prop) => match &**prop {
                                        Prop::KeyValue(kv) => {
                                            let policy_name = match &kv.key {
                                                PropName::Ident(id) => match id.sym.parse() {
                                                    Ok(name) => name,
                                                    // unknown rule, just ignore it
                                                    Err(_) => continue,
                                                },
                                                _ => bail!("expected property!"),
                                            };

                                            let body = match policy_name {
                                                PolicyName::Read
                                                | PolicyName::Create
                                                | PolicyName::Update => match &*kv.value {
                                                    Expr::Arrow(arrow) => {
                                                        let arrow_func =
                                                            ArrowFunction::parse(arrow)?;
                                                        FilterPolicy::from_arrow(&arrow_func, sm.clone())?.into()
                                                    }
                                                    _ => bail!("Only arrow functions are supported in policies"),
                                                },
                                                PolicyName::OnRead | PolicyName::OnCreate | PolicyName::OnUpdate | PolicyName::GeoLoc => {
                                                    match &*kv.value {
                                                        Expr::Arrow(arrow) => {
                                                            TransformPolicy::from_arrow(arrow, sm.clone())?
                                                                .into()
                                                        },
                                                        _ => bail!("Only arrow functions are supported in policies"),
                                                    }
                                                }
                                            };

                                            policies.insert(policy_name, body);
                                        }
                                        _ => bail!("unexpexted property in policy object"),
                                    },
                                    _ => bail!("expexted property"),
                                }
                            }
                        }
                        _ => bail!("default export for policies should be an object."),
                    },
                    // ignore everything else
                    _ => continue,
                },
                ModuleItem::Stmt(_) => continue,
            };
        }

        Ok(Self { policies })
    }

    pub fn iter(&self) -> impl Iterator<Item = (&PolicyName, &Policy)> {
        self.policies.iter()
    }
}

#[derive(Debug, Clone)]
pub struct PolicyParams {
    names: Vec<String>,
}

impl PolicyParams {
    pub fn get_positional_param_name(&self, pos: usize) -> &str {
        &self.names[pos]
    }

    fn from_idents(param_names: &[&Ident]) -> Self {
        let mut names = Vec::new();
        for name in param_names {
            names.push(name.sym.to_string());
        }

        Self { names }
    }
}

#[derive(Debug, Clone)]
pub struct FilterPolicy {
    pub where_conds: Option<Cond>,
    pub predicates: Predicates,
    pub env: Arc<Environment>,
    pub js_code: Box<[u8]>,
    params: PolicyParams,
}

impl FilterPolicy {
    fn from_arrow(arrow: &ArrowFunction, sm: Lrc<SourceMap>) -> Result<Self> {
        let params: Vec<_> = arrow.params().map(|(name, _)| name).collect();
        let params = PolicyParams::from_idents(&params);

        let mut builder = RulesBuilder::new(&arrow.stmt_map);
        let actions = builder.infer_rules_from_region(&arrow.regions, Cond::True)?;
        let predicates = builder.predicates;
        let actions = Arc::new(actions.simplify(&predicates));
        let where_conds = generate_where_from_rules(&actions).map(|c| c.simplify(&predicates));
        let env = Arc::new(builder.env);
        let js_code = emit_arrow_js_code(arrow.orig, sm)?;

        Ok(Self {
            where_conds,
            predicates,
            params,
            env,
            js_code,
        })
    }

    pub fn params(&self) -> &PolicyParams {
        &self.params
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LogicOp {
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
    pub fn simplify(&self, preds: &Predicates) -> Self {
        // FIXME: if there are too many predicates, we might need to use another algorithm.

        let mut mapping = Vec::new();
        let b = self.to_bool(preds, &mut mapping);
        // FIXME: why exactly is this method returning a vec?
        let simp = &b.simplify()[0];

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
                    Predicate::Lit(Value::Bool(true)) => Bool::True,
                    Predicate::Lit(Value::Bool(false)) => Bool::False,
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Var {
    Ident(String),
    Member(usize, String),
}

pub type VarId = usize;

#[derive(Default, Clone, Debug)]
pub struct Environment {
    vars: bimap::BiMap<Var, VarId>,
}

impl Environment {
    fn insert(&mut self, var: Var) -> VarId {
        match self.vars.get_by_left(&var) {
            Some(idx) => *idx,
            None => {
                let idx = self.vars.len();
                self.vars.insert(var, idx);
                idx
            }
        }
    }

    pub fn get(&self, id: VarId) -> &Var {
        self.vars.get_by_right(&id).unwrap()
    }
}

#[derive(Clone, Debug)]
pub enum Predicate {
    Bin {
        op: LogicOp,
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    Not(Box<Self>),
    Lit(Value),
    Var(VarId),
}

impl Predicate {
    pub fn is_lit(&self) -> bool {
        matches!(self, Self::Lit(_))
    }

    pub fn as_lit(&self) -> Option<&Value> {
        match self {
            Self::Lit(ref l) => Some(l),
            _ => None,
        }
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Self::Lit(_))
    }

    pub fn as_var(&self) -> Option<VarId> {
        match self {
            Self::Var(id) => Some(*id),
            _ => None,
        }
    }

    pub fn is_reducible(&self) -> bool {
        match self {
            Predicate::Bin { lhs, rhs, .. } => lhs.is_reducible() && rhs.is_reducible(),
            Predicate::Not(inner) => inner.is_reducible(),
            Predicate::Lit(_) => true,
            Predicate::Var(_) => false,
        }
    }

    fn parse_expr(expr: &Expr, env: &mut Environment) -> Self {
        match expr {
            Expr::Unary(u) => match u.op {
                UnaryOp::Bang => Self::Not(Box::new(Self::parse_expr(&u.arg, env))),
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
                    lhs: Box::new(Self::parse_expr(&bin.left, env)),
                    rhs: Box::new(Self::parse_expr(&bin.right, env)),
                }
            }
            Expr::Lit(lit) => match lit {
                Lit::Str(s) => Self::Lit((*s.value).into()),
                Lit::Bool(b) => Self::Lit(b.value.into()),
                Lit::Null(_) => Self::Lit(Value::Null),
                Lit::Num(n) => Self::Lit(n.value.into()),
                Lit::BigInt(_) => todo!(),
                Lit::Regex(_) => todo!(),
                Lit::JSXText(_) => todo!(),
            },
            Expr::Ident(s) => {
                let var = Var::Ident(s.sym.to_string());
                Self::Var(env.insert(var))
            }
            Expr::Paren(e) => Self::parse_expr(&e.expr, env),
            Expr::Member(m) => match Self::parse_expr(&m.obj, env) {
                Predicate::Var(v) => match &m.prop {
                    MemberProp::Ident(id) => {
                        let var = Var::Member(v, id.sym.to_string());
                        Self::Var(env.insert(var))
                    }
                    MemberProp::Computed(comp) => {
                        let prop = match &*comp.expr {
                            Expr::Lit(Lit::Str(prop)) => prop,
                            _ => panic!("unsupported computed property expression."),
                        };
                        let var = Var::Member(v, prop.value.to_string());
                        Self::Var(env.insert(var))
                    }
                    _ => panic!("invalid member expression"),
                },
                _ => panic!("invalid member expression"),
            },
            _ => panic!("unsupported expr"),
        }
    }
}

#[derive(Default, Clone)]
pub struct Actions {
    actions: HashMap<Action, Cond>,
}

impl Actions {
    pub fn iter(&self) -> impl Iterator<Item = (&Action, &Cond)> {
        self.actions.iter()
    }
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

#[derive(Clone, Default)]
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

    pub fn get(&self, id: PredicateId) -> &Predicate {
        self.0.get(id).expect("invalid predicate id!")
    }

    pub fn map(&self, f: impl FnMut(&Predicate) -> Predicate) -> Self {
        Self(self.0.iter().map(f).collect())
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
    env: Environment,
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
            env: Environment::default(),
        }
    }

    fn extract_cond_from_test(&mut self, region: &Region) -> Cond {
        match &region {
            Region::BasicBlock(stmts) => {
                assert_eq!(
                    stmts.len(),
                    1,
                    "test region should contain a unique expression statement"
                );

                match self.stmt_map[stmts[0]].stmt {
                    Stmt::If(stmt) => {
                        let predicate = Predicate::parse_expr(&stmt.test, &mut self.env);
                        let id = self.predicates.insert(predicate);
                        Cond::Predicate(id)
                    }
                    _ => unreachable!("expected if statement"),
                }
            }
            _ => unreachable!(),
        }
    }

    fn infer_rules_from_region(&mut self, region: &Region, cond: Cond) -> anyhow::Result<Actions> {
        let action = match region {
            Region::Cond(region) => {
                let test_cond = Box::new(self.extract_cond_from_test(&region.test_region));

                let cons_cond = Cond::And(test_cond.clone(), Box::new(cond.clone()));
                let cons_rules = self.infer_rules_from_region(&region.cons_region, cons_cond)?;

                let alt_cond = Cond::And(Box::new(Cond::Not(test_cond)), Box::new(cond));
                let alt_rules = self.infer_rules_from_region(&region.alt_region, alt_cond)?;

                cons_rules.merge(&alt_rules)
            }
            Region::Seq { .. } => {
                todo!()
            }
            Region::BasicBlock(b) => self.infer_basic_block(b, cond),
            Region::Loop(_) => bail!("loops are not supported in the filter rules."),
        };

        Ok(action)
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

#[derive(Debug, Clone)]
pub struct TransformPolicy {
    pub js_code: Box<[u8]>,
}

impl TransformPolicy {
    fn from_arrow(arrow: &ArrowExpr, sm: Lrc<SourceMap>) -> Result<Self> {
        let js_code = emit_arrow_js_code(arrow, sm)?;
        Ok(Self { js_code })
    }
}

fn emit_arrow_js_code(arrow: &ArrowExpr, sm: Lrc<SourceMap>) -> Result<Box<[u8]>> {
    let module = Module {
        body: vec![ModuleItem::Stmt(Stmt::Expr(ExprStmt {
            span: arrow.span,
            expr: Box::new(Expr::Arrow(arrow.clone())),
        }))],
        span: arrow.span,
        shebang: None,
    };

    let mut js_code = Vec::new();
    emit(module, Target::JavaScript, sm, &mut js_code)?;

    Ok(js_code.into_boxed_slice())
}
