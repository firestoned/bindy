// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `leader.rs`

#[cfg(test)]
mod tests {
    use super::super::{leadership_lost, LeaderElectionConfig};
    use bindy_api::constants::{
        DEFAULT_LEASE_DURATION_SECS, DEFAULT_LEASE_RENEW_DEADLINE_SECS,
        DEFAULT_LEASE_RETRY_PERIOD_SECS,
    };
    use std::collections::HashMap;
    use std::time::Duration;

    fn config(vars: &[(&str, &str)]) -> LeaderElectionConfig {
        let env: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        LeaderElectionConfig::from_lookup(|key| env.get(key).cloned())
    }

    #[test]
    fn defaults_when_nothing_is_set() {
        let c = config(&[]);
        assert!(c.enabled);
        assert_eq!(c.lease_name, "bindy-leader");
        assert_eq!(c.lease_namespace, "bindy-system");
        assert_eq!(c.lease_duration, DEFAULT_LEASE_DURATION_SECS);
        assert_eq!(c.renew_deadline, DEFAULT_LEASE_RENEW_DEADLINE_SECS);
        assert_eq!(c.retry_period, DEFAULT_LEASE_RETRY_PERIOD_SECS);
        assert!(
            c.identity.starts_with("bindy-"),
            "random fallback identity: {}",
            c.identity
        );
    }

    #[test]
    fn every_setting_can_be_overridden() {
        let c = config(&[
            ("BINDY_ENABLE_LEADER_ELECTION", "false"),
            ("BINDY_LEASE_NAME", "custom-lease"),
            ("BINDY_LEASE_NAMESPACE", "custom-ns"),
            ("BINDY_LEASE_DURATION_SECONDS", "30"),
            ("BINDY_LEASE_RENEW_DEADLINE_SECONDS", "20"),
            ("BINDY_LEASE_RETRY_PERIOD_SECONDS", "5"),
            ("POD_NAME", "bindy-0"),
        ]);
        assert!(!c.enabled);
        assert_eq!(c.lease_name, "custom-lease");
        assert_eq!(c.lease_namespace, "custom-ns");
        assert_eq!(c.lease_duration, 30);
        assert_eq!(c.renew_deadline, 20);
        assert_eq!(c.retry_period, 5);
        assert_eq!(c.identity, "bindy-0");
    }

    #[test]
    fn invalid_values_fall_back_to_defaults() {
        let c = config(&[
            ("BINDY_ENABLE_LEADER_ELECTION", "maybe"),
            ("BINDY_LEASE_DURATION_SECONDS", "fifteen"),
            ("BINDY_LEASE_RETRY_PERIOD_SECONDS", "-1"),
        ]);
        assert!(c.enabled, "an unparseable switch keeps leader election on");
        assert_eq!(c.lease_duration, DEFAULT_LEASE_DURATION_SECS);
        assert_eq!(c.retry_period, DEFAULT_LEASE_RETRY_PERIOD_SECS);
    }

    #[test]
    fn namespace_falls_back_to_the_pod_namespace() {
        assert_eq!(
            config(&[("POD_NAMESPACE", "pod-ns")]).lease_namespace,
            "pod-ns"
        );
        assert_eq!(
            config(&[
                ("POD_NAMESPACE", "pod-ns"),
                ("BINDY_LEASE_NAMESPACE", "explicit")
            ])
            .lease_namespace,
            "explicit"
        );
    }

    #[test]
    fn identity_prefers_pod_name_then_hostname() {
        assert_eq!(config(&[("HOSTNAME", "host-1")]).identity, "host-1");
        assert_eq!(
            config(&[("HOSTNAME", "host-1"), ("POD_NAME", "pod-1")]).identity,
            "pod-1"
        );
    }

    #[tokio::test]
    async fn leadership_lost_resolves_when_leadership_flips_off() {
        let (tx, rx) = tokio::sync::watch::channel(true);
        let lost = tokio::spawn(leadership_lost(rx));

        tx.send(true).unwrap(); // a renewal: still leader
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!lost.is_finished(), "still leading, must keep waiting");

        tx.send(false).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), lost)
            .await
            .expect("must resolve once leadership is lost")
            .expect("task did not panic");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn leadership_lost_errors_when_the_lease_task_goes_away() {
        let (tx, rx) = tokio::sync::watch::channel(true);
        drop(tx);
        assert!(leadership_lost(rx).await.is_err());
    }
}
