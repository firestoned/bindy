// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn unset_is_all() {
        assert_eq!(NamespaceScope::parse(None), NamespaceScope::All);
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

    #[test]
    fn owned_targets_is_one_cluster_wide_target_by_default() {
        assert_eq!(owned_targets(&NamespaceScope::All), vec![None]);
    }

    #[test]
    fn owned_targets_lists_each_watched_namespace() {
        let scope = NamespaceScope::Namespaces(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            owned_targets(&scope),
            vec![Some("a".to_string()), Some("b".to_string())]
        );
    }
}

/// `run_per_target`, `target_label` and `ALL_NAMESPACES_LABEL`.
#[cfg(test)]
mod per_target_tests {
    use crate::namespace_scope::{run_per_target, target_label, ALL_NAMESPACES_LABEL};
    use crate::shutdown::channel;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Long enough that a test would visibly hang if the helper waited.
    const NEVER: Duration = Duration::from_secs(3600);
    /// Upper bound on any of these tests' runs.
    const TEST_DEADLINE: Duration = Duration::from_secs(5);

    fn targets(names: &[Option<&str>]) -> Vec<Option<String>> {
        names.iter().map(|t| t.map(str::to_string)).collect()
    }

    #[test]
    fn the_all_namespaces_label_is_unchanged() {
        assert_eq!(ALL_NAMESPACES_LABEL, "<all>");
    }

    #[test]
    fn target_label_names_the_namespace_or_all() {
        assert_eq!(target_label(None), ALL_NAMESPACES_LABEL);
        assert_eq!(target_label(Some("tenant-a")), "tenant-a");
    }

    #[tokio::test]
    async fn every_target_gets_a_controller() {
        let (trigger, signal) = channel();
        trigger.fire();
        let started = Arc::new(AtomicUsize::new(0));
        let counter = started.clone();

        run_per_target(
            targets(&[Some("a"), Some("b"), Some("c")]),
            &signal,
            move |_t| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .await
        .expect("all drained after shutdown");

        assert_eq!(started.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn the_first_failure_is_reported_without_waiting_for_the_others() {
        let (_trigger, signal) = channel();

        let result = tokio::time::timeout(
            TEST_DEADLINE,
            run_per_target(
                targets(&[Some("ok"), Some("bad")]),
                &signal,
                |t| async move {
                    if t.as_deref() == Some("bad") {
                        return Err(anyhow::anyhow!("watch failed"));
                    }
                    tokio::time::sleep(NEVER).await;
                    Ok(())
                },
            ),
        )
        .await
        .expect("reported immediately");

        let text = format!("{:#}", result.expect_err("a failure fails the fan-out"));
        assert!(text.contains("watch failed"), "{text}");
        assert!(text.contains("bad"), "names the target: {text}");
    }

    #[tokio::test]
    async fn an_early_exit_before_shutdown_is_reported_immediately() {
        let (_trigger, signal) = channel();

        let result = tokio::time::timeout(
            TEST_DEADLINE,
            run_per_target(targets(&[None, Some("x")]), &signal, |t| async move {
                if t.is_none() {
                    return Ok(());
                }
                tokio::time::sleep(NEVER).await;
                Ok(())
            }),
        )
        .await
        .expect("reported immediately");

        let text = format!("{:#}", result.expect_err("an early exit is abnormal"));
        assert!(text.contains("exited unexpectedly"), "{text}");
        assert!(
            text.contains(ALL_NAMESPACES_LABEL),
            "names the target: {text}"
        );
    }

    #[tokio::test]
    async fn exits_after_shutdown_are_a_drain() {
        let (trigger, signal) = channel();
        trigger.fire();

        run_per_target(targets(&[None]), &signal, |_t| async { Ok(()) })
            .await
            .expect("a drained controller is fine");
    }

    #[tokio::test]
    async fn a_failure_after_shutdown_is_still_a_failure() {
        let (trigger, signal) = channel();
        trigger.fire();

        let result = run_per_target(targets(&[Some("a")]), &signal, |_t| async {
            Err(anyhow::anyhow!("drain broke"))
        })
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn no_targets_is_an_error() {
        let (_trigger, signal) = channel();

        let result = run_per_target(Vec::new(), &signal, |_t| async { Ok(()) }).await;

        assert!(
            result.is_err(),
            "nothing to run is never a healthy controller"
        );
    }
}
