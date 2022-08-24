// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::BTreeMap;
use std::ops::Bound;

#[derive(Clone, Debug)]
pub struct PrefixMap<T> {
    map: BTreeMap<String, T>,
}

impl<T> Default for PrefixMap<T> {
    fn default() -> Self {
        Self {
            map: Default::default(),
        }
    }
}

impl<T> PrefixMap<T> {
    /// Returns the longest map entry whose key is a prefix of path, if one exists.
    pub fn longest_prefix(&self, path: &str) -> Option<(&str, &T)> {
        let path_range = (Bound::Unbounded, Bound::Included(path));
        let tree_range = self.map.range::<str, _>(path_range);
        for (p, v) in tree_range.rev() {
            if is_path_prefix(p, path) {
                return Some((p, v));
            }
        }
        None
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn insert(&mut self, k: String, v: T) -> Option<T> {
        self.map.insert(k, v)
    }
}

    pub fn remove_prefix(&mut self, prefix: &str) {
        self.map.retain(|k, _| !is_path_prefix(prefix, k))
    }
}

fn is_path_prefix(needle: &str, haystack: &str) -> bool {
    if !haystack.starts_with(needle) {
        return false;
    }

    needle.ends_with('/') || {
        let unmatched = &haystack[needle.len()..];
        unmatched.is_empty() || unmatched.starts_with('/')
    }
}

#[cfg(test)]
mod tests {
    use super::PrefixMap;
    use std::collections::BTreeMap;

    fn entry(path: &str) -> (String, String) {
        (path.to_string(), path.to_string())
    }

    fn fixture() -> PrefixMap<String> {
        let map = BTreeMap::from([entry("/a/b/c"), entry("/a/b"), entry("/a/bb/c")]);
        PrefixMap { map }
    }

    fn lp<'t>(path: &str, tree: &'t PrefixMap<String>) -> Option<(&'t str, &'t String)> {
        tree.longest_prefix(path.as_ref())
    }

    macro_rules! assert_longest_prefix {
        ( $tree:expr, $path:expr, $expected:expr ) => {{
            let e = entry($expected);
            let e: (&str, &String) = (&e.0, &e.1);
            assert_eq!(lp($path, &$tree), Some(e))
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

    #[test]
    fn root() {
        let mut map = PrefixMap::default();
        map.insert("/".into(), 10);
        map.insert("/hello".into(), 20);
        assert_eq!(map.longest_prefix(""), None);
        assert_eq!(map.longest_prefix("/"), Some(("/", &10)));
        assert_eq!(map.longest_prefix("/foo"), Some(("/", &10)));
        assert_eq!(map.longest_prefix("/hello"), Some(("/hello", &20)));
        assert_eq!(map.longest_prefix("/hello/foo"), Some(("/hello", &20)));
    }

    #[test]
    fn not_simple_prefix() {
        let mut map = PrefixMap::default();
        map.insert("/hello".into(), 10);
        assert_eq!(map.longest_prefix("/hello"), Some(("/hello", &10)));
        assert_eq!(map.longest_prefix("/hell"), None);
        assert_eq!(map.longest_prefix("/hellos"), None);
    }

    #[test]
    fn needle_ends_with_slash() {
        let mut map = PrefixMap::default();
        map.insert("/hello/".into(), 10);
        assert_eq!(map.longest_prefix("/hello"), None);
        assert_eq!(map.longest_prefix("/hello/"), Some(("/hello/", &10)));
        assert_eq!(map.longest_prefix("/hello/foo"), Some(("/hello/", &10)));
    }

    #[test]
    fn needle_or_haystack_empty() {
        let mut map = PrefixMap::default();
        map.insert("".into(), 10);
        map.insert("/hello".into(), 20);
        assert_eq!(map.longest_prefix(""), Some(("", &10)));
        assert_eq!(map.longest_prefix("/"), Some(("", &10)));
        assert_eq!(map.longest_prefix("/hell"), Some(("", &10)));
        assert_eq!(map.longest_prefix("/hello"), Some(("/hello", &20)));
    }
}
