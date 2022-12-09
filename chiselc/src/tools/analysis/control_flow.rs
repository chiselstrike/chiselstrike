// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::fmt;

use anyhow::{bail, Result};
use petgraph::dot::{Config, Dot};
use petgraph::graph::{DefaultIx, Graph, NodeIndex};
use petgraph::visit::Reversed;
use swc_ecmascript::ast::Stmt;

use super::stmt_map::StmtMap;

pub type Idx = NodeIndex<DefaultIx>;

#[derive(Debug, Clone, Copy)]
pub enum Edge {
    True,
    False,
    Flow,
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Edge::True => "T",
            Edge::False => "F",
            Edge::Flow => "",
        };

        f.write_str(name)
    }
}

#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub stmt: &'a Stmt,
}

#[derive(Debug, Clone)]
pub enum Node {
    Stmt,
    Start,
    End,
}

type CfgGraph = Graph<Node, Edge>;

#[derive(Clone)]
struct CFGBuilder<'a, G> {
    inner: G,
    previous: Idx,
    map: StmtMap<'a>,
    end: Idx,
}

impl<'a> CFGBuilder<'a, CfgGraph> {
    fn merge_out(&mut self, from: &[(Idx, Edge)], to: Idx) {
        for (root, edge) in from {
            self.inner.add_edge(*root, to, *edge);
        }
    }

    fn block(&mut self, stmts: &'a [Stmt]) -> Result<(Idx, Vec<(Idx, Edge)>)> {
        let mut root = None;
        let mut current = None;

        for stmt in stmts {
            let (idx, outs) = self.stmt(stmt)?;
            match (root, &current) {
                (None, None) => {
                    root.replace(idx);
                    current.replace(outs);
                }
                (_, Some(curr)) => {
                    self.merge_out(curr, idx);
                    current.replace(outs);
                }
                _ => unreachable!(),
            }
        }
        Ok((root.unwrap_or(self.previous), current.unwrap_or_default()))
    }

    fn stmt(&mut self, stmt: &'a Stmt) -> Result<(Idx, Vec<(Idx, Edge)>)> {
        match stmt {
            Stmt::Block(block) => self.block(&block.stmts),
            Stmt::If(if_stmt) => {
                let root = self.add_stmt_node(stmt);
                let (cons_root, mut cons_out) = self.stmt(&if_stmt.cons)?;

                self.inner.add_edge(root, cons_root, Edge::True);

                match if_stmt.alt {
                    Some(ref alt) => {
                        let (alt_root, alt_out) = self.stmt(alt)?;
                        if self.previous != self.end {
                            self.inner.add_edge(root, alt_root, Edge::False);
                            cons_out.extend_from_slice(&alt_out);
                        }
                    }
                    None => {
                        cons_out.push((root, Edge::False));
                    }
                }
                Ok((root, cons_out))
            }
            // Stmt::While(while_stmt) => {
            //     let root = self.add_stmt_node(stmt);
            //     let (body_root, body_outs) = self.stmt(&while_stmt.body)?;
            //     self.inner.add_edge(root, body_root, Edge::False);
            //
            //     self.merge_out(&body_outs, root);
            //
            //     Ok((root, vec![(root, Edge::True)]))
            // }
            Stmt::Expr(_) | Stmt::Decl(_) | Stmt::Empty(_) => {
                let idx = self.add_stmt_node(stmt);
                Ok((idx, vec![(idx, Edge::Flow)]))
            }
            Stmt::Return(_) => {
                let idx = self.add_stmt_node(stmt);
                self.inner.add_edge(idx, self.end, Edge::Flow);
                Ok((idx, vec![]))
            }
            // Stmt::Debugger(_) => todo!(),
            // Stmt::With(_) => todo!(),
            // Stmt::Labeled(_) => todo!(),
            // Stmt::Break(_) => todo!(),
            // Stmt::Continue(_) => todo!(),
            // Stmt::Switch(_) => todo!(),
            // Stmt::Throw(_) => todo!(),
            // Stmt::Try(_) => todo!(),
            // Stmt::DoWhile(_) => todo!(),
            // Stmt::For(_) => todo!(),
            // Stmt::ForIn(_) => todo!(),
            // Stmt::ForOf(_) => todo!(),
            _ => bail!("unsupported statement type"),
        }
    }

    fn add_stmt_node(&mut self, stmt: &'a Stmt) -> Idx {
        let entry = Entry { stmt };
        let idx = self.inner.add_node(Node::Stmt);
        self.map.insert(idx, entry);
        self.previous = idx;
        idx
    }
}

/// Control-flow graph of a program.
///
/// Strictly speaking this is a full flow graph, since the basic blocks are not reduces, and each
/// node is an individual statment, but this server the basis for the contruction of the PDG and
/// D-IR, where this representation is more convenient.
#[derive(Default, Clone)]
pub struct ControlFlow<G = CfgGraph> {
    pub inner: G,
    pub(crate) start: Idx,
    #[allow(dead_code)]
    pub(crate) end: Idx,
}

impl ControlFlow<CfgGraph> {
    // FIXME: it might be a good idea to artificially bind the graph and the map together with a lifetime.
    pub fn build(stmts: &[Stmt]) -> Result<(Self, StmtMap)> {
        let mut inner = Graph::new();
        let start = inner.add_node(Node::Start);
        let end = inner.add_node(Node::End);
        let mut builder = CFGBuilder {
            inner,
            previous: start,
            map: Default::default(),
            end,
        };

        let (root, outs) = builder.block(stmts)?;

        builder.inner.add_edge(start, root, Edge::Flow);

        builder.merge_out(&outs, end);

        let map = builder.map;

        let this = Self {
            inner: builder.inner,
            start,
            end,
        };

        Ok((this, map))
    }

    /// returns the dot format graph the the CFG
    /// if sm is Some, then the nodes are resolved to the statements line numbers
    #[allow(dead_code)]
    pub fn dot(&self) -> String {
        let node_getter = |_, (idx, node): (Idx, &Node)| match node {
            Node::Stmt => format!(r#"label="L{}" "#, idx.index() - 1),
            Node::Start => r#"label = "Start""#.into(),
            Node::End => r#"label = "End""#.into(),
        };

        impl fmt::Display for Node {
            fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
                Ok(())
            }
        }

        let edge_getter = |_, _| String::new();
        Dot::with_attr_getters(
            &self.inner,
            &[Config::NodeNoLabel],
            &edge_getter,
            &node_getter,
        )
        .to_string()
    }
}

#[allow(dead_code)]
impl<G> ControlFlow<G> {
    /// Resturns the inverse control flow graph
    pub fn reversed(&self) -> ControlFlow<Reversed<&G>> {
        let inner = Reversed(&self.inner);
        ControlFlow {
            inner,
            start: self.end,
            end: self.start,
        }
    }

    pub fn graph(&self) -> &G {
        &self.inner
    }

    pub fn graph_mut(&mut self) -> &mut G {
        &mut self.inner
    }

    pub fn start(&self) -> Idx {
        self.start
    }

    pub fn set_start(&mut self, idx: Idx) {
        self.start = idx;
    }

    pub fn end(&self) -> Idx {
        self.end
    }
}
