// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `secondary.rs`

#[cfg(test)]
mod tests {
    use crate::crd::InstanceReference;

    /// Helper to create test instance references
    fn create_instance_ref(name: &str, namespace: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            last_reconciled_at: None,
        }
    }

    #[test]
    fn test_instance_reference_secondary_identity() {
        let ref1 = create_instance_ref("secondary-1", "default");
        let ref2 = create_instance_ref("secondary-1", "default");
        let ref3 = create_instance_ref("secondary-2", "default");

        assert_eq!(ref1, ref2);
        assert_ne!(ref1, ref3);
    }
}

/// A secondary whose role cannot be read must fail the filter instead of
/// silently dropping out of the zone's secondary set (B9).
#[cfg(test)]
mod unreadable_role_tests {
    use crate::crd::{Bind9Instance, InstanceReference};
    use crate::dnszone::secondary::filter_secondary_instances;
    use crate::dnszone::status_helpers::calculate_expected_instance_counts;
    use kube::runtime::{reflector, watcher};
    use serde_json::json;

    fn instance(name: &str, role: &str) -> Bind9Instance {
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": {"name": name, "namespace": "dns"},
            "spec": {"clusterRef": "c", "role": role},
        }))
        .expect("valid Bind9Instance fixture")
    }

    fn reference(name: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: "dns".to_string(),
            last_reconciled_at: None,
        }
    }

    fn store(instances: Vec<Bind9Instance>) -> crate::context::MultiStore<Bind9Instance> {
        let (store, mut writer) = reflector::store();
        writer.apply_watcher_event(&watcher::Event::Init);
        for i in instances {
            writer.apply_watcher_event(&watcher::Event::InitApply(i));
        }
        writer.apply_watcher_event(&watcher::Event::InitDone);
        crate::context::MultiStore::new(vec![store])
    }

    /// A client that cannot reach any API server, so the fallback GET for an
    /// instance the store does not hold fails with a transport error.
    fn unreachable_client() -> kube::Client {
        let config =
            kube::Config::new("http://127.0.0.1:9".parse().expect("valid unreachable URL"));
        kube::Client::try_from(config).expect("client builds without connecting")
    }

    #[tokio::test]
    async fn cached_secondaries_are_returned() {
        let store = store(vec![instance("p0", "primary"), instance("s0", "secondary")]);
        let refs = vec![reference("p0"), reference("s0")];

        let secondaries = filter_secondary_instances(&unreachable_client(), &store, &refs)
            .await
            .expect("cached roles need no API call");

        assert_eq!(secondaries, vec![reference("s0")]);
    }

    #[tokio::test]
    async fn an_unreadable_instance_fails_the_secondary_filter() {
        let store = store(vec![instance("s0", "secondary")]);
        let refs = vec![reference("s0"), reference("not-cached")];

        let result = filter_secondary_instances(&unreachable_client(), &store, &refs).await;

        assert!(
            result.is_err(),
            "an unreadable instance must fail the filter, got {result:?}"
        );
    }

    #[tokio::test]
    async fn an_unreadable_instance_fails_the_expected_counts() {
        let store = store(vec![instance("p0", "primary")]);
        let refs = vec![reference("p0"), reference("not-cached")];

        let result = calculate_expected_instance_counts(&unreachable_client(), &store, &refs).await;

        assert!(
            result.is_err(),
            "a failed role lookup must not count as zero expected instances, got {result:?}"
        );
    }
}
