// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::ops::Deref;

use petgraph::algo::dominators::{self, Dominators};
use petgraph::visit::EdgeRef;
use petgraph::EdgeDirection;

use super::control_flow::{ControlFlow, Edge, Idx, Node};

#[derive(Debug)]
pub enum StmtKind {
    /// Represents a conditional node statement
    Conditional,
    /// Represents a loop node statement
    #[allow(dead_code)]
    Loop,
    /// Basic block component, like an expression statement, a return statement, a variable
    /// declaration...
    BBComponent,
    /// A ghost node: does not give any information about controlflow
    Ignore,
}

/// A conditional region is comprised of three regions: the test region, contains the test
/// expression, the cons region contains the regions evaluated on true, and the alt region, the
/// region evaluated on false
#[derive(Debug, PartialEq, Eq)]
pub struct CondRegion {
    /// original statement of the region pub test: Idx,
    pub test_region: Region,
    pub cons_region: Region,
    pub alt_region: Region,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LoopRegion {
    pub header: Idx,
    pub body: Region,
}

/// A sequential region is comprised of two regions separated by a conditional (or loop) region.
#[derive(Debug, PartialEq, Eq)]
pub struct SeqRegion(pub Region, pub Region);

/// A series of statement that are always evaluated together, without conditional controlflow in
/// between.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BasicBlock(Vec<Idx>);

impl Deref for BasicBlock {
    type Target = [Idx];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Region {
    BasicBlock(BasicBlock),
    Seq(Box<SeqRegion>),
    Cond(Box<CondRegion>),
    Loop(Box<LoopRegion>),
}

/// A function that classifies a stmt idx into a stmt kind. This is passed to the cfg builder, and
/// gets called everytime the builder encounters a stmt.
pub type StmtVisitor<'a> = &'a (dyn Fn(Idx) -> StmtKind + 'a);

impl Region {
    pub fn from_cfg(cfg: &ControlFlow, visitor: StmtVisitor) -> Self {
        let graph = cfg.graph();
        let dominators = dominators::simple_fast(graph, cfg.start());

        let mut builder = RegionBuilder {
            dominators,
            cfg,
            visitor,
        };
        // The start should always have a neighbor, at least the end.
        let root = cfg.graph().neighbors(cfg.start()).next().unwrap();
        let (maybe_region, end) = builder.make_seq_region(root);
        assert_eq!(end, cfg.end(), "we should have reached the end of the CFG");
        maybe_region.unwrap_or_else(|| BasicBlock::default().into())
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

struct RegionBuilder<'a> {
    dominators: Dominators<Idx>,
    cfg: &'a ControlFlow,
    visitor: StmtVisitor<'a>,
}

impl RegionBuilder<'_> {
    fn make_seq_region(&mut self, root: Idx) -> (Option<Region>, Idx) {
        let mut current = root;
        let mut regions = Vec::new();

        // we add regions to the seq, until the end of the dominance region of the root.
        while let (Some(region), next) = self.make_region(current) {
            current = next;
            regions.push(region);
            if !self
                .dominators
                .strict_dominators(current)
                .unwrap()
                .any(|d| d == root)
            {
                break;
            }
        }

        let region = regions.into_iter().reduce(|a, b| SeqRegion(a, b).into());
        (region, current)
    }

    fn make_region(&mut self, root: Idx) -> (Option<Region>, Idx) {
        if root == self.cfg.end() {
            return (None, root);
        }

        let graph = self.cfg.graph();
        let stmt = match graph[root] {
            Node::Stmt => (self.visitor)(root),
            Node::Start | Node::End => {
                unreachable!("There should not be an end of the cfg reaching here.")
            }
        };

        // FIXME: we could probably express that only in term of nodes, and detect loops with dominators
        // etc... but it is a lot simple this way, at the cost of having to pass around the map
        match stmt {
            StmtKind::BBComponent => self.make_basic_block(root),
            StmtKind::Conditional => self.make_cond_region(root),
            // Stmt::While(_) => make_loop_region(graph, root, vm, map),
            bad => unimplemented!("unsupported statement: {bad:?}"),
        }
    }

    /// returns the True node and the False node from a given conditional node
    fn get_cond_targets(&self, idx: Idx) -> (Idx, Idx) {
        let graph = self.cfg.graph();
        let mut neighs = graph.edges(idx);
        // an if node always has two outgoing branches.
        let fst = neighs.next().expect("invalid conditional node");
        let snd = neighs.next().expect("invalid conditional node");
        assert!(neighs.next().is_none(), "invalid conditional node!");

        match (fst.weight(), snd.weight()) {
            (Edge::True, Edge::False) => (fst.target(), snd.target()),
            (Edge::False, Edge::True) => (snd.target(), fst.target()),
            bad => panic!("invalid if node branches: {bad:?}"),
        }
    }

    // fn make_loop_region<VM: VisitMap<Idx>>(
    //     graph: &Graph<Node, Edge>,
    //     idx: Idx,
    //     vm: &mut VM,
    //     map: &StmtMap,
    // ) -> (Option<Region>, Option<Idx>) {
    //     let (true_idx, loop_tgt) = get_cond_targets(idx, graph);
    //     // if this loop has no body, we insert an empty block.
    //     let body = make_region(graph, loop_tgt, vm, map)
    //         .0
    //         .unwrap_or_else(|| BasicBlock::empty().into());
    //
    //     let region = LoopRegion { header: idx, body };
    //
    //     (Some(region.into()), Some(true_idx))
    // }

    fn make_cond_region(&mut self, idx: Idx) -> (Option<Region>, Idx) {
        let (cons_idx, alt_idx) = self.get_cond_targets(idx);
        let (cons_region, cons_end) = self.make_seq_region(cons_idx);
        let (alt_region, alt_end) = self.make_seq_region(alt_idx);

        assert_eq!(
            cons_end, alt_end,
            "the two branching region should flow into the same node."
        );

        let region = CondRegion {
            test_region: BasicBlock(vec![idx]).into(),
            cons_region: cons_region.unwrap_or_else(|| BasicBlock::default().into()),
            alt_region: alt_region.unwrap_or_else(|| BasicBlock::default().into()),
        };

        (Some(region.into()), cons_end)
    }

    fn is_leader(&self, idx: Idx) -> bool {
        let graph = self.cfg.graph();
        // is this the first statement?
        graph
            .neighbors_directed(idx, EdgeDirection::Incoming)
            .next() == Some(self.cfg.start())
        // or the last?
        || idx == self.cfg.end()
        // there are more than one incomming edge, one has to be a jump target
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

    fn make_basic_block(&mut self, root: Idx) -> (Option<Region>, Idx) {
        if root == self.cfg.end() {
            return (None, root);
        }
        let graph = self.cfg.graph();
        let mut out = vec![root];
        // There has to be a neighbor, since only the end has no neighbors, and we checked that we are
        // not the end.
        let mut current = graph.neighbors(root).next().unwrap();
        // we assume that there will always be *at least* one statement in a basic block, otherwise,
        // we wouldn't have been called.
        while !self.is_leader(current) {
            out.push(current);

            // a statement always has a unique neighbor, because of the "end" node of the CFG.
            current = graph.neighbors(current).next().unwrap();
        }

        (Some(BasicBlock(out).into()), current)
    }
}

        }
        vm.visit(current);
        out.push(current);

        // a statement always has a unique neighbor, because of the "end" node of the CFG.
        current = graph.neighbors(current).next().unwrap();
    }

    (Some(BasicBlock(out).into()), Some(current))
}
