// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `primary.rs`

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

    #[tokio::test]
    async fn test_filter_primary_instances_all_primary() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 3 instance references
        //        AND all 3 instances have role=Primary
        // When: filter_primary_instances is called
        // Then: Should return all 3 instance references
    }

    #[tokio::test]
    async fn test_filter_primary_instances_mixed_roles() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 5 instance references
        //        AND 2 instances have role=Primary
        //        AND 3 instances have role=Secondary
        // When: filter_primary_instances is called
        // Then: Should return only the 2 primary instance references
    }

    #[tokio::test]
    async fn test_filter_primary_instances_none_primary() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 3 instance references
        //        AND all 3 instances have role=Secondary
        // When: filter_primary_instances is called
        // Then: Should return empty vec
    }

    #[tokio::test]
    async fn test_filter_primary_instances_handles_missing_instances() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 3 instance references
        //        AND 1 instance no longer exists in the API
        //        AND 2 instances have role=Primary
        // When: filter_primary_instances is called
        // Then: Should skip the missing instance
        //       AND return the 2 primary instances
        //       AND log warning "Failed to get instance"
    }

    #[tokio::test]
    async fn test_find_all_primary_pods_no_instances() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with no Bind9Instance resources with role=Primary
        // When: find_all_primary_pods is called
        // Then: Should return error "No PRIMARY Bind9Instance resources found"
    }

    #[tokio::test]
    async fn test_find_all_primary_pods_finds_running_pods() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with 2 PRIMARY instances
        //        AND instance-1 has 2 running pods
        //        AND instance-2 has 1 running pod
        // When: find_all_primary_pods is called
        // Then: Should return 3 PodInfo structs
        //       AND all pods should have phase="Running"
    }

    #[tokio::test]
    async fn test_find_all_primary_pods_skips_non_running() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with 1 PRIMARY instance
        //        AND 2 pods in Running phase
        //        AND 1 pod in Pending phase
        // When: find_all_primary_pods is called
        // Then: Should return only the 2 running pods
        //       AND log debug "Skipping pod ... (phase: Pending, not running)"
    }

    #[tokio::test]
    async fn test_find_primary_ips_from_instances() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 3 instance references
        //        AND 2 are PRIMARY instances
        //        AND primary-1 has 2 running pods (IPs: 10.0.1.1, 10.0.1.2)
        //        AND primary-2 has 1 running pod (IP: 10.0.2.1)
        // When: find_primary_ips_from_instances is called
        // Then: Should return vec!["10.0.1.1", "10.0.1.2", "10.0.2.1"]
    }

    #[tokio::test]
    async fn test_find_primary_ips_from_instances_skips_secondary() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: 3 instance references
        //        AND 1 is PRIMARY with 1 running pod (IP: 10.0.1.1)
        //        AND 2 are SECONDARY
        // When: find_primary_ips_from_instances is called
        // Then: Should return vec!["10.0.1.1"]
        //       AND not include secondary pod IPs
    }

    #[tokio::test]
    async fn test_for_each_primary_endpoint_success() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with 2 PRIMARY instances
        //        AND each instance has 1 endpoint
        // When: for_each_primary_endpoint is called with an operation
        // Then: Should execute the operation on both endpoints
        //       AND return Ok((Some(first_endpoint), 2))
    }

    #[tokio::test]
    async fn test_for_each_primary_endpoint_with_rndc_key() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with 2 PRIMARY instances
        //        AND each instance has RNDC key secret
        // When: for_each_primary_endpoint is called with with_rndc_key=true
        // Then: Should load RNDC key for each instance
        //       AND pass key_data to the operation closure
    }

    #[tokio::test]
    async fn test_for_each_primary_endpoint_continues_on_error() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A cluster with 3 PRIMARY instances
        //        AND the operation fails on the 2nd instance
        // When: for_each_primary_endpoint is called
        // Then: Should continue processing the 3rd instance
        //       AND return error with all failures listed
        //       AND log error "Failed operation on endpoint ..."
    }

    #[test]
    fn test_instance_reference_identity() {
        let ref1 = create_instance_ref("instance-1", "default");
        let ref2 = create_instance_ref("instance-1", "default");
        let ref3 = create_instance_ref("instance-2", "default");

        assert_eq!(ref1, ref2);
        assert_ne!(ref1, ref3);
    }
}

