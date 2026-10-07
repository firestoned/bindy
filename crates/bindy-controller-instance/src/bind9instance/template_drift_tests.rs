// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `template_drift.rs`: the API-server defaulting each
//! comparison helper absorbs, and the differences each one still reports.

#[cfg(test)]
mod tests {
    use super::super::{
        env_equivalent, list_equivalent, map_equivalent, pull_policy_equivalent,
        quantities_equivalent, resources_equivalent,
    };
    use k8s_openapi::api::core::v1::{
        ConfigMapKeySelector, EnvVar, EnvVarSource, ObjectFieldSelector, ResourceRequirements,
        SecretKeySelector, TopologySpreadConstraint,
    };
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use std::collections::BTreeMap;

    fn q(value: &str) -> Quantity {
        Quantity(value.to_string())
    }

    fn requests(pairs: &[(&str, &str)]) -> ResourceRequirements {
        ResourceRequirements {
            requests: Some(
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), q(v)))
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn plain(name: &str, value: Option<&str>) -> EnvVar {
        EnvVar {
            name: name.to_string(),
            value: value.map(str::to_string),
            ..Default::default()
        }
    }

    fn from_secret(optional: Option<bool>) -> EnvVar {
        EnvVar {
            name: "RNDC_SECRET".to_string(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: "rndc-key".to_string(),
                    key: "secret".to_string(),
                    optional,
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn from_configmap(optional: Option<bool>) -> EnvVar {
        EnvVar {
            name: "FROM_CM".to_string(),
            value_from: Some(EnvVarSource {
                config_map_key_ref: Some(ConfigMapKeySelector {
                    name: "cm".to_string(),
                    key: "k".to_string(),
                    optional,
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn from_field(api_version: Option<&str>) -> EnvVar {
        EnvVar {
            name: "POD_NAME".to_string(),
            value_from: Some(EnvVarSource {
                field_ref: Some(ObjectFieldSelector {
                    api_version: api_version.map(str::to_string),
                    field_path: "metadata.name".to_string(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // ------------------------------------------------------------------
    // Quantities
    // ------------------------------------------------------------------

    #[test]
    fn identical_quantities_are_equivalent() {
        assert!(quantities_equivalent(&q("100m"), &q("100m")));
    }

    #[test]
    fn the_api_server_canonical_forms_are_equivalent() {
        assert!(quantities_equivalent(&q("0.5"), &q("500m")));
        assert!(quantities_equivalent(&q("1000m"), &q("1")));
        assert!(quantities_equivalent(&q("1024Mi"), &q("1Gi")));
        assert!(quantities_equivalent(&q("1e3"), &q("1k")));
        assert!(quantities_equivalent(&q("0.1"), &q("100m")));
        assert!(quantities_equivalent(&q("+2"), &q("2")));
        assert!(quantities_equivalent(&q("1.5Gi"), &q("1536Mi")));
    }

    #[test]
    fn different_quantities_are_not_equivalent() {
        assert!(!quantities_equivalent(&q("100m"), &q("200m")));
        assert!(!quantities_equivalent(&q("1G"), &q("1Gi")));
        assert!(!quantities_equivalent(&q("-1"), &q("1")));
    }

    #[test]
    fn unparseable_quantities_compare_as_text() {
        assert!(quantities_equivalent(&q("bogus"), &q("bogus")));
        assert!(!quantities_equivalent(&q("bogus"), &q("1")));
    }

    // ------------------------------------------------------------------
    // Resources
    // ------------------------------------------------------------------

    /// The rc.6 hot loop: the API server stores `resources: {}` for a
    /// container rendered without resources.
    #[test]
    fn absent_resources_equal_the_empty_object_the_api_server_returns() {
        let live = ResourceRequirements::default();
        assert!(resources_equivalent(Some(&live), None));
        assert!(resources_equivalent(None, Some(&live)));
    }

    #[test]
    fn empty_request_and_limit_maps_equal_absent_ones() {
        let live = ResourceRequirements {
            limits: Some(BTreeMap::new()),
            requests: Some(BTreeMap::new()),
            claims: Some(vec![]),
        };
        assert!(resources_equivalent(Some(&live), None));
    }

    #[test]
    fn canonicalised_requests_are_equivalent() {
        let live = requests(&[("cpu", "500m"), ("memory", "1Gi")]);
        let desired = requests(&[("cpu", "0.5"), ("memory", "1024Mi")]);
        assert!(resources_equivalent(Some(&live), Some(&desired)));
    }

    #[test]
    fn a_changed_request_is_a_difference() {
        let live = requests(&[("cpu", "500m")]);
        let desired = requests(&[("cpu", "250m")]);
        assert!(!resources_equivalent(Some(&live), Some(&desired)));
    }

    #[test]
    fn an_added_or_removed_request_is_a_difference() {
        let live = requests(&[("cpu", "500m")]);
        let desired = requests(&[("cpu", "500m"), ("memory", "64Mi")]);
        assert!(!resources_equivalent(Some(&live), Some(&desired)));
        assert!(!resources_equivalent(Some(&desired), Some(&live)));
        assert!(!resources_equivalent(Some(&live), None));
    }

    // ------------------------------------------------------------------
    // Environment
    // ------------------------------------------------------------------

    #[test]
    fn an_empty_value_equals_an_absent_one() {
        let live = vec![plain("EMPTY", None)];
        let desired = vec![plain("EMPTY", Some(""))];
        assert!(env_equivalent(Some(&live), Some(&desired)));
    }

    #[test]
    fn absent_env_equals_an_empty_list() {
        assert!(env_equivalent(None, Some(&vec![])));
        assert!(env_equivalent(Some(&vec![]), None));
    }

    #[test]
    fn optional_false_equals_optional_absent() {
        assert!(env_equivalent(
            Some(&vec![from_secret(Some(false))]),
            Some(&vec![from_secret(None)])
        ));
        assert!(env_equivalent(
            Some(&vec![from_configmap(None)]),
            Some(&vec![from_configmap(Some(false))])
        ));
    }

    #[test]
    fn optional_true_is_a_difference() {
        assert!(!env_equivalent(
            Some(&vec![from_secret(Some(true))]),
            Some(&vec![from_secret(None)])
        ));
    }

    #[test]
    fn a_defaulted_field_ref_api_version_is_equivalent() {
        assert!(env_equivalent(
            Some(&vec![from_field(Some("v1"))]),
            Some(&vec![from_field(None)])
        ));
    }

    #[test]
    fn a_changed_value_or_order_is_a_difference() {
        let a = plain("A", Some("1"));
        let b = plain("B", Some("2"));
        assert!(!env_equivalent(
            Some(&vec![a.clone()]),
            Some(&vec![plain("A", Some("3"))])
        ));
        assert!(!env_equivalent(
            Some(&vec![a.clone(), b.clone()]),
            Some(&vec![b, a])
        ));
    }

    // ------------------------------------------------------------------
    // imagePullPolicy
    // ------------------------------------------------------------------

    #[test]
    fn an_explicit_policy_compares_as_is() {
        let image = Some("ghcr.io/example/bindcar:v1");
        assert!(pull_policy_equivalent(
            Some("IfNotPresent"),
            Some("IfNotPresent"),
            image
        ));
        assert!(!pull_policy_equivalent(
            Some("Always"),
            Some("IfNotPresent"),
            image
        ));
    }

    #[test]
    fn an_absent_policy_takes_the_api_server_default_for_the_image() {
        assert!(pull_policy_equivalent(
            Some("IfNotPresent"),
            None,
            Some("ghcr.io/example/bindcar:v1")
        ));
        assert!(pull_policy_equivalent(
            Some("Always"),
            None,
            Some("ghcr.io/example/bindcar:latest")
        ));
        assert!(pull_policy_equivalent(
            Some("Always"),
            None,
            Some("ghcr.io/example/bindcar")
        ));
        assert!(pull_policy_equivalent(
            Some("Always"),
            None,
            Some("registry.example:5000/bindcar")
        ));
        assert!(pull_policy_equivalent(
            Some("IfNotPresent"),
            None,
            Some("ghcr.io/example/bindcar@sha256:abcd")
        ));
        assert!(!pull_policy_equivalent(
            Some("IfNotPresent"),
            None,
            Some("ghcr.io/example/bindcar")
        ));
    }

    // ------------------------------------------------------------------
    // Lists and maps
    // ------------------------------------------------------------------

    #[test]
    fn an_absent_list_equals_an_empty_one() {
        let empty: Vec<TopologySpreadConstraint> = vec![];
        assert!(list_equivalent(None, Some(&empty)));
        assert!(list_equivalent(Some(&empty), None));
    }

    #[test]
    fn a_non_empty_list_differs_from_an_absent_one() {
        let one = vec![TopologySpreadConstraint {
            max_skew: 1,
            topology_key: "topology.kubernetes.io/zone".to_string(),
            when_unsatisfiable: "ScheduleAnyway".to_string(),
            ..Default::default()
        }];
        assert!(!list_equivalent(None, Some(&one)));
        assert!(list_equivalent(Some(&one), Some(&one)));
    }

    #[test]
    fn an_absent_map_equals_an_empty_one() {
        let empty: BTreeMap<String, String> = BTreeMap::new();
        assert!(map_equivalent(None, Some(&empty)));
        let one: BTreeMap<String, String> = [("a".to_string(), "b".to_string())].into();
        assert!(!map_equivalent(None, Some(&one)));
        assert!(map_equivalent(Some(&one), Some(&one)));
    }
}
