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

/// The `DNSZone` controller's action for each outcome (ADR-0016), and the
/// duplicate-zone mapper that replaced the timer a `DuplicateZone` loser
/// waited on.
#[cfg(test)]
mod event_driven_tests {
    use super::super::{action_for_zone_outcome, zones_contending_for_name};
    use crate::crd::DNSZone;
    use crate::dnszone::types::{ZoneOutcome, REASON_DEGRADED, REASON_DUPLICATE_ZONE};
    use bindy_controller_sdk::reconcile::MAX_SCHEDULED_WAKE;
    use bindy_controller_sdk::retry::{reset_reconcile_backoff, RECONCILE_BACKOFF_INITIAL};
    use kube::runtime::controller::Action;
    use kube::runtime::reflector::ObjectRef;
    use std::sync::Arc;
    use std::time::Duration;

    fn zone(namespace: &str, name: &str, zone_name: &str, duplicate: bool) -> DNSZone {
        let mut value = serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": {"name": name, "namespace": namespace},
            "spec": {
                "zoneName": zone_name,
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
        });
        if duplicate {
            value["status"] = serde_json::json!({"conditions": [{
                "type": "Ready",
                "status": "False",
                "reason": REASON_DUPLICATE_ZONE,
                "message": "already declared"
            }]});
        }
        serde_json::from_value(value).expect("valid DNSZone fixture")
    }

    #[test]
    fn a_converged_zone_awaits_the_next_change() {
        let z = zone("dns", "converged", "example.com", false);
        assert_eq!(
            action_for_zone_outcome(&z, &ZoneOutcome::Converged { next_wake: None }),
            Action::await_change()
        );
    }

    #[test]
    fn a_waiting_zone_awaits_the_next_change() {
        let z = zone("dns", "waiting", "example.com", false);
        assert_eq!(
            action_for_zone_outcome(
                &z,
                &ZoneOutcome::Waiting {
                    reason: REASON_DUPLICATE_ZONE
                }
            ),
            Action::await_change()
        );
    }

    #[test]
    fn a_degraded_zone_retries_on_the_backoff() {
        let z = zone("dns", "degraded", "example.com", false);
        reset_reconcile_backoff(&bindy_controller_sdk::error::backoff_key(&z));

        assert_eq!(
            action_for_zone_outcome(
                &z,
                &ZoneOutcome::Retry {
                    reason: REASON_DEGRADED
                }
            ),
            Action::requeue(RECONCILE_BACKOFF_INITIAL)
        );
    }

    #[test]
    fn a_zone_waiting_on_a_secondary_transfer_rechecks_on_the_short_interval() {
        let z = zone("dns", "transfer-pending", "example.com", false);
        reset_reconcile_backoff(&bindy_controller_sdk::error::backoff_key(&z));

        assert_eq!(
            action_for_zone_outcome(
                &z,
                &ZoneOutcome::Retry {
                    reason: crate::dnszone::types::REASON_TRANSFER_PENDING
                }
            ),
            Action::requeue(super::super::TRANSFER_PENDING_RECHECK)
        );
        reset_reconcile_backoff(&bindy_controller_sdk::error::backoff_key(&z));
    }

    #[test]
    fn a_scheduled_wake_is_honoured_and_capped() {
        let z = zone("dns", "signed", "example.com", false);
        let soon = Duration::from_secs(60);

        assert_eq!(
            action_for_zone_outcome(
                &z,
                &ZoneOutcome::Converged {
                    next_wake: Some(soon)
                }
            ),
            Action::requeue(soon)
        );
        assert_eq!(
            action_for_zone_outcome(
                &z,
                &ZoneOutcome::Converged {
                    next_wake: Some(MAX_SCHEDULED_WAKE * 2)
                }
            ),
            Action::requeue(MAX_SCHEDULED_WAKE)
        );
    }

    fn names(refs: Vec<ObjectRef<DNSZone>>) -> Vec<String> {
        let mut names: Vec<String> = refs
            .into_iter()
            .map(|r| format!("{}/{}", r.namespace.unwrap_or_default(), r.name))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_zone_change_wakes_the_other_claimants_of_its_name() {
        let zones = vec![
            Arc::new(zone("team-a", "winner", "example.com", false)),
            Arc::new(zone("team-b", "loser", "example.com", true)),
            Arc::new(zone("team-c", "unrelated", "other.com", false)),
        ];
        let deleted_winner = zone("team-a", "winner", "example.com", false);

        assert_eq!(
            names(zones_contending_for_name(&zones, &deleted_winner)),
            vec!["team-b/loser".to_string()]
        );
    }

    #[test]
    fn a_zone_renamed_away_still_wakes_the_zones_it_blocked() {
        // The winner's spec.zoneName changed: the mapper only sees the new
        // name, so a zone still reporting DuplicateZone is woken regardless.
        let zones = vec![
            Arc::new(zone("team-a", "winner", "renamed.com", false)),
            Arc::new(zone("team-b", "loser", "example.com", true)),
        ];
        let renamed = zone("team-a", "winner", "renamed.com", false);

        assert_eq!(
            names(zones_contending_for_name(&zones, &renamed)),
            vec!["team-b/loser".to_string()]
        );
    }

    #[test]
    fn a_zone_without_contenders_wakes_nothing() {
        let zones = vec![
            Arc::new(zone("team-a", "only", "example.com", false)),
            Arc::new(zone("team-c", "unrelated", "other.com", false)),
        ];
        let only = zone("team-a", "only", "example.com", false);

        assert!(zones_contending_for_name(&zones, &only).is_empty());
    }
}
