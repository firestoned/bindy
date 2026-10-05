// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `watch.rs`

#[cfg(test)]
mod tests {
    use super::super::zones_selecting_instance;
    use crate::crd::{Bind9Instance, DNSZone};
    use kube::runtime::reflector::ObjectRef;
    use std::sync::Arc;

    fn zone(namespace: &str, name: &str, instances_from: serde_json::Value) -> Arc<DNSZone> {
        let mut spec = serde_json::json!({
            "zoneName": format!("{name}.example"),
            "soaRecord": {
                "primaryNs": "ns1.example.com.",
                "adminEmail": "admin.example.com.",
                "serial": 1,
                "refresh": 3600,
                "retry": 600,
                "expire": 604_800,
                "negativeTtl": 86400
            }
        });
        if !instances_from.is_null() {
            spec["bind9InstancesFrom"] = instances_from;
        }
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "DNSZone",
                "metadata": {"name": name, "namespace": namespace},
                "spec": spec
            }))
            .expect("valid DNSZone"),
        )
    }

    fn instance(namespace: &str, name: &str, labels: serde_json::Value) -> Bind9Instance {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": {"name": name, "namespace": namespace, "labels": labels},
            "spec": {"clusterRef": "prod", "role": "primary"}
        }))
        .expect("valid Bind9Instance")
    }

    fn selector(labels: serde_json::Value) -> serde_json::Value {
        serde_json::json!([{ "selector": { "matchLabels": labels } }])
    }

    #[test]
    fn an_instance_maps_to_every_zone_whose_selector_matches_its_labels() {
        let zones = vec![
            zone(
                "team-a",
                "match",
                selector(serde_json::json!({"tier": "edge"})),
            ),
            zone(
                "team-b",
                "also-match",
                selector(serde_json::json!({"tier": "edge"})),
            ),
            zone(
                "team-a",
                "no-match",
                selector(serde_json::json!({"tier": "core"})),
            ),
            zone("team-a", "no-selector", serde_json::Value::Null),
        ];
        let inst = instance("dns", "edge-0", serde_json::json!({"tier": "edge"}));

        let mut got = zones_selecting_instance(&zones, &inst);
        got.sort_by_key(ToString::to_string);
        let mut want: Vec<ObjectRef<DNSZone>> = vec![
            ObjectRef::new("match").within("team-a"),
            ObjectRef::new("also-match").within("team-b"),
        ];
        want.sort_by_key(ToString::to_string);
        assert_eq!(got, want);
    }

    #[test]
    fn an_instance_without_labels_maps_to_nothing() {
        let zones = vec![zone(
            "team-a",
            "z",
            selector(serde_json::json!({"tier": "edge"})),
        )];
        let inst = instance("dns", "bare-0", serde_json::Value::Null);
        assert!(zones_selecting_instance(&zones, &inst).is_empty());
    }
}
