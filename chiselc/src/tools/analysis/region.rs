// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::ops::Deref;

use petgraph::{
    visit::{EdgeRef, VisitMap, Visitable},
    EdgeDirection, Graph,
};
use swc_ecmascript::ast::{Decl, Stmt};

use super::{
    control_flow::{ControlFlow, Edge, Idx, Node},
    stmt_map::StmtMap,
};

#[derive(Debug)]
pub struct CondRegion {
    /// original statement of the region pub test: Idx,
    pub test_region: Region,
    pub cons_region: Region,
    pub alt_region: Region,
}

#[derive(Debug)]
pub struct LoopRegion {
    pub header: Idx,
    pub body: Region,
}

#[derive(Debug)]
pub struct SeqRegion(pub Region, pub Region);

#[derive(Debug, Default)]
pub struct BasicBlock(Vec<Idx>);

impl BasicBlock {
    fn empty() -> Self {
        Self(Vec::new())
    }
}

impl Deref for BasicBlock {
    type Target = [Idx];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub enum Region {
    BasicBlock(BasicBlock),
    Seq(Box<SeqRegion>),
    Cond(Box<CondRegion>),
    Loop(Box<LoopRegion>),
}

impl Region {
    pub fn from_cfg(cfg: &ControlFlow, map: &StmtMap) -> Self {
        let graph = cfg.graph();
        let mut vm = graph.visit_map();
        let mut current = cfg.start();

        let mut regions = Vec::new();

        while let (region, Some(next)) = make_region(graph, current, &mut vm, map) {
            current = next;
            if let Some(region) = region {
                regions.push(region);
            }
        }

        regions
            .into_iter()
            .reduce(|a, b| SeqRegion(a, b).into())
            .unwrap()
    }

    pub fn as_basic_block(&self) -> Option<&BasicBlock> {
        match self {
            Self::BasicBlock(ref b) => Some(b),
            _ => None,
        }
    }

    pub fn as_cond_region(&self) -> Option<&CondRegion> {
        match self {
            Self::Cond(ref c) => Some(c),
            _ => None,
        }
    }

    pub fn as_seq_region(&self) -> Option<&SeqRegion> {
        match self {
            Self::Seq(ref s) => Some(s),
            _ => None,
        }
    }
}

macro_rules! into_region {
    ($($region_ty:ty => $region_variant:ident), *) => {
        $(
            impl From<$region_ty> for Region {
                fn from(region: $region_ty) -> Self {
                    Self::$region_variant(Box::new(region))
                }
            }
        )*
    };
}

into_region!(
    SeqRegion  => Seq,
    CondRegion => Cond,
    LoopRegion => Loop
);

impl From<BasicBlock> for Region {
    fn from(block: BasicBlock) -> Self {
        Self::BasicBlock(block)
    }
}

fn is_basic_block_component(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Decl(Decl::Var(_)) | Stmt::Expr(_) | Stmt::Return(_)
    )
}

fn make_region<VM: VisitMap<Idx>>(
    graph: &Graph<Node, Edge>,
    idx: Idx,
    vm: &mut VM,
    map: &StmtMap,
) -> (Option<Region>, Option<Idx>) {
    if vm.is_visited(&idx) {
        return (None, None);
    }

    vm.visit(idx);

    let stmt = match graph[idx] {
        Node::Stmt => map[idx].stmt,
        _ => return (None, graph.neighbors(idx).next()),
    };

    // FIXME: we could probably express that only in term of nodes, and detect loops with dominators
    // etc... but it is a lot simple this way, at the cost of having to pass around the map
    match stmt {
        stmt if is_basic_block_component(stmt) => make_basic_block(graph, idx, vm),
        Stmt::If(_) => make_cond_region(graph, idx, vm, map),
        Stmt::While(_) => make_loop_region(graph, idx, vm, map),

        bad => unimplemented!("unsupported statement: {bad:?}"),
    }
}

/// returns the True node and the False node from a given conditional node
fn get_cond_targets(idx: Idx, graph: &Graph<Node, Edge>) -> (Idx, Idx) {
    let mut neighs = graph.edges(idx);
    // an if node always has two outgoing branches.
    let fst = neighs.next().unwrap();
    let snd = neighs.next().unwrap();

    assert!(neighs.next().is_none(), "invalid conditional node!");

    match (fst.weight(), snd.weight()) {
        (Edge::True, Edge::False) => (fst.target(), snd.target()),
        (Edge::False, Edge::True) => (snd.target(), fst.target()),
        bad => panic!("invalid if node branches: {bad:?}"),
    }
}

fn make_loop_region<VM: VisitMap<Idx>>(
    graph: &Graph<Node, Edge>,
    idx: Idx,
    vm: &mut VM,
    map: &StmtMap,
) -> (Option<Region>, Option<Idx>) {
    let (true_idx, loop_tgt) = get_cond_targets(idx, graph);
    // if this loop has no body, we insert an empty block.
    let body = make_region(graph, loop_tgt, vm, map)
        .0
        .unwrap_or_else(|| BasicBlock::empty().into());

    let region = LoopRegion { header: idx, body };

    (Some(region.into()), Some(true_idx))
}

fn make_cond_region<VM: VisitMap<Idx>>(
    graph: &Graph<Node, Edge>,
    idx: Idx,
    vm: &mut VM,
    map: &StmtMap,
) -> (Option<Region>, Option<Idx>) {
    let (cons_idx, alt_idx) = get_cond_targets(idx, graph);
    let (cons_region, cons_idx) = make_region(graph, cons_idx, vm, map);

    // the cons branch always leads somewhere (at least to end!)
    let alt_region = if cons_idx.unwrap() != alt_idx {
        make_region(graph, alt_idx, vm, map).0.unwrap()
    } else {
        BasicBlock::empty().into()
    };

    let region = CondRegion {
        test_region: BasicBlock(vec![idx]).into(),
        cons_region: cons_region.unwrap(),
        alt_region,
    };

    (Some(region.into()), cons_idx)
}

fn is_leader(idx: Idx, graph: &Graph<Node, Edge>) -> bool {
    // is this the first statement?
    matches!(
        graph
            .neighbors_directed(idx, EdgeDirection::Incoming)
            .next()
            .map(|i| &graph[i]),
        Some(Node::Labeled("start"))
    )
        // or the last?
        || matches!(graph[idx], Node::Labeled("stop"))
    // there are more than one incomming edge, one has to be a jump
    || graph
        .neighbors_directed(idx, EdgeDirection::Incoming)
        .count()
        > 1
    // this is a conditional branch
    || graph
        .neighbors_directed(idx, EdgeDirection::Outgoing)
        .count()
        > 1
}

fn make_basic_block<VM: VisitMap<Idx>>(
    graph: &Graph<Node, Edge>,
    idx: Idx,
    vm: &mut VM,
) -> (Option<Region>, Option<Idx>) {
    let mut out = vec![idx];
    let mut current = graph.neighbors(idx).next().unwrap();
    // we assume that there will always be *at least* one statement in a basic block, otherwise,
    // we wouldn't have been called.
    while !is_leader(current, graph) {
        if vm.is_visited(&current) {
            break;
        }
        vm.visit(current);
        out.push(current);

        // a statement always has a unique neighbor, because of the "end" node of the CFG.
        current = graph.neighbors(current).next().unwrap();
    }

    (Some(BasicBlock(out).into()), Some(current))
}
