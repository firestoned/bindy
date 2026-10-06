// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `error.rs`

#[cfg(test)]
mod tests {
    use super::super::{
        backoff_key as object_backoff_key, converged_action, error_policy, retry_action,
        retry_action_at_least, ReconcileError,
    };
    use crate::retry::{reset_reconcile_backoff, RECONCILE_BACKOFF_INITIAL, RECONCILE_BACKOFF_MAX};
    use k8s_openapi::api::core::v1::ConfigMap;
    use kube::api::ObjectMeta;
    use kube::runtime::controller::Action;
    use std::sync::Arc;
    use std::time::Duration;

    fn config_map(namespace: &str, name: &str) -> Arc<ConfigMap> {
        Arc::new(ConfigMap {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    fn backoff_key(namespace: &str, name: &str) -> String {
        format!("{}/{namespace}/{name}", std::any::type_name::<ConfigMap>())
    }

    #[test]
    fn reconcile_error_is_transparent_over_anyhow() {
        let err = ReconcileError::from(anyhow::anyhow!("zone push failed"));
        assert_eq!(err.to_string(), "zone push failed");
    }

    #[test]
    fn first_failure_requeues_after_initial_backoff() {
        let (ns, name) = ("error-tests", "first-failure");
        reset_reconcile_backoff(&backoff_key(ns, name));

        let action = error_policy(
            config_map(ns, name),
            &ReconcileError::from(anyhow::anyhow!("boom")),
            Arc::new(()),
        );

        assert_eq!(action, Action::requeue(RECONCILE_BACKOFF_INITIAL));
    }

    #[test]
    fn repeated_failures_back_off_up_to_the_cap() {
        let (ns, name) = ("error-tests", "repeated-failures");
        reset_reconcile_backoff(&backoff_key(ns, name));
        let err = ReconcileError::from(anyhow::anyhow!("boom"));

        let first = error_policy(config_map(ns, name), &err, Arc::new(()));
        let second = error_policy(config_map(ns, name), &err, Arc::new(()));
        assert_eq!(first, Action::requeue(RECONCILE_BACKOFF_INITIAL));
        assert_eq!(second, Action::requeue(RECONCILE_BACKOFF_INITIAL * 2));

        let mut last = second;
        for _ in 0..16 {
            last = error_policy(config_map(ns, name), &err, Arc::new(()));
        }
        assert_eq!(last, Action::requeue(RECONCILE_BACKOFF_MAX));
    }

    #[test]
    fn objects_of_different_kinds_do_not_share_a_failure_counter() {
        // Same namespace/name, different kind: the key includes the type.
        let (ns, name) = ("error-tests", "shared-name");
        reset_reconcile_backoff(&backoff_key(ns, name));
        let err = ReconcileError::from(anyhow::anyhow!("boom"));
        let _ = error_policy(config_map(ns, name), &err, Arc::new(()));

        let secret = Arc::new(k8s_openapi::api::core::v1::Secret {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(ns.to_string()),
                ..Default::default()
            },
            ..Default::default()
        });
        reset_reconcile_backoff(&format!(
            "{}/{ns}/{name}",
            std::any::type_name::<k8s_openapi::api::core::v1::Secret>()
        ));
        let action = error_policy(secret, &err, Arc::new(()));

        assert_eq!(action, Action::requeue(RECONCILE_BACKOFF_INITIAL));
    }

    #[test]
    fn the_backoff_key_names_the_type_namespace_and_object() {
        let cm = config_map("error-tests", "key");
        assert_eq!(
            object_backoff_key(cm.as_ref()),
            backoff_key("error-tests", "key")
        );
    }

    /// A reconcile that ended `Ok` but failed against BIND9 retries on the
    /// same per-object backoff as an `Err` (ADR-0016 decision 3), not on a
    /// fixed interval.
    #[test]
    fn a_retry_from_the_reconcile_backs_off_like_an_error() {
        let (ns, name) = ("error-tests", "retry-action");
        reset_reconcile_backoff(&backoff_key(ns, name));
        let cm = config_map(ns, name);

        assert_eq!(
            retry_action(cm.as_ref()),
            Action::requeue(RECONCILE_BACKOFF_INITIAL)
        );
        assert_eq!(
            retry_action(cm.as_ref()),
            Action::requeue(RECONCILE_BACKOFF_INITIAL * 2)
        );
    }

    #[test]
    fn a_retry_is_never_sooner_than_its_floor() {
        let (ns, name) = ("error-tests", "retry-floor");
        reset_reconcile_backoff(&backoff_key(ns, name));
        let cm = config_map(ns, name);
        let floor = Duration::from_secs(30);

        assert_eq!(
            retry_action_at_least(cm.as_ref(), floor),
            Action::requeue(floor)
        );
        for _ in 0..16 {
            let _ = retry_action_at_least(cm.as_ref(), floor);
        }
        assert_eq!(
            retry_action_at_least(cm.as_ref(), floor),
            Action::requeue(RECONCILE_BACKOFF_MAX.max(floor))
        );
    }

    #[test]
    fn convergence_awaits_change_and_clears_the_backoff() {
        let (ns, name) = ("error-tests", "converged");
        reset_reconcile_backoff(&backoff_key(ns, name));
        let cm = config_map(ns, name);
        let _ = retry_action(cm.as_ref());
        let _ = retry_action(cm.as_ref());

        assert_eq!(converged_action(cm.as_ref()), Action::await_change());
        assert_eq!(
            retry_action(cm.as_ref()),
            Action::requeue(RECONCILE_BACKOFF_INITIAL),
            "the first failure after convergence starts from the fast interval again"
        );
    }
}
