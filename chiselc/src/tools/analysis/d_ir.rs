// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

// This module is not used for *now*. Ultimately, Dir will be used to bring more features to the
// filter syntax, but it was there already, it'll sich around for when we need it.
#![allow(dead_code)]
///! This module contains the code for the D_IR described in [this paper](https://www.cse.iitb.ac.in/~venkateshek/p1781-emani.pdf). This representation gives us algebraic equations for all variables in a program, and is useful for some code transformation, especially from imperative code to SQL.
use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::tools::analysis::region::CondRegion;
use crate::Symbol;

use anyhow::{bail, Result};
use petgraph::dot::Dot;
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::stable_graph::StableDiGraph;
use petgraph::visit::{EdgeRef, VisitMap, Visitable};
use petgraph::EdgeDirection;
use swc_ecmascript::ast::{
    AssignOp, BinExpr, BinaryOp, CallExpr, Callee, Expr, ExprStmt, Ident, MemberExpr, MemberProp,
    Pat, PatOrExpr, Stmt, VarDecl,
};

use super::control_flow::Idx;
use super::region::Region;
use super::stmt_map::StmtMap;

type Graph = StableDiGraph<Node, Edge>;

/// The `VeMap` contains pointer from symbols to nodes of the Eedag,
/// representing the algebraic computation for the value of this symbol.
#[derive(Default, Debug, Clone)]
pub(crate) struct VeMap {
    map: HashMap<Symbol, NodeIndex>,
}

fn is_blackbox(idx: NodeIndex) -> bool {
    // FIXME: this is not great...
    idx.index() == 0
}

fn is_type(sym: &Symbol) -> bool {
    sym.chars().next().map(char::is_uppercase).unwrap_or(false)
}

impl VeMap {
    fn insert(&mut self, sym: Symbol, root: NodeIndex) -> NodeIndex {
        match self.get(&sym) {
            Some(root) if is_blackbox(root) => root,
            _ => {
                self.map.insert(sym, root);
                root
            }
        }
    }

    pub fn get(&self, sym: &Symbol) -> Option<NodeIndex> {
        self.map.get(sym).copied()
    }

    /// Merges other into self. (left union)
    fn merge(&self, other: &Self) -> Self {
        let mut new = self.clone();
        for (k, v) in other.map.iter() {
            // propagate blackbox
            if !new.map.contains_key(k) || is_blackbox(*v) {
                new.map.insert(k.clone(), *v);
            }
        }

        new
    }

    fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.map.keys()
    }

    pub(crate) fn contains(&self, sym: &Symbol) -> bool {
        self.map.contains_key(sym)
    }

    fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    Binary(BinaryOp),
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(b) => b.fmt(f),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Node {
    Op(Op),
    Lit,
    Ident(Symbol),
    Project,
    Cond,
    Call,
    BlackBox,
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Op(op) => op.fmt(f),
            Node::Lit => f.write_str("lit"),
            Node::Ident(ident) => f.write_str(ident),
            Node::Project => f.write_str("."),
            Node::Cond => f.write_str("cond"),
            Node::Call => f.write_str("call"),
            Node::BlackBox => f.write_str("blackbox"),
        }
    }
}

impl From<BinaryOp> for Node {
    fn from(op: BinaryOp) -> Self {
        Node::Op(Op::Binary(op))
    }
}

impl From<&Ident> for Node {
    fn from(id: &Ident) -> Self {
        Node::Ident(id.sym.clone())
    }
}

#[derive(Clone, Debug)]
pub enum Edge {
    /// Edges for which order matters, like operands to an operator, or arguments to a function
    Indexed(usize),
    /// Conects a conditional node to its True branch
    True,
    /// Conects a conditional node to its False branch
    False,
    /// Conects a conditional node to its condition branch
    Test,
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Edge::Indexed(n) => n.fmt(f),
            Edge::True => f.write_str("true"),
            Edge::False => f.write_str("false"),
            Edge::Test => f.write_str("test"),
        }
    }
}

