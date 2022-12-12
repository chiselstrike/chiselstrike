// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use quine_mc_cluskey::Bool;
use serde_json::Value;
use swc_common::errors::{DiagnosticId, Handler};
use swc_common::sync::Lrc;
use swc_common::{MultiSpan, SourceMap, Spanned};
use swc_ecmascript::ast::{
    ArrowExpr, BinaryOp, Expr, ExprStmt, Ident, Lit, MemberProp, Module, ModuleDecl, ModuleItem,
    Prop, PropName, PropOrSpread, Stmt, UnaryOp,
};
use url::Url;

use crate::parse::{emit, ParserContext};
use crate::rewrite::Target;
use crate::tools::analysis::control_flow::Idx;
use crate::tools::analysis::region::Region;
use crate::tools::analysis::stmt_map::StmtMap;
use crate::tools::functions::ArrowFunction;

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PolicyName {
    Create,
    GeoLoc,
    OnCreate,
    OnRead,
    OnUpdate,
    Read,
    Update,
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
            Policy::Filter(f) => f.code(),
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
    /// Checks `code` with path `file_path` and returns error/warning encountered
    pub fn check(code: &[u8], file_path: Url) -> Result<()> {
        let ctx = ParserContext::new();
        let module = ctx.parse(
            std::str::from_utf8(code)
                .context("the provided code is not valid UTF-8")?
                .to_owned(),
            Some(file_path),
            false,
        )?;

        let result = Self::parse_module(&module, &ctx);

        let msgs = ctx.error_buffer.get();
        if !msgs.is_empty() {
            eprintln!("{msgs}");
        }

        result.map(|_| ())
    }

    pub fn parse_code(code: &[u8]) -> Result<Self> {
        let ctx = ParserContext::new();
        let module = ctx.parse(
            std::str::from_utf8(code)
                .context("the provided code is not valid UTF-8")?
                .to_owned(),
            None,
            false,
        )?;

        Self::parse_module(&module, &ctx)
    }

    fn parse_module(module: &Module, ctx: &ParserContext) -> Result<Self> {
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
                                                _ => {
                                                    let err = ctx
                                                        .handler
                                                        .span_fatal(kv.span(), "expected property");
                                                    bail!(err)
                                                }
                                            };

                                            let body = match policy_name {
                                                PolicyName::Read
                                                | PolicyName::Create
                                                | PolicyName::Update => match &*kv.value {
                                                    Expr::Arrow(arrow) => {
                                                        if matches!(policy_name, PolicyName::Read) {
                                                            FilterPolicy::from_arrow(arrow, ctx)?
                                                                .into()
                                                        } else {
                                                            FilterPolicy::from_arrow_unoptimized(
                                                                arrow, ctx,
                                                            )?
                                                            .into()
                                                        }
                                                    }
                                                    _ => {
                                                        let err = ctx.handler.span_fatal(kv.value.span(), "Only arrow functions are supported in policies");
                                                        bail!(err)
                                                    }
                                                },
                                                PolicyName::OnRead
                                                | PolicyName::OnCreate
                                                | PolicyName::OnUpdate
                                                | PolicyName::GeoLoc => match &*kv.value {
                                                    Expr::Arrow(arrow) => {
                                                        TransformPolicy::from_arrow(
                                                            arrow,
                                                            ctx.sm.clone(),
                                                        )?
                                                        .into()
                                                    }
                                                    _ => {
                                                        let err = ctx.handler.span_fatal(kv.value.span(), "Only arrow functions are supported in policies");
                                                        bail!(err);
                                                    }
                                                },
                                            };

                                            policies.insert(policy_name, body);
                                        }
                                        _ => {
                                            let err = ctx.handler.span_fatal(
                                                prop.span(),
                                                "unexpected property in policy object",
                                            );
                                            bail!(err)
                                        }
                                    },
                                    _ => {
                                        let err =
                                            ctx.handler.span_fatal(o.span(), "expected property");
                                        bail!(err)
                                    }
                                }
                            }
                        }
                        _ => {
                            let err = ctx.handler.span_fatal(
                                e.span(),
                                "default export for policies should be an object.",
                            );
                            bail!(err)
                        }
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
pub struct OptimizedFilterPolicy {
    pub js_code: Box<[u8]>,
    pub skip_cond: Option<Cond>,
    pub predicates: Predicates,
    pub env: Arc<Environment>,
    pub params: PolicyParams,
}

