// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::{Path, PathBuf};

/// Returns the longest map entry whose key is a prefix of path, if one exists.
pub(crate) fn longest_prefix<'t, 'p, V>(
    path: &'p Path,
    tree: &'t BTreeMap<PathBuf, V>,
) -> Option<(&'t PathBuf, &'t V)> {
    let path_range = (Bound::Unbounded, Bound::Included(path));
    let tree_range = tree.range::<Path, _>(path_range);
    for (p, v) in tree_range.rev() {
        if path.starts_with(p) {
            return Some((p, v));
        }
    }
    None
}
