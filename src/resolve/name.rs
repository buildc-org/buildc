pub(crate) fn named(path: &str, given: &str) -> bool {
    path == given
        || (!given.is_empty()
            && path
                .strip_suffix(given)
                .is_some_and(|prefix| prefix.ends_with('/')))
}

pub(crate) fn matching(paths: &[String], given: &str) -> Vec<String> {
    if let Some(exact) = paths.iter().find(|path| *path == given) {
        return vec![exact.clone()];
    }
    paths
        .iter()
        .filter(|path| named(path, given))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::matching;

    #[test]
    fn module_names_are_unambiguous_suffixes() {
        let paths = vec![
            "one/hardening/coredumps".to_string(),
            "two/coredumps".to_string(),
            "one/updates".to_string(),
        ];

        assert_eq!(matching(&paths, "updates"), ["one/updates"]);
        assert_eq!(
            matching(&paths, "coredumps"),
            ["one/hardening/coredumps", "two/coredumps"]
        );
    }

    #[test]
    fn an_exact_path_beats_a_suffix() {
        let paths = vec!["a/x".to_string(), "b/a/x".to_string()];

        assert_eq!(matching(&paths, "a/x"), ["a/x"]);
        assert_eq!(matching(&paths, "b/a/x"), ["b/a/x"]);
        assert_eq!(matching(&paths, "x"), ["a/x", "b/a/x"]);
    }
}
