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
}
