// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `watch.rs`

#[cfg(test)]
mod tests {
    use super::super::{
        instances_of_cluster, instances_of_provider, instances_selected_by_zone, zone_selection_key,
    };
    use crate::crd::{Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSZone, DNSZoneStatus};
    use kube::api::ObjectMeta;
    use kube::runtime::reflector::ObjectRef;
    use std::sync::Arc;

    fn zone_selecting(instances: &[(&str, &str)]) -> DNSZone {
        let mut zone: DNSZone = serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {"name": "example-com", "namespace": "team-a"},
            "spec": {
                "zoneName": "example.com",
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 1,
                    "refresh": 3600,
                    "retry": 600,
                    "expire": 604_800,
                    "negativeTtl": 86400
                }
            }
        }))
        .expect("valid DNSZone");
        let refs: Vec<serde_json::Value> = instances
            .iter()
            .map(|(ns, name)| {
                serde_json::json!({
                    "apiVersion": "bindy.firestoned.io/v1beta1",
                    "kind": "Bind9Instance",
                    "name": name,
                    "namespace": ns,
                    "status": "Configured"
                })
            })
            .collect();
        zone.status = Some(
            serde_json::from_value::<DNSZoneStatus>(serde_json::json!({ "bind9Instances": refs }))
                .expect("valid status"),
        );
        zone
    }

    fn instance(namespace: &str, name: &str, cluster_ref: &str) -> Arc<Bind9Instance> {
        let mut inst: Bind9Instance = serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": {"name": name, "namespace": namespace},
            "spec": {"clusterRef": cluster_ref, "role": "primary"}
        }))
        .expect("valid Bind9Instance");
        inst.metadata.namespace = Some(namespace.to_string());
        Arc::new(inst)
    }

    fn refs(items: &[(&str, &str)]) -> Vec<ObjectRef<Bind9Instance>> {
        items
            .iter()
            .map(|(ns, name)| ObjectRef::new(name).within(ns))
            .collect()
    }

    #[test]
    fn a_zone_maps_to_the_instances_it_selected() {
        let zone = zone_selecting(&[("team-a", "primary-0"), ("team-a", "secondary-0")]);
        assert_eq!(
            instances_selected_by_zone(&zone, None),
            refs(&[("team-a", "primary-0"), ("team-a", "secondary-0")])
        );
    }

    #[test]
    fn a_namespaced_controller_keeps_only_its_own_instances() {
        // A zone in team-a served by an instance in shared-dns (cross-namespace
        // targeting): only the shared-dns controller may take that ref.
        let zone = zone_selecting(&[("team-a", "local-0"), ("shared-dns", "shared-0")]);
        assert_eq!(
            instances_selected_by_zone(&zone, Some("shared-dns")),
            refs(&[("shared-dns", "shared-0")])
        );
        assert_eq!(
            instances_selected_by_zone(&zone, Some("team-a")),
            refs(&[("team-a", "local-0")])
        );
    }

    #[test]
    fn a_zone_without_status_maps_to_nothing() {
        let mut zone = zone_selecting(&[]);
        zone.status = None;
        assert!(instances_selected_by_zone(&zone, None).is_empty());
    }

    #[test]
    fn a_cluster_maps_to_its_instances_in_its_namespace() {
        let cluster = Bind9Cluster {
            metadata: ObjectMeta {
                name: Some("prod".to_string()),
                namespace: Some("dns".to_string()),
                ..ObjectMeta::default()
            },
            spec: serde_json::from_value(serde_json::json!({})).expect("empty spec"),
            status: None,
        };
        let instances = vec![
            instance("dns", "prod-0", "prod"),
            instance("dns", "dev-0", "dev"),
            instance("other", "prod-0", "prod"),
        ];
        assert_eq!(
            instances_of_cluster(&instances, &cluster),
            refs(&[("dns", "prod-0")])
        );
    }

    #[test]
    fn a_provider_maps_to_its_instances_in_every_namespace() {
        let provider = ClusterBind9Provider {
            metadata: ObjectMeta {
                name: Some("global".to_string()),
                ..ObjectMeta::default()
            },
            spec: serde_json::from_value(serde_json::json!({})).expect("empty spec"),
            status: None,
        };
        let instances = vec![
            instance("a", "g-0", "global"),
            instance("b", "g-1", "global"),
            instance("a", "x-0", "other"),
        ];
        assert_eq!(
            instances_of_provider(&instances, &provider),
            refs(&[("a", "g-0"), ("b", "g-1")])
        );
    }

    #[test]
    fn a_timestamp_written_into_zone_status_does_not_change_the_selection_key() {
        let zone = zone_selecting(&[("team-a", "primary-0")]);
        let mut stamped = zone.clone();
        let status = stamped.status.as_mut().unwrap();
        status.observed_generation = Some(42);
        status.bind9_instances[0].last_reconciled_at = Some("2026-10-05T10:00:00Z".to_string());
        assert_eq!(zone_selection_key(&zone), zone_selection_key(&stamped));
    }

    #[test]
    fn the_selection_key_changes_with_the_selected_instances() {
        let one = zone_selecting(&[("team-a", "primary-0")]);
        let two = zone_selecting(&[("team-a", "primary-0"), ("team-a", "primary-1")]);
        assert_ne!(zone_selection_key(&one), zone_selection_key(&two));
    }

    #[test]
    fn the_selection_key_ignores_the_order_instances_are_listed_in() {
        let ab = zone_selecting(&[("team-a", "a"), ("team-a", "b")]);
        let ba = zone_selecting(&[("team-a", "b"), ("team-a", "a")]);
        assert_eq!(zone_selection_key(&ab), zone_selection_key(&ba));
    }

    #[test]
    fn the_selection_key_changes_with_the_zone_name_and_on_deletion() {
        let zone = zone_selecting(&[("team-a", "primary-0")]);
        let mut renamed = zone.clone();
        renamed.spec.zone_name = "other.example.com".to_string();
        assert_ne!(zone_selection_key(&zone), zone_selection_key(&renamed));

        let mut deleting = zone.clone();
        deleting.metadata.deletion_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                k8s_openapi::jiff::Timestamp::now(),
            ));
        assert_ne!(zone_selection_key(&zone), zone_selection_key(&deleting));
    }
}