#[derive(Debug, Clone)]
pub enum FilterPolicy {
    Optimized(OptimizedFilterPolicy),
    Js { js_code: Box<[u8]> },
}

impl FilterPolicy {
    fn from_arrow(arrow: &ArrowExpr, ctx: &ParserContext) -> Result<Self> {
        match Self::try_from_arrow_optimized(arrow, ctx) {
            Ok(pol) => Ok(pol),
            Err(_) => Self::from_arrow_unoptimized(arrow, ctx),
        }
    }

    fn from_arrow_unoptimized(arrow: &ArrowExpr, ctx: &ParserContext) -> Result<Self> {
        let js_code = emit_arrow_js_code(arrow, ctx.sm.clone())?;
        Ok(Self::Js { js_code })
    }

    fn try_from_arrow_optimized(arrow: &ArrowExpr, ctx: &ParserContext) -> Result<Self> {
        let arrow = ArrowFunction::parse(arrow)?;
        let params: Vec<_> = arrow.params().map(|(name, _)| name).collect();
        let params = PolicyParams::from_idents(&params);
        let mut builder = RulesBuilder::new(&arrow.stmt_map, &ctx.handler);
        let skip_cond = builder
            .skip_condition_from_region(&arrow.regions, Cond::True)?
            .map(Cond::not);
        let predicates = builder.predicates;
        let env = Arc::new(builder.env);
        let js_code = emit_arrow_js_code(arrow.orig, ctx.sm.clone())?;

        Ok(Self::Optimized(OptimizedFilterPolicy {
            skip_cond,
            predicates,
            params,
            env,
            js_code,
        }))
    }

    pub fn code(&self) -> &[u8] {
        match self {
            FilterPolicy::Optimized(pol) => &pol.js_code,
            FilterPolicy::Js { ref js_code } => js_code,
        }
    }

    #[cfg(test)]
    fn as_optimized(&self) -> Option<&OptimizedFilterPolicy> {
        match self {
            FilterPolicy::Optimized(ref pol) => Some(pol),
            FilterPolicy::Js { .. } => None,
        }
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

#[derive(Clone)]
pub enum Cond {
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Not(Box<Self>),
    /// Predicate identified by an id
    Predicate(usize),
    True,
    False,
}

impl fmt::Debug for Cond {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::And(lhs, rhs) => write!(f, "({lhs:?} && {rhs:?})"),
            Self::Or(lhs, rhs) => write!(f, "({lhs:?} || {rhs:?})"),
            Self::Not(arg) => write!(f, "!{arg:?}"),
            Self::Predicate(arg) => write!(f, "p{arg}"),
            Self::True => write!(f, "True"),
            Self::False => write!(f, "False"),
        }
    }
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

    fn not(c: Self) -> Self {
        Self::Not(c.into())
    }

    fn or(lhs: Cond, rhs: Cond) -> Self {
        match (lhs, rhs) {
            (Cond::True, _) | (_, Cond::True) => Cond::True,
            (Cond::False, other) | (other, Cond::False) => other,
            (lhs, rhs) => Cond::Or(lhs.into(), rhs.into()),
        }
    }