fn reachable(graph: &Graph, root: NodeIndex) -> Graph {
    let mut out = Graph::new();
    let idx = out.add_node(graph[root].clone());

    let mut stack = vec![(root, idx)];
    let mut vm = graph.visit_map();
    while let Some((graph_idx, out_idx)) = stack.pop() {
        if !vm.is_visited(&graph_idx) {
            vm.visit(graph_idx);
            let outgoing = graph.edges_directed(graph_idx, EdgeDirection::Outgoing);
            for e in outgoing {
                let idx = out.add_node(graph[e.target()].clone());
                out.add_edge(out_idx, idx, e.weight().clone());
                stack.push((e.target(), idx));
            }
        }
    }

    out
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum EnrichedRegionInner {
    Cond {
        test: EnrichedRegion,
        cons: EnrichedRegion,
        alt: EnrichedRegion,
    },
    Seq {
        r1: EnrichedRegion,
        r2: EnrichedRegion,
    },
    Basic(Vec<Idx>),
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct EnrichedRegion {
    pub(crate) ve_map: VeMap,
    context: GraphContext,
    pub inner: Box<EnrichedRegionInner>,
}

/// The d_ir of a program, as described in https://www.cse.iitb.ac.in/~venkateshek/p1781-emani.pdf.
/// The DIr is comprised of nested region, each their own `VeMap`.
#[derive(Debug)]
pub struct DIr {
    graph: Graph,
    pub region: EnrichedRegion,
}

impl DIr {
    pub fn from_region(region: &Region, stmt_map: &StmtMap) -> Result<Self> {
        let mut builder = Builder::new(stmt_map);
        let mut ctx = GraphContext::default();
        let region = builder.build_region(region, &mut ctx)?;
        let graph = builder.graph;

        Ok(Self { graph, region })
    }

    pub fn dot(&self, root: NodeIndex) -> String {
        let subgraph = reachable(&self.graph, root);
        let dot = Dot::new(&subgraph);
        format!("{dot}")
    }

    pub fn syms(&self) -> impl Iterator<Item = &Symbol> {
        self.region.ve_map.map.keys()
    }

    pub fn get_root(&self, sym: &Symbol) -> Option<NodeIndex> {
        self.region.ve_map.map.get(sym).copied()
    }
}

struct Builder<'a> {
    stmt_map: &'a StmtMap<'a>,
    graph: Graph,
    /// The blackbox is a sink node for anything that cannot be analyzed statically. Usually it
    /// means that we won't be abel to optimize this code.
    blackbox: NodeIndex,
}

impl<'a> Builder<'a> {
    fn new(stmt_map: &'a StmtMap<'a>) -> Self {
        let mut graph = Graph::default();
        let blackbox = graph.add_node(Node::BlackBox);
        assert_eq!(blackbox.index(), 0);
        Self {
            stmt_map,
            graph,
            blackbox,
        }
    }

    fn build_simple_statement(&mut self, idx: Idx, ctx: &mut GraphContext) -> VeMap {
        let mut ve_map = VeMap::default();
        let stmt = self.stmt_map[idx].stmt;
        match stmt {
            Stmt::Expr(ExprStmt { expr, .. }) => {
                self.build_expr(expr, ctx, &mut ve_map);
            }
            Stmt::Decl(swc_ecmascript::ast::Decl::Var(var_decl)) => {
                self.build_var_decl(var_decl, ctx, &mut ve_map);
            }
            // We only consider the condition expression in a if statment. The branches should be
            // handled by build_cond_statement.
            Stmt::If(if_stmt) => {
                self.build_expr(&if_stmt.test, ctx, &mut ve_map);
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    let out = self.build_expr(arg, ctx, &mut ve_map);
                    // FIXME: remove this hack.
                    ve_map.insert("__out__".into(), out);
                }
            }
            other => unimplemented!("unsupported statement: {other:?}"),
        };

        ve_map
    }

    fn build_assign(
        &mut self,
        op: &AssignOp,
        target: Symbol,
        value: &Expr,
        ctx: &mut GraphContext,
        ve_map: &mut VeMap,
    ) -> NodeIndex {
        if !matches!(op, AssignOp::Assign) {
            panic!("unssuported assign expression");
        }

        match value {
            Expr::Ident(id) => {
                // we are conservative here and consider that we are always aliasing the rhs.
                ve_map.insert(id.sym.clone(), self.blackbox);
                let node = ctx.add_node(Node::Ident(id.sym.clone()), &mut self.graph);
                ve_map.insert(target, node)
            }
            Expr::Bin(_) | Expr::Member(_) | Expr::Lit(_) | Expr::Call(_) => {
                let root = self.build_expr(value, ctx, ve_map);
                ve_map.insert(target, root)
            }
            other => unimplemented!("unsupported assign expression: {other:?}"),
        }
    }

    fn build_call(
        &mut self,
        call: &CallExpr,
        ctx: &mut GraphContext,
        ve_map: &mut VeMap,
    ) -> NodeIndex {
        let callee = match extract_callee(&call.callee) {
            Ok(sym) => ctx.add_node(Node::Ident(sym), &mut self.graph),
            Err(callee) if callee.is_expr() => {
                self.build_expr(callee.as_expr().unwrap(), ctx, ve_map)
            }
            Err(e) => panic!("unsupported call expression: {e:?}"),
        };

        let call_node = ctx.add_node(Node::Call, &mut self.graph);
        ctx.add_edge(call_node, callee, Edge::Indexed(0), &mut self.graph);

        // we are being very conservative here: anything that appears in a function call,
        // we consider we loose track of it.
        for (pos, arg) in call.args.iter().enumerate() {
            if arg.spread.is_some() {
                panic!("unsupported spread in call");
            }

            let root = match &*arg.expr {
                Expr::Ident(id) => ve_map.insert(id.sym.clone(), self.blackbox),
                Expr::Lit(_) => self.build_expr(&arg.expr, ctx, ve_map),
                other => unimplemented!("unsupported call argument: {other:?}"),
            };

            ctx.add_edge(call_node, root, Edge::Indexed(pos + 1), &mut self.graph);
        }

        call_node
    }

    fn build_expr(&mut self, expr: &Expr, ctx: &mut GraphContext, ve_map: &mut VeMap) -> NodeIndex {
        match expr {
            Expr::Bin(BinExpr {
                op, left, right, ..
            }) => {
                let left = self.build_expr(left, ctx, ve_map);
                let right = self.build_expr(right, ctx, ve_map);
                let node = Node::from(*op);
                let node_idx = ctx.add_node(node, &mut self.graph);
                ctx.add_edge(node_idx, left, Edge::Indexed(0), &mut self.graph);
                ctx.add_edge(node_idx, right, Edge::Indexed(1), &mut self.graph);
                ctx.root.replace(node_idx);
                node_idx
            }
            Expr::Assign(assign) => {
                let target = match &assign.left {
                    PatOrExpr::Expr(expr) => match &**expr {
                        Expr::Ident(id) => id.sym.clone(),
                        other => unimplemented!("unsupported assign target: {other:?}"),
                    },
                    PatOrExpr::Pat(pat) => match &**pat {
                        Pat::Ident(id) => id.sym.clone(),
                        other => unimplemented!("unsupported assign target: {other:?}"),
                    },
                };

                self.build_assign(&assign.op, target, &assign.right, ctx, ve_map)
            }

            Expr::Ident(id) => ctx.add_node(Node::from(id), &mut self.graph),
            Expr::Lit(_) => ctx.add_node(Node::Lit, &mut self.graph),
            Expr::Member(expr) => self.build_member(expr, ctx, ve_map),
            Expr::Call(call) => self.build_call(call, ctx, ve_map),
            Expr::Paren(expr) => self.build_expr(&expr.expr, ctx, ve_map),
            other => unimplemented!("unsupported expr: {other:?}"),
        }
    }

    fn build_member(
        &mut self,
        mem: &MemberExpr,
        ctx: &mut GraphContext,
        ve_map: &mut VeMap,
    ) -> NodeIndex {
        let project_idx = ctx.add_node(Node::Project, &mut self.graph);
        let obj_idx = match &*mem.obj {
            Expr::Ident(ident) => ctx.add_node(Node::Ident(ident.sym.clone()), &mut self.graph),
            Expr::Call(call) => self.build_call(call, ctx, ve_map),
            Expr::Member(mem) => self.build_member(mem, ctx, ve_map),
            other => todo!("must deref to a target: {other:?}"),
        };
        ctx.add_edge(project_idx, obj_idx, Edge::Indexed(0), &mut self.graph);

        let prop_idx = match &mem.prop {
            MemberProp::Ident(id) => ctx.add_node(Node::Ident(id.sym.clone()), &mut self.graph),
            _ => todo!(),
        };

        ctx.add_edge(project_idx, prop_idx, Edge::Indexed(1), &mut self.graph);

        project_idx
    }

    fn build_var_decl(
        &mut self,
        decl: &VarDecl,
        ctx: &mut GraphContext,
        ve_map: &mut VeMap,
    ) -> Option<NodeIndex> {
        if decl.decls.len() > 1 {
            todo!("unsupported multi-assignment");
        }

        let decl = &decl.decls[0];

        let target = match decl.name {
            Pat::Ident(ref id) => id.sym.clone(),
            _ => unimplemented!("unsupported assignment target"),
        };

        match decl.init {
            Some(ref expr) => self
                .build_assign(&AssignOp::Assign, target, expr, ctx, ve_map)
                .into(),
            None => None,
        }
    }

    fn build_region(&mut self, region: &Region, ctx: &mut GraphContext) -> Result<EnrichedRegion> {
        let region = match region {
            Region::BasicBlock(_) => self.build_basic_block(region, ctx),
            Region::Seq(_) => self.build_seq_region(region, ctx)?,
            Region::Cond(_) => self.build_cond_region(region, ctx)?,
            Region::Loop(_) => bail!("loops are not supported!"),
        };

        Ok(region)
    }

    fn build_seq_region(
        &mut self,
        region: &Region,
        ctx: &mut GraphContext,
    ) -> Result<EnrichedRegion> {
        let r = region
            .as_seq_region()
            .expect("build seq region should only be called for a seq region");
        let mut r1_ctx = GraphContext::default();
        let r1 = self.build_region(&r.0, &mut r1_ctx)?;
        let r2 = self.build_region(&r.1, ctx)?;

        let (ve_map, context) = self.merge(&r1_ctx, &r1.ve_map, ctx, &r2.ve_map);

        Ok(EnrichedRegion {
            ve_map,
            context,
            inner: Box::new(EnrichedRegionInner::Seq { r1, r2 }),
        })
    }

    fn build_basic_block(&mut self, region: &Region, ctx: &mut GraphContext) -> EnrichedRegion {
        let idxs = region.as_basic_block().unwrap();
        if idxs.is_empty() {
            return EnrichedRegion {
                ve_map: VeMap::default(),
                context: GraphContext::default(),
                inner: Box::new(EnrichedRegionInner::Basic(idxs.to_vec())),
            };
        }

        let mut ctx_temp = GraphContext::default();
        let mut ve_map = VeMap::default();

        for idx in idxs.iter() {
            let ve_map_temp = self.build_simple_statement(*idx, &mut ctx_temp);
            self.merge(ctx, &ve_map, &ctx_temp, &ve_map_temp);
            std::mem::swap(ctx, &mut ctx_temp);
            ctx_temp.clear();

            ve_map = ve_map_temp;
        }

        EnrichedRegion {
            ve_map,
            context: ctx.clone(),
            inner: Box::new(EnrichedRegionInner::Basic(idxs.to_vec())),
        }
    }

    fn build_cond_region(
        &mut self,
        region: &Region,
        ctx: &mut GraphContext,
    ) -> Result<EnrichedRegion> {
        let CondRegion {
            test_region,
            cons_region,
            alt_region,
        } = region
            .as_cond_region()
            .expect("build_cond_region should only be called with a cond region");

        let mut ve_map = VeMap::default();

        let cons_region = self.build_region(cons_region, ctx)?;
        let alt_region = self.build_region(alt_region, ctx)?;
        let mut test_ctx = GraphContext::default();
        let test_region = self.build_region(test_region, &mut test_ctx)?;
        let test_root = test_ctx.root.expect("test expression should have a root!");

        let changed: HashSet<_> = cons_region
            .ve_map
            .symbols()
            .chain(alt_region.ve_map.symbols())
            .chain(test_region.ve_map.symbols())
            .collect();

        // TODO: handle mutation in test expression! this should be done like a sequential region:
        // it's as if test condition was a  region that was always executed before the cond region.
        assert!(
            test_region.ve_map.is_empty(),
            "unsupported mutations in test expression"
        );

        for sym in changed {
            if cons_region.ve_map.contains(sym) || alt_region.ve_map.contains(sym) {
                let mut make_node = || ctx.add_node(Node::Ident(sym.clone()), &mut self.graph);
                let cons_root = cons_region.ve_map.get(sym).unwrap_or_else(&mut make_node);
                let alt_root = alt_region.ve_map.get(sym).unwrap_or_else(&mut make_node);

                let cond_root = ctx.add_node(Node::Cond, &mut self.graph);
                ctx.add_edge(cond_root, cons_root, Edge::True, &mut self.graph);
                ctx.add_edge(cond_root, alt_root, Edge::False, &mut self.graph);
                ctx.add_edge(cond_root, test_root, Edge::Test, &mut self.graph);
                ve_map.insert(sym.clone(), cond_root);
            }
        }

        Ok(EnrichedRegion {
            ve_map,
            context: ctx.clone(),
            inner: Box::new(EnrichedRegionInner::Cond {
                test: test_region,
                cons: cons_region,
                alt: alt_region,
            }),
        })
    }

    /// Merges sequention regions r1 and r2, with contexts ctx1 and ctx2 and ve_map1 and ve_map2,
    /// into r2, mutatng it's ctx, and ve_map.
    fn merge(
        &mut self,
        ctx1: &GraphContext,
        ve_map1: &VeMap,
        ctx2: &GraphContext,
        ve_map2: &VeMap,
    ) -> (VeMap, GraphContext) {
        //FIXME:  lots of collects here, to make borrow checker happy, maybe there's a way around that.
        let mut ctx2 = ctx2.clone();
        let leafs = ctx2
            .leafs(&self.graph)
            .filter_map(|(idx, n)| match n {
                Node::Ident(id) => Some((idx, id.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();

        // join dangling symbols leafs
        for (idx, ident) in leafs {
            if let Some(tree) = ve_map1.get(&ident) {
                let incoming = self
                    .graph
                    .edges_directed(idx, EdgeDirection::Incoming)
                    .filter_map(|e| ctx2.contains_edge(e.id()).then_some((e.id(), e.source())))
                    .collect::<Vec<_>>();

                for (id, source) in incoming {
                    if let Some(weight) = ctx2.remove_edge(id, &mut self.graph) {
                        ctx2.add_edge(source, tree, weight, &mut self.graph);
                    }
                }
            }
        }
        ctx2.merge(ctx1);
        (ve_map2.merge(ve_map1), ctx2)
    }
}

pub(crate) fn extract_callee(callee: &Callee) -> Result<Symbol, &Callee> {
    match callee {
        Callee::Expr(e) => match e.as_ref() {
            Expr::Ident(id) => Ok(id.sym.clone()),
            Expr::Member(mem) => match mem.obj.as_ident().zip(mem.prop.as_ident()) {
                // FIXME: we need a more robust method to identify class members
                Some((obj, prop)) if is_type(&obj.sym) => {
                    Ok(format!("{}.{}", obj.sym, prop.sym).into())
                }
                _ => Err(callee),
            },
            _ => Err(callee),
        },
        _ => Err(callee),
    }
}

/// Represents  sub-graph of a wider graph.
/// This is used to differentiate ee-dags in the global context.
// FIXME: (opti) It may be desirable to make this a trait and have a dummy graph context, for when we
// don't acutally need it.
#[derive(Debug, Default, Clone)]
struct GraphContext {
    // FIXME: use a bitmap instead here
    nodes: HashSet<NodeIndex>,
    edges: HashSet<EdgeIndex>,
    root: Option<NodeIndex>,
}

impl GraphContext {
    fn add_node(&mut self, node: Node, graph: &mut Graph) -> NodeIndex {
        let idx = graph.add_node(node);
        self.nodes.insert(idx);
        idx
    }

    fn add_edge(&mut self, a: NodeIndex, b: NodeIndex, edge: Edge, graph: &mut Graph) -> EdgeIndex {
        let idx = graph.add_edge(a, b, edge);
        self.edges.insert(idx);
        idx
    }

    fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
    }

    fn leafs<'a>(&'a self, graph: &'a Graph) -> impl Iterator<Item = (NodeIndex, &'a Node)> {
        self.nodes.iter().map(|idx| (*idx, &graph[*idx]))
    }

    fn merge(&mut self, other: &Self) {
        self.nodes.extend(other.nodes.iter().copied());
        self.edges.extend(other.edges.iter().copied());
    }

    fn contains_edge(&self, e: EdgeIndex) -> bool {
        self.edges.contains(&e)
    }

    fn remove_edge(&mut self, e: EdgeIndex, graph: &mut Graph) -> Option<Edge> {
        self.edges.remove(&e);
        graph.remove_edge(e)
    }
}
