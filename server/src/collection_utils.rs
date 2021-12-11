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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn entry(path: &str) -> (PathBuf, String) {
        (PathBuf::from(path), path.to_string())
    }

    fn fixture() -> BTreeMap<PathBuf, String> {
        BTreeMap::from([entry("/a/b/c"), entry("/a/b"), entry("/a/bb/c")])
    }

    fn lp<'t>(
        path: &str,
        tree: &'t BTreeMap<PathBuf, String>,
    ) -> Option<(&'t PathBuf, &'t String)> {
        crate::collection_utils::longest_prefix(path.as_ref(), tree)
    }

    macro_rules! assert_longest_prefix {
        ( $tree:expr, $path:expr, $expected:expr ) => {{
            let e = entry($expected);
            assert_eq!(lp($path, &$tree), Some((&e.0, &e.1)))
        }};
    }

    #[test]
    fn exact() {
        let tt = fixture();
        assert_longest_prefix!(tt, "/a/b", "/a/b");
        assert_longest_prefix!(tt, "/a/b/c", "/a/b/c");
        assert_longest_prefix!(tt, "/a/bb/c", "/a/bb/c");
    }

    #[test]
    fn unmatched() {
        let tt = fixture();
        assert_eq!(lp("/", &tt), None);
        assert_eq!(lp("/g", &tt), None);
        assert_eq!(lp("/a", &tt), None);
        assert_eq!(lp("/a/c", &tt), None);
        assert_eq!(lp("/a/bb", &tt), None);
    }

    #[test]
    fn longer() {
        let tt = fixture();
        assert_longest_prefix!(tt, "/a/b/c/d", "/a/b/c");
        assert_longest_prefix!(tt, "/a/bb/c/d", "/a/bb/c");
        assert_longest_prefix!(tt, "/a/b/d", "/a/b");
    }
}
