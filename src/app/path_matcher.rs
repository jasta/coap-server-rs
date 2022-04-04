use dashmap::{DashMap, ReadOnlyView};
use std::collections::HashMap;

pub struct PathMatcher<V> {
    map: ReadOnlyView<Vec<String>, V>,
}

impl<V> FromIterator<(Vec<String>, V)> for PathMatcher<V> {
    fn from_iter<I: IntoIterator<Item = (Vec<String>, V)>>(iter: I) -> Self {
        Self {
            map: DashMap::from_iter(iter).into_read_only(),
        }
    }
}

impl<V> PathMatcher<V> {
    pub fn from_path_strings(src: HashMap<String, V>) -> Self {
        let iter = src.into_iter().map(|(k, v)| {
            let new_key: Vec<_> = k
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            (new_key, v)
        });
        Self::from_iter(iter)
    }

    pub fn lookup(&self, path: &[String]) -> Option<MatchedResult<V>> {
        for search_depth in (0..path.len() + 1).rev() {
            let search_path = &path[0..search_depth];
            if let Some(value) = self.map.get(search_path) {
                return Some(MatchedResult {
                    value,
                    matched_index: search_depth,
                });
            }
        }
        None
    }
}

pub struct MatchedResult<'a, V> {
    pub value: &'a V,
    pub matched_index: usize,
}

#[cfg(test)]
mod tests {
    use crate::app::path_matcher::PathMatcher;

    #[test]
    fn test_unambiguous_exact_match() {
        let matcher = new_matcher(["/a/b/c", "/a/b/d"]);
        assert_exact_match(&matcher, "/a/b/c");
        assert_exact_match(&matcher, "/a/b/d");
    }

    #[test]
    fn test_ambiguous_match() {
        let matcher = new_matcher(["/a/b/c", "/a/b/d"]);
        assert_no_match(&matcher, "/a/b");
    }

    #[test]
    fn test_inexact_match() {
        let matcher = new_matcher(["/a/b/c"]);
        assert_prefix_match(&matcher, "/a/b/c/d", "/a/b/c", "d");
    }

    fn new_matcher<const N: usize>(paths: [&str; N]) -> PathMatcher<String> {
        let path_kv_pairs = paths.into_iter().map(|path| {
            let key = path_to_vec(path);
            let value = path.to_string();
            (key, value)
        });

        PathMatcher::from_iter(path_kv_pairs)
    }

    fn assert_no_match(matcher: &PathMatcher<String>, path: &str) {
        let paths = path_to_vec(path);
        assert!(matcher.lookup(&paths).is_none());
    }

    fn assert_exact_match(matcher: &PathMatcher<String>, path: &str) {
        assert_prefix_match(matcher, path, path, "");
    }

    fn assert_prefix_match(
        matcher: &PathMatcher<String>,
        path: &str,
        expected_value: &str,
        expected_suffix: &str,
    ) {
        let paths = path_to_vec(path);

        let result = matcher.lookup(&paths);
        let matched = result.unwrap_or_else(|| panic!("Expected {expected_value}"));
        assert_eq!(matched.value, expected_value);

        let actual_suffix = &paths[matched.matched_index..];
        assert_eq!(actual_suffix, &path_to_vec(expected_suffix));
    }

    fn path_to_vec(path: &str) -> Vec<String> {
        if path.is_empty() {
            return vec![];
        }
        path.split('/').map(|p| p.to_string()).collect()
    }
}
