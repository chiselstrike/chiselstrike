// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::ops::Index;

use vec_map::VecMap;

use super::control_flow::{Entry, Idx};

/// Maps idx in some data structure to stmts in a swc ast.
#[derive(Default, Debug, Clone)]
pub struct StmtMap<'a> {
    map: VecMap<Entry<'a>>,
}

impl<'a> StmtMap<'a> {
    /// insert a new entry in the statement map. Assumes that idx will be incremented monotonically
    /// by 1
    pub(crate) fn insert(&mut self, idx: Idx, entry: Entry<'a>) {
        self.map.insert(idx.index(), entry);
    }
}

impl<'a> Index<Idx> for StmtMap<'a> {
    type Output = Entry<'a>;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.map[index.index()]
    }
}