    fn and(lhs: Cond, rhs: Cond) -> Self {
        match (lhs, rhs) {
            (Cond::True, other) | (other, Cond::True) => other,
            (Cond::False, _) | (_, Cond::False) => Cond::False,
            (lhs, rhs) => Cond::And(lhs.into(), rhs.into()),
        }
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
            Bool::And(it) => Cond::and(
                Self::from_bool(&it[0], mapping),
                Self::from_bool(&it[1], mapping),
            ),
            Bool::Or(it) => Cond::or(
                Self::from_bool(&it[0], mapping),
                Self::from_bool(&it[1], mapping),
            ),
            Bool::Not(b) => Cond::not(Self::from_bool(b, mapping)),
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

struct RulesBuilder<'a, 'h> {
    stmt_map: &'a StmtMap<'a>,
    predicates: Predicates,
    predicate_cache: HashMap<&'a Expr, usize>,
    env: Environment,
    handler: &'h Handler,
}

#[derive(PartialEq, Debug, Eq, Hash, Clone, Copy)]
pub enum Action {
    Allow,
    Log,
    Deny,
    Skip,
}

impl<'a, 'h> RulesBuilder<'a, 'h> {
    fn new(stmt_map: &'a StmtMap<'a>, handler: &'h Handler) -> Self {
        Self {
            stmt_map,
            predicates: Predicates::new(),
            env: Environment::default(),
            predicate_cache: HashMap::new(),
            handler,
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
                        let id = match self.predicate_cache.get(&*stmt.test) {
                            Some(id) => *id,
                            None => {
                                let id = self.predicates.insert(predicate);
                                self.predicate_cache.insert(&stmt.test, id);
                                id
                            }
                        };
                        Cond::Predicate(id)
                    }
                    _ => unreachable!("expected if statement"),
                }
            }
            _ => unreachable!(),
        }
    }

    fn skip_condition_from_region(
        &mut self,
        region: &Region,
        cond: Cond,
    ) -> anyhow::Result<Option<Cond>> {
        let action = match region {
            Region::Cond(region) => {
                let test_cond = self.extract_cond_from_test(&region.test_region);

                let cons_cond = Cond::and(test_cond.clone(), cond.clone());
                let cons_cond = self.skip_condition_from_region(&region.cons_region, cons_cond)?;

                let alt_cond = Cond::and(Cond::not(test_cond), cond);
                let alt_cond = self.skip_condition_from_region(&region.alt_region, alt_cond)?;

                match (cons_cond, alt_cond) {
                    (None, None) => None,
                    (None, Some(cond)) | (Some(cond), None) => Some(cond),
                    // This probably mean True?
                    (Some(lhs), Some(rhs)) => Some(Cond::or(lhs, rhs)),
                }
            }
            // A Seq region may have side effects, never skip
            // TODO: warn user if seq contains branch that may skip
            Region::Seq(seq) => {
                if self
                    .skip_condition_from_region(&seq.0, cond.clone())?
                    .is_some()
                    || self.skip_condition_from_region(&seq.1, cond)?.is_some()
                {
                    let _ = self.handler.fatal("skip branch could not be optimized");
                }
                None
            }
            Region::BasicBlock(b) => self.skip_condition_from_block(b, cond),
            // TODO: warn if loop leads to skip!
            Region::Loop(_) => None,
        };

        Ok(action)
    }

    fn skip_condition_from_block(&mut self, b: &[Idx], cond: Cond) -> Option<Cond> {
        let is_skip = b
            .iter()
            .map(|i| self.stmt_map[*i].stmt)
            .map(stmt_to_action)
            .any(|action| matches!(action, Some(Action::Skip)));

        if is_skip && b.len() == 1 {
            return Some(cond);
        } else if is_skip {
            // this is a skip branch, but it has side effects. Do not skip, but warn the user
            let mut spans = b
                .iter()
                .map(|i| self.stmt_map[*i].stmt.span())
                .collect::<Vec<_>>();
            // remove the return statement.
            spans.pop();
            let multi_span = MultiSpan::from_spans(spans);
            self.handler.span_warn_with_code(
                multi_span,
                "Could not optimize skip branch. Consider making the return statement the only statement in the skip branch block.",
                DiagnosticId::Lint(String::new()),
            );
        }

        None
    }
}