/// Primary-role lookups from the `Bind9Instance` reflector store (ADR-0015):
/// the record write path no longer GETs every instance to learn its role.
#[cfg(test)]
mod store_role_tests {
    use crate::crd::{Bind9Instance, InstanceReference, ServerRole};
    use crate::primary::{
        filter_instances_by_role_cached, instance_role_in_store, primary_role_in_store,
    };
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

    fn reference(name: &str, namespace: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
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

    #[test]
    fn primary_instance_in_store_is_primary() {
        let store = store(vec![instance("p0", "primary"), instance("s0", "secondary")]);

        assert_eq!(
            primary_role_in_store(&store, &reference("p0", "dns")),
            Some(true)
        );
    }

    #[test]
    fn secondary_instance_in_store_is_not_primary() {
        let store = store(vec![instance("p0", "primary"), instance("s0", "secondary")]);

        assert_eq!(
            primary_role_in_store(&store, &reference("s0", "dns")),
            Some(false)
        );
    }

    #[test]
    fn instance_missing_from_store_is_unknown() {
        let store = store(vec![instance("p0", "primary")]);

        assert_eq!(primary_role_in_store(&store, &reference("p1", "dns")), None);
        assert_eq!(
            primary_role_in_store(&store, &reference("p0", "other")),
            None
        );
    }

    #[test]
    fn the_role_of_a_cached_instance_comes_from_the_store() {
        let store = store(vec![instance("p0", "primary"), instance("s0", "secondary")]);

        assert_eq!(
            instance_role_in_store(&store, &reference("s0", "dns")),
            Some(ServerRole::Secondary)
        );
        assert_eq!(
            instance_role_in_store(&store, &reference("x0", "dns")),
            None
        );
    }

    /// A client that cannot reach any API server: if the filter tried a GET
    /// for a cached instance, that instance would be skipped and the
    /// assertions below would fail (ADR-0016 decision 6).
    fn unreachable_client() -> kube::Client {
        let config =
            kube::Config::new("http://127.0.0.1:9".parse().expect("valid unreachable URL"));
        kube::Client::try_from(config).expect("client builds without connecting")
    }

    #[tokio::test]
    async fn cached_instances_are_filtered_by_role_without_an_api_call() {
        let store = store(vec![
            instance("p0", "primary"),
            instance("s0", "secondary"),
            instance("s1", "secondary"),
        ]);
        let refs = vec![
            reference("p0", "dns"),
            reference("s0", "dns"),
            reference("s1", "dns"),
        ];
        let client = unreachable_client();

        let secondaries =
            filter_instances_by_role_cached(&client, &store, &refs, &ServerRole::Secondary).await;
        let primaries =
            filter_instances_by_role_cached(&client, &store, &refs, &ServerRole::Primary).await;

        assert_eq!(
            secondaries,
            vec![reference("s0", "dns"), reference("s1", "dns")]
        );
        assert_eq!(primaries, vec![reference("p0", "dns")]);
    }

    #[tokio::test]
    async fn an_instance_neither_cached_nor_readable_is_skipped() {
        let store = store(vec![instance("p0", "primary")]);
        let refs = vec![reference("p0", "dns"), reference("gone", "dns")];

        let primaries = filter_instances_by_role_cached(
            &unreachable_client(),
            &store,
            &refs,
            &ServerRole::Primary,
        )
        .await;

        assert_eq!(primaries, vec![reference("p0", "dns")]);
    }
}
