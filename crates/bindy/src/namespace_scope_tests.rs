// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn unset_is_all() {
        assert_eq!(NamespaceScope::parse(None), NamespaceScope::All);
        assert!(NamespaceScope::parse(None).is_all());
        assert!(NamespaceScope::parse(None).namespaces().is_empty());
    }

    #[test]
    fn empty_and_whitespace_are_all() {
        assert_eq!(NamespaceScope::parse(Some("")), NamespaceScope::All);
        assert_eq!(NamespaceScope::parse(Some("   ")), NamespaceScope::All);
        // A string of only separators is also treated as "all".
        assert_eq!(NamespaceScope::parse(Some(" , , ")), NamespaceScope::All);
    }

    #[test]
    fn single_namespace() {
        assert_eq!(
            NamespaceScope::parse(Some("bindy-system")),
            NamespaceScope::Namespaces(vec!["bindy-system".to_string()])
        );
    }

    #[test]
    fn comma_separated_list_is_trimmed() {
        let scope = NamespaceScope::parse(Some(" bindy-system , tenant-a ,tenant-b"));
        assert_eq!(
            scope,
            NamespaceScope::Namespaces(vec![
                "bindy-system".to_string(),
                "tenant-a".to_string(),
                "tenant-b".to_string(),
            ])
        );
        assert!(!scope.is_all());
        assert_eq!(scope.namespaces().len(), 3);
    }

    #[test]
    fn duplicates_removed_order_preserved() {
        assert_eq!(
            NamespaceScope::parse(Some("tenant-b,tenant-a,tenant-b,tenant-a")),
            NamespaceScope::Namespaces(vec!["tenant-b".to_string(), "tenant-a".to_string()])
        );
    }

    #[test]
    fn empty_entries_dropped() {
        assert_eq!(
            NamespaceScope::parse(Some("tenant-a,,tenant-b,")),
            NamespaceScope::Namespaces(vec!["tenant-a".to_string(), "tenant-b".to_string()])
        );
    }

    // --- api_targets: the bridge from a scope to concrete Api construction ---

    #[test]
    fn api_targets_all_is_single_cluster_wide_target() {
        // `None` means `Api::all` — exactly one watch, cluster-wide. This is the
        // backward-compatible default and MUST stay a single target so the
        // unscoped deployment keeps its current one-watch-per-kind behaviour.
        assert_eq!(NamespaceScope::All.api_targets(), vec![None]);
    }

    #[test]
    fn api_targets_namespaces_is_one_target_per_namespace() {
        let scope = NamespaceScope::parse(Some("tenant-a,tenant-b"));
        assert_eq!(
            scope.api_targets(),
            vec![Some("tenant-a"), Some("tenant-b")]
        );
    }

    #[test]
    fn api_targets_preserves_order_and_dedup_from_parse() {
        let scope = NamespaceScope::parse(Some("b,a,b"));
        assert_eq!(scope.api_targets(), vec![Some("b"), Some("a")]);
    }

    #[test]
    fn api_targets_is_never_empty() {
        // An empty target list would silently disable every watch — the operator
        // would come up healthy and reconcile nothing. Guard both variants.
        assert!(!NamespaceScope::All.api_targets().is_empty());
        assert!(!NamespaceScope::parse(Some("only-one"))
            .api_targets()
            .is_empty());
        // Whitespace-only input degrades to All, which still yields one target.
        assert_eq!(
            NamespaceScope::parse(Some("  ,  ")).api_targets(),
            vec![None]
        );
    }
}