fn stmt_to_action(stmt: &Stmt) -> Option<Action> {
    match stmt {
        Stmt::Return(ret) => match &ret.arg {
            Some(arg) => match &**arg {
                Expr::Member(m) => {
                    match &*m.obj {
                        Expr::Ident(id) if &*id.sym == "Action" => (),
                        expr => panic!("invalid return expression {expr:?}"),
                    };

                    match &m.prop {
                        MemberProp::Ident(id) => match &*id.sym {
                            "Allow" => Some(Action::Allow),
                            "Skip" => Some(Action::Skip),
                            "Deny" => Some(Action::Deny),
                            "Log" => Some(Action::Log),
                            _ => None,
                        },
                        _ => panic!("invalid return expression"),
                    }
                }
                _ => None,
            },
            None => None,
        },
        _ => None,
    }
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

#[cfg(test)]
mod test {
    use std::borrow::Cow;
    use std::collections::BTreeMap;

    use boa_engine::property::Attribute;
    use boa_engine::{Context, JsValue};
    use itertools::Itertools;
    use regex::{Captures, Regex};

    use crate::parse::ParserContext;

    use super::*;

    fn eval_cond(cond: &Cond, preds: &[bool]) -> bool {
        match cond {
            Cond::And(lhs, rhs) => eval_cond(lhs, preds) && eval_cond(rhs, preds),
            Cond::Or(lhs, rhs) => eval_cond(lhs, preds) || eval_cond(rhs, preds),
            Cond::Not(cond) => !eval_cond(cond, preds),
            Cond::Predicate(i) => preds[*i],
            Cond::True => true,
            Cond::False => false,
        }
    }

    /// Returns an iterator over all combinations of arrays of bools of size N.
    fn combinations<const N: usize>() -> impl Iterator<Item = [bool; N]> {
        (0u32..).take_while(|&x| x != ((1 << N) - 1) + 1).map(|x| {
            let mut out = [false; N];
            out.iter_mut()
                .enumerate()
                .for_each(|(i, o)| *o = (x & (1 << i)) == 0);
            out
        })
    }

    fn make_context() -> Context {
        let mut context = Context::default();
        let actions = r#"const Action = { Allow: 0, Deny: 1, Skip: 2, Log: 3 }"#;
        let actions = context.eval(actions).unwrap();
        context.register_global_property("Action", actions, Attribute::all());
        context
    }

    /// This macro takes a string of code and returns a function that takes a map of variables and
    /// substitutes occurences of these variable in the code with their value in the map.
    macro_rules! gen_code {
        ($code:literal) => {
            |map: &BTreeMap<String, bool>| -> Cow<'static, str> {
                let pat = map.keys().join("|");
                let re = Regex::new(&format!("({pat})")).unwrap();

                re.replace_all($code, |cap: &Captures| {
                    let var = &cap[0];
                    match map.get(var) {
                        Some(repl) => repl.to_string(),
                        None => var.to_string(),
                    }
                })
            }
        };
    }

    fn filter_from(code: &str) -> FilterPolicy {
        let parser = ParserContext::new();
        let module = parser.parse(code.into(), None, false).unwrap();
        let policies = Policies::parse_module(&module, &parser).unwrap();

        policies
            .policies
            .get(&PolicyName::Read)
            .unwrap()
            .as_filter()
            .unwrap()
            .clone()
    }

    /// Generates tests that verify that the skip expression matches the output of the js.
    /// The first argument is the name of the test.
    /// The second argument is the number of predicates in the code
    /// the third argument tells whether the code is expected to always be optimized. If true, for
    /// all input for which the function returns skip, the skip expression should return true, and
    /// for all expressions for which the skip expression returns true, the js should return Skip.
    /// If it is set to false, then only the second part is checked.
    /// The last argument is the js code to test, where presicated should be variables in the form
    /// p1, p2... pn.
    ///
    /// The invariant described is checked exhaustively for all combination of predicate value.
    macro_rules! exhaustive_skip_test {
        ($name:ident, $pred_count:literal, $should_always_optimize:literal, $code:literal) => {
            #[test]
            fn $name() {
                let code_gen = gen_code!($code);
                let mut context = make_context();
                let mut combs = combinations::<$pred_count>();

                while let Some(comb) = combs.next() {
                    let map = comb
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (format!("p{i}"), *v))
                        .collect();
                    let code = code_gen(&map);
                    let read_filter = filter_from(&code);
                    let read_filter = read_filter.as_optimized().unwrap();
                    let cond = read_filter.skip_cond.as_ref().unwrap();
                    let body = &read_filter.js_code;
                    let should_skip = eval_cond(&cond, &comb);

                    if $should_always_optimize {
                        let ret = context
                            .eval(body.as_ref())
                            .unwrap()
                            .as_callable()
                            .unwrap()
                            .call(
                                &JsValue::Null,
                                &[JsValue::Null, JsValue::Null],
                                &mut context,
                            )
                            .unwrap();

                        if ret.as_number() == Some(2.0) {
                            assert!(should_skip);
                        }

                        if should_skip {
                            assert_eq!(ret.as_number(), Some(2.0));
                        }
                    } else if should_skip {
                        // fitler tells us to skip
                        // make sure js code aggrees
                        let ret = context
                            .eval(body.as_ref())
                            .unwrap()
                            .as_callable()
                            .unwrap()
                            .call(
                                &JsValue::Null,
                                &[JsValue::Null, JsValue::Null],
                                &mut context,
                            )
                            .unwrap();

                        assert_eq!(ret.as_number().unwrap() as usize, 2);
                    }
                }
            }
        };
    }

    exhaustive_skip_test!(
        basic,
        1,
        true,
        r##"
        export default {
            read: (entity, ctx) => {
                if (p0) {
                    return Action.Skip;
                }
            }
        }"##
    );

    exhaustive_skip_test!(
        two_branches,
        2,
        true,
        r##"
        export default {
            read: (entity, ctx) => {
                if (p0) {
                    return Action.Skip;
                } else if (p1) {
                    return Action.Skip;
                }
            }
        }"##
    );

    exhaustive_skip_test!(
        allow_branch_with_side_effect,
        2,
        true,
        r##"
        export default {
            read: (entity, ctx) => {
                if (p0) {
                    let x = 1;
                    return Action.Allow;
                } else if (p1) {
                    return Action.Skip;
                }
            }
        }"##
    );

    exhaustive_skip_test!(
        complex_expression_always_optimizable,
        5,
        true,
        r##"
        export default {
            read: (entity, ctx) => {
            if (p0) {
                if (p1) {
                    return Action.Allow;
                } else {
                    return Action.Skip;
                }
            } else if (p2) {
                if (p3) {
                    let x = 12;
                    return Action.Log;
                }
            }

            if (p4) {
                return Action.Skip;
            }
        }
        }"##
    );

    exhaustive_skip_test!(
        skip_not_always_optimizable,
        2,
        false,
        r##"
        export default {
            read: (entity, ctx) => {
            if (p0) {
                let x = 12;
                return Action.Skip;
            } else if (p1) {
                return Action.Skip;
            }
        }
        }"##
    );

    #[test]
    fn cant_optimize_skip_side_effect() {
        let code = r##"
        export default {
            read: (entity, ctx) => {
                if (p0) {
                    let x = 12;
                    return Action.Skip;
                }
            }
        }"##;

        let filter = filter_from(code);
        let filter = filter.as_optimized().unwrap();
        assert!(filter.skip_cond.is_none());
    }

    #[test]
    fn cant_optimize_seq_region_skip() {
        let code = r##"
        export default {
            read: (entity, ctx) => {
                let x = 12;
                if (p0) {
                    return Action.Skip;
                }
            }
        }"##;

        let filter = filter_from(code);
        let filter = filter.as_optimized().unwrap();
        assert!(filter.skip_cond.is_none());
    }

    #[test]
    fn can_optimize_seq_cond_skip() {
        let code = r##"
        export default {
            read: (entity, ctx) => {
                if (p0) {
                  return Action.Allow;
                }

                if (p1) {
                   return Action.Deny;
                }

                if (p2) {
                   return Action.Log;
                }

                if (p3) {
                    return Action.Skip;
                }
            }
        }"##;

        let filter = filter_from(code);
        let filter = filter.as_optimized().unwrap();
        let debug_skip_cond = format!("{:?}", filter.skip_cond.as_ref().unwrap());
        assert_eq!(debug_skip_cond, "(p3 && (!p2 && (!p1 && !p0)))");
    }

    #[test]
    fn partial_optimize() {
        let code = r##"
        export default {
            read: (entity, ctx) => {
                // this skip is optimized
                if (p0) {
                    return Action.Skip;
                }
                // some side effect
                hello();
                if (p1) {
                    // this skip cannot be optimized
                    return Action.Skip;
                }
            }
        }"##;

        let filter = filter_from(code);
        let skip = format!(
            "{:?}",
            filter.as_optimized().unwrap().skip_cond.as_ref().unwrap()
        );
        assert_eq!(skip, "p0")
    }
}
