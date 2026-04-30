// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `scout.rs` — pure helper functions (no Kubernetes API calls)

#[cfg(test)]
mod tests {
    use crate::scout::{
        arecord_cr_name, arecord_label_selector, build_service_arecord, derive_record_name,
        get_record_name_annotation, get_zone_annotation, has_finalizer, is_arecord_enabled,
        is_being_deleted, is_loadbalancer_service, is_scout_opted_in, resolve_ip_from_annotation,
        resolve_ip_from_service_lb_status, resolve_ips, resolve_record_name, resolve_zone,
        service_arecord_cr_name, service_arecord_label_selector, stale_arecord_label_selector,
        ServiceARecordParams, FINALIZER_SCOUT, LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT,
        LABEL_SOURCE_CLUSTER, LABEL_SOURCE_NAME, LABEL_SOURCE_NAMESPACE, LABEL_ZONE,
    };
    use k8s_openapi::api::core::v1::{
        LoadBalancerIngress as ServiceLoadBalancerIngress, LoadBalancerStatus, Service,
        ServiceSpec, ServiceStatus,
    };
    use k8s_openapi::api::networking::v1::{
        Ingress, IngressLoadBalancerIngress, IngressLoadBalancerStatus, IngressStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use std::collections::BTreeMap;

    // =========================================================================
    // is_arecord_enabled
    // =========================================================================

    #[test]
    fn test_is_arecord_enabled_true() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/recordKind".to_string(),
            "ARecord".to_string(),
        );
        assert!(is_arecord_enabled(&annotations));
    }

    #[test]
    fn test_is_arecord_enabled_wrong_kind() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/recordKind".to_string(),
            "CNAME".to_string(),
        );
        assert!(!is_arecord_enabled(&annotations));
    }

    #[test]
    fn test_is_arecord_enabled_missing_annotation() {
        let annotations = BTreeMap::new();
        assert!(!is_arecord_enabled(&annotations));
    }

    #[test]
    fn test_is_arecord_enabled_wrong_case() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/recordKind".to_string(),
            "arecord".to_string(),
        );
        assert!(!is_arecord_enabled(&annotations));
    }

    // =========================================================================
    // get_zone_annotation
    // =========================================================================

    #[test]
    fn test_get_zone_annotation_present() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/zone".to_string(),
            "example.com".to_string(),
        );
        assert_eq!(
            get_zone_annotation(&annotations),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_get_zone_annotation_missing() {
        let annotations = BTreeMap::new();
        assert_eq!(get_zone_annotation(&annotations), None);
    }

    #[test]
    fn test_get_zone_annotation_empty_value() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/zone".to_string(), "".to_string());
        assert_eq!(get_zone_annotation(&annotations), None);
    }

    // =========================================================================
    // derive_record_name
    // =========================================================================

    #[test]
    fn test_derive_record_name_subdomain() {
        // "app.example.com" with zone "example.com" → "app"
        let result = derive_record_name("app.example.com", "example.com").unwrap();
        assert_eq!(result, "app");
    }

    #[test]
    fn test_derive_record_name_apex() {
        // "example.com" with zone "example.com" → "@"
        let result = derive_record_name("example.com", "example.com").unwrap();
        assert_eq!(result, "@");
    }

    #[test]
    fn test_derive_record_name_deep_subdomain() {
        // "deep.sub.example.com" with zone "example.com" → "deep.sub"
        let result = derive_record_name("deep.sub.example.com", "example.com").unwrap();
        assert_eq!(result, "deep.sub");
    }

    #[test]
    fn test_derive_record_name_host_not_in_zone() {
        // "app.other.com" with zone "example.com" → error
        let result = derive_record_name("app.other.com", "example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_record_name_trailing_dot_stripped() {
        // Ingress hosts may have trailing dots in edge cases
        let result = derive_record_name("app.example.com.", "example.com").unwrap();
        assert_eq!(result, "app");
    }

    // =========================================================================
    // get_record_name_annotation
    // =========================================================================

    #[test]
    fn test_get_record_name_annotation_present() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "myapp".to_string(),
        );
        assert_eq!(
            get_record_name_annotation(&annotations),
            Some("myapp".to_string())
        );
    }

    #[test]
    fn test_get_record_name_annotation_missing() {
        let annotations = BTreeMap::new();
        assert_eq!(get_record_name_annotation(&annotations), None);
    }

    #[test]
    fn test_get_record_name_annotation_empty_value() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "".to_string(),
        );
        assert_eq!(get_record_name_annotation(&annotations), None);
    }

    #[test]
    fn test_get_record_name_annotation_whitespace_only() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "   ".to_string(),
        );
        assert_eq!(get_record_name_annotation(&annotations), None);
    }

    #[test]
    fn test_get_record_name_annotation_trims_whitespace() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "  custom  ".to_string(),
        );
        assert_eq!(
            get_record_name_annotation(&annotations),
            Some("custom".to_string())
        );
    }

    // =========================================================================
    // resolve_record_name (annotation override → derive_record_name fallback)
    // =========================================================================

    #[test]
    fn test_resolve_record_name_override_wins() {
        // When the annotation is set, it overrides the derived name
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "myapp".to_string(),
        );
        let result = resolve_record_name(&annotations, "app.example.com", "example.com").unwrap();
        assert_eq!(result, "myapp");
    }

    #[test]
    fn test_resolve_record_name_falls_back_to_derived_when_absent() {
        let annotations = BTreeMap::new();
        let result = resolve_record_name(&annotations, "app.example.com", "example.com").unwrap();
        assert_eq!(result, "app");
    }

    #[test]
    fn test_resolve_record_name_falls_back_to_derived_when_empty() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "".to_string(),
        );
        let result = resolve_record_name(&annotations, "app.example.com", "example.com").unwrap();
        assert_eq!(result, "app");
    }

    #[test]
    fn test_resolve_record_name_apex_override() {
        // "@" is a valid override for the zone apex
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "@".to_string(),
        );
        let result = resolve_record_name(&annotations, "app.example.com", "example.com").unwrap();
        assert_eq!(result, "@");
    }

    #[test]
    fn test_resolve_record_name_override_skips_zone_validation() {
        // When the annotation is set, the host is no longer required to belong to the zone:
        // the user is explicitly choosing the record name within the zone.
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "myapp".to_string(),
        );
        let result = resolve_record_name(&annotations, "app.other.com", "example.com").unwrap();
        assert_eq!(result, "myapp");
    }

    #[test]
    fn test_resolve_record_name_propagates_derive_error_when_no_override() {
        let annotations = BTreeMap::new();
        let result = resolve_record_name(&annotations, "app.other.com", "example.com");
        assert!(result.is_err());
    }

    // =========================================================================
    // resolve_ip_from_annotation
    // =========================================================================

    #[test]
    fn test_resolve_ip_from_annotation_present() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/ip".to_string(), "10.0.0.1".to_string());
        assert_eq!(
            resolve_ip_from_annotation(&annotations),
            Some("10.0.0.1".to_string())
        );
    }

    #[test]
    fn test_resolve_ip_from_annotation_missing() {
        let annotations = BTreeMap::new();
        assert_eq!(resolve_ip_from_annotation(&annotations), None);
    }

    #[test]
    fn test_resolve_ip_from_annotation_empty_value() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/ip".to_string(), "".to_string());
        assert_eq!(resolve_ip_from_annotation(&annotations), None);
    }

    // =========================================================================
    // arecord_cr_name
    // =========================================================================

    #[test]
    fn test_arecord_cr_name_basic() {
        // scout-{cluster}-{namespace}-{ingress}-{index}
        let name = arecord_cr_name("prod", "default", "my-ingress", 0);
        assert_eq!(name, "scout-prod-default-my-ingress-0");
    }

    #[test]
    fn test_arecord_cr_name_sanitizes_underscores() {
        // underscores → hyphens
        let name = arecord_cr_name("prod", "my_namespace", "my_ingress", 1);
        assert_eq!(name, "scout-prod-my-namespace-my-ingress-1");
    }

    #[test]
    fn test_arecord_cr_name_max_253_chars() {
        // Very long inputs must be truncated to ≤ 253 chars (Kubernetes name limit)
        let long_cluster = "a".repeat(100);
        let long_ns = "b".repeat(100);
        let long_ingress = "c".repeat(100);
        let name = arecord_cr_name(&long_cluster, &long_ns, &long_ingress, 0);
        assert!(
            name.len() <= 253,
            "CR name exceeded 253 chars: {} chars",
            name.len()
        );
    }

    #[test]
    fn test_arecord_cr_name_lowercase() {
        let name = arecord_cr_name("PROD", "Default", "My-Ingress", 0);
        assert_eq!(name, name.to_lowercase());
    }

    #[test]
    fn test_arecord_cr_name_no_trailing_hyphen() {
        let name = arecord_cr_name("prod", "default", "ingress", 0);
        assert!(!name.ends_with('-'));
    }

    // =========================================================================
    // resolve_ips (priority: annotation > default_ips > lb_status)
    // =========================================================================

    fn ingress_with_lb_ip(ip: &str) -> Ingress {
        Ingress {
            status: Some(IngressStatus {
                load_balancer: Some(IngressLoadBalancerStatus {
                    ingress: Some(vec![IngressLoadBalancerIngress {
                        ip: Some(ip.to_string()),
                        hostname: None,
                        ports: None,
                    }]),
                }),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_resolve_ips_annotation_wins_over_defaults() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/ip".to_string(), "1.2.3.4".to_string());
        let defaults = vec!["9.9.9.9".to_string()];
        let ingress = Ingress::default();
        assert_eq!(
            resolve_ips(&annotations, &defaults, &ingress),
            Some(vec!["1.2.3.4".to_string()])
        );
    }

    #[test]
    fn test_resolve_ips_annotation_wins_over_lb_status() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/ip".to_string(), "1.2.3.4".to_string());
        let ingress = ingress_with_lb_ip("10.0.0.1");
        assert_eq!(
            resolve_ips(&annotations, &[], &ingress),
            Some(vec!["1.2.3.4".to_string()])
        );
    }

    #[test]
    fn test_resolve_ips_defaults_win_over_lb_status() {
        let annotations = BTreeMap::new();
        let defaults = vec!["9.9.9.9".to_string(), "8.8.8.8".to_string()];
        let ingress = ingress_with_lb_ip("10.0.0.1");
        assert_eq!(
            resolve_ips(&annotations, &defaults, &ingress),
            Some(vec!["9.9.9.9".to_string(), "8.8.8.8".to_string()])
        );
    }

    #[test]
    fn test_resolve_ips_lb_status_fallback() {
        let annotations = BTreeMap::new();
        let ingress = ingress_with_lb_ip("10.0.0.1");
        assert_eq!(
            resolve_ips(&annotations, &[], &ingress),
            Some(vec!["10.0.0.1".to_string()])
        );
    }

    #[test]
    fn test_resolve_ips_none_when_nothing_available() {
        let annotations = BTreeMap::new();
        let ingress = Ingress::default();
        assert_eq!(resolve_ips(&annotations, &[], &ingress), None);
    }

    #[test]
    fn test_resolve_ips_multiple_defaults_returned() {
        let annotations = BTreeMap::new();
        let defaults = vec![
            "192.168.1.1".to_string(),
            "192.168.1.2".to_string(),
            "192.168.1.3".to_string(),
        ];
        let ingress = Ingress::default();
        assert_eq!(
            resolve_ips(&annotations, &defaults, &ingress),
            Some(defaults.clone())
        );
    }

    // =========================================================================
    // has_finalizer
    // =========================================================================

    #[test]
    fn test_has_finalizer_present() {
        let ingress = Ingress {
            metadata: kube::api::ObjectMeta {
                finalizers: Some(vec![FINALIZER_SCOUT.to_string()]),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(has_finalizer(&ingress));
    }

    #[test]
    fn test_has_finalizer_absent() {
        let ingress = Ingress {
            metadata: kube::api::ObjectMeta {
                finalizers: Some(vec!["other.io/finalizer".to_string()]),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!has_finalizer(&ingress));
    }

    #[test]
    fn test_has_finalizer_none() {
        let ingress = Ingress {
            metadata: kube::api::ObjectMeta {
                finalizers: None,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!has_finalizer(&ingress));
    }

    #[test]
    fn test_has_finalizer_among_others() {
        let ingress = Ingress {
            metadata: kube::api::ObjectMeta {
                finalizers: Some(vec![
                    "other.io/finalizer".to_string(),
                    FINALIZER_SCOUT.to_string(),
                    "another.io/finalizer".to_string(),
                ]),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(has_finalizer(&ingress));
    }

    // =========================================================================
    // is_being_deleted
    // =========================================================================

    #[test]
    fn test_is_being_deleted_with_timestamp() {
        let ingress = Ingress {
            metadata: kube::api::ObjectMeta {
                deletion_timestamp: Some(Time(k8s_openapi::jiff::Timestamp::now())),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(is_being_deleted(&ingress));
    }

    #[test]
    fn test_is_being_deleted_without_timestamp() {
        let ingress = Ingress {
            metadata: kube::api::ObjectMeta {
                deletion_timestamp: None,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!is_being_deleted(&ingress));
    }

    // =========================================================================
    // arecord_label_selector
    // =========================================================================

    #[test]
    fn test_arecord_label_selector_format() {
        let selector = arecord_label_selector("prod", "my-ns", "my-ingress");
        assert_eq!(
            selector,
            format!(
                "{}={},{}=prod,{}=my-ns,{}=my-ingress",
                LABEL_MANAGED_BY,
                LABEL_MANAGED_BY_SCOUT,
                LABEL_SOURCE_CLUSTER,
                LABEL_SOURCE_NAMESPACE,
                LABEL_SOURCE_NAME,
            )
        );
    }

    #[test]
    fn test_arecord_label_selector_contains_all_keys() {
        let selector = arecord_label_selector("prod", "ns", "ing");
        assert!(selector.contains(LABEL_MANAGED_BY));
        assert!(selector.contains(LABEL_SOURCE_CLUSTER));
        assert!(selector.contains(LABEL_SOURCE_NAMESPACE));
        assert!(selector.contains(LABEL_SOURCE_NAME));
    }

    // =========================================================================
    // stale_arecord_label_selector
    // =========================================================================

    #[test]
    fn test_stale_arecord_label_selector_uses_not_equal_for_cluster() {
        // Must use != so it matches ARecords from any previous cluster name,
        // regardless of what that name was.
        let selector = stale_arecord_label_selector("new-cluster", "my-ns", "my-ingress");
        assert!(
            selector.contains(&format!("{}!=new-cluster", LABEL_SOURCE_CLUSTER)),
            "selector must use != for source-cluster: got {selector}"
        );
    }

    #[test]
    fn test_stale_arecord_label_selector_still_filters_by_managed_by() {
        let selector = stale_arecord_label_selector("new-cluster", "my-ns", "my-ingress");
        assert!(
            selector.contains(&format!("{}={}", LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT)),
            "selector must still filter managed-by=scout"
        );
    }

    #[test]
    fn test_stale_arecord_label_selector_contains_namespace_and_ingress() {
        let selector = stale_arecord_label_selector("new-cluster", "my-ns", "my-ingress");
        assert!(selector.contains(&format!("{}=my-ns", LABEL_SOURCE_NAMESPACE)));
        assert!(selector.contains(&format!("{}=my-ingress", LABEL_SOURCE_NAME)));
    }

    #[test]
    fn test_stale_arecord_label_selector_does_not_match_current_cluster() {
        // The whole point: current cluster is excluded, not selected.
        let selector = stale_arecord_label_selector("current", "ns", "ing");
        // Must NOT contain an equality match on the current cluster
        assert!(
            !selector.contains(&format!("{}=current", LABEL_SOURCE_CLUSTER)),
            "selector must NOT positively select current cluster"
        );
    }

    // =========================================================================
    // is_scout_opted_in (accepts scout-enabled: "true" OR recordKind: "ARecord")
    // =========================================================================

    #[test]
    fn test_is_scout_opted_in_via_scout_enabled_true() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/scout-enabled".to_string(),
            "true".to_string(),
        );
        assert!(is_scout_opted_in(&annotations));
    }

    #[test]
    fn test_is_scout_opted_in_via_record_kind_arecord() {
        // Backward-compatible: old annotation still opts in
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/recordKind".to_string(),
            "ARecord".to_string(),
        );
        assert!(is_scout_opted_in(&annotations));
    }

    #[test]
    fn test_is_scout_opted_in_false_when_neither() {
        let annotations = BTreeMap::new();
        assert!(!is_scout_opted_in(&annotations));
    }

    #[test]
    fn test_is_scout_opted_in_false_scout_enabled_false() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/scout-enabled".to_string(),
            "false".to_string(),
        );
        assert!(!is_scout_opted_in(&annotations));
    }

    #[test]
    fn test_is_scout_opted_in_false_wrong_value() {
        // Must be exactly "true" — "yes", "1", etc. are rejected
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/scout-enabled".to_string(),
            "yes".to_string(),
        );
        assert!(!is_scout_opted_in(&annotations));
    }

    // =========================================================================
    // resolve_zone (annotation override → default zone → None)
    // =========================================================================

    #[test]
    fn test_resolve_zone_from_annotation() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/zone".to_string(),
            "example.com".to_string(),
        );
        assert_eq!(
            resolve_zone(&annotations, None),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_resolve_zone_from_default() {
        let annotations = BTreeMap::new();
        assert_eq!(
            resolve_zone(&annotations, Some("default.com")),
            Some("default.com".to_string())
        );
    }

    #[test]
    fn test_resolve_zone_annotation_wins_over_default() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/zone".to_string(),
            "explicit.com".to_string(),
        );
        assert_eq!(
            resolve_zone(&annotations, Some("default.com")),
            Some("explicit.com".to_string())
        );
    }

    #[test]
    fn test_resolve_zone_none_when_no_annotation_and_no_default() {
        let annotations = BTreeMap::new();
        assert_eq!(resolve_zone(&annotations, None), None);
    }

    #[test]
    fn test_resolve_zone_empty_annotation_falls_back_to_default() {
        // Empty annotation treated as absent — falls back to default zone
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/zone".to_string(), "".to_string());
        assert_eq!(
            resolve_zone(&annotations, Some("default.com")),
            Some("default.com".to_string())
        );
    }

    // =========================================================================
    // is_loadbalancer_service
    // =========================================================================

    fn service_with_type(svc_type: &str) -> Service {
        Service {
            spec: Some(ServiceSpec {
                type_: Some(svc_type.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_is_loadbalancer_service_true() {
        assert!(is_loadbalancer_service(&service_with_type("LoadBalancer")));
    }

    #[test]
    fn test_is_loadbalancer_service_false_clusterip() {
        assert!(!is_loadbalancer_service(&service_with_type("ClusterIP")));
    }

    #[test]
    fn test_is_loadbalancer_service_false_nodeport() {
        assert!(!is_loadbalancer_service(&service_with_type("NodePort")));
    }

    #[test]
    fn test_is_loadbalancer_service_false_no_spec() {
        let svc = Service::default();
        assert!(!is_loadbalancer_service(&svc));
    }

    // =========================================================================
    // resolve_ip_from_service_lb_status
    // =========================================================================

    fn service_with_lb_ip(ip: &str) -> Service {
        Service {
            status: Some(ServiceStatus {
                load_balancer: Some(LoadBalancerStatus {
                    ingress: Some(vec![ServiceLoadBalancerIngress {
                        ip: Some(ip.to_string()),
                        hostname: None,
                        ..Default::default()
                    }]),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_resolve_ip_from_service_lb_status_has_ip() {
        let svc = service_with_lb_ip("1.2.3.4");
        assert_eq!(
            resolve_ip_from_service_lb_status(&svc),
            Some("1.2.3.4".to_string())
        );
    }

    #[test]
    fn test_resolve_ip_from_service_lb_status_empty_list() {
        let svc = Service {
            status: Some(ServiceStatus {
                load_balancer: Some(LoadBalancerStatus {
                    ingress: Some(vec![]),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(resolve_ip_from_service_lb_status(&svc), None);
    }

    #[test]
    fn test_resolve_ip_from_service_lb_status_no_status() {
        let svc = Service::default();
        assert_eq!(resolve_ip_from_service_lb_status(&svc), None);
    }

    #[test]
    fn test_resolve_ip_from_service_lb_status_hostname_only() {
        // hostname-only LB entries have no IP — should return None
        let svc = Service {
            status: Some(ServiceStatus {
                load_balancer: Some(LoadBalancerStatus {
                    ingress: Some(vec![ServiceLoadBalancerIngress {
                        ip: None,
                        hostname: Some("my-lb.example.com".to_string()),
                        ..Default::default()
                    }]),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(resolve_ip_from_service_lb_status(&svc), None);
    }

    // =========================================================================
    // service_arecord_cr_name
    // =========================================================================

    #[test]
    fn test_service_arecord_cr_name_format() {
        // scout-{cluster}-{namespace}-{service} — no index suffix
        let name = service_arecord_cr_name("prod", "default", "my-svc");
        assert_eq!(name, "scout-prod-default-my-svc");
    }

    #[test]
    fn test_service_arecord_cr_name_sanitizes_underscores() {
        let name = service_arecord_cr_name("prod", "my_namespace", "my_service");
        assert_eq!(name, "scout-prod-my-namespace-my-service");
    }

    #[test]
    fn test_service_arecord_cr_name_lowercase() {
        let name = service_arecord_cr_name("PROD", "Default", "My-Service");
        assert_eq!(name, name.to_lowercase());
    }

    #[test]
    fn test_service_arecord_cr_name_max_253_chars() {
        let long_cluster = "a".repeat(100);
        let long_ns = "b".repeat(100);
        let long_svc = "c".repeat(100);
        let name = service_arecord_cr_name(&long_cluster, &long_ns, &long_svc);
        assert!(
            name.len() <= 253,
            "CR name exceeded 253 chars: {} chars",
            name.len()
        );
    }

    // =========================================================================
    // service_arecord_label_selector
    // =========================================================================

    #[test]
    fn test_service_arecord_label_selector_format() {
        let selector = service_arecord_label_selector("prod", "my-ns", "my-svc");
        assert_eq!(
            selector,
            format!(
                "{}={},{}=prod,{}=my-ns,{}=my-svc",
                LABEL_MANAGED_BY,
                LABEL_MANAGED_BY_SCOUT,
                LABEL_SOURCE_CLUSTER,
                LABEL_SOURCE_NAMESPACE,
                LABEL_SOURCE_NAME,
            )
        );
    }

    #[test]
    fn test_service_arecord_label_selector_contains_all_keys() {
        let selector = service_arecord_label_selector("prod", "ns", "svc");
        assert!(selector.contains(LABEL_MANAGED_BY));
        assert!(selector.contains(LABEL_SOURCE_CLUSTER));
        assert!(selector.contains(LABEL_SOURCE_NAMESPACE));
        assert!(selector.contains(LABEL_SOURCE_NAME));
    }

    // =========================================================================
    // build_service_arecord
    // =========================================================================

    #[test]
    fn test_build_service_arecord_sets_source_service_label() {
        let arecord = build_service_arecord(ServiceARecordParams {
            name: "scout-prod-default-my-svc",
            target_namespace: "bindy-system",
            record_name: "my-svc",
            ips: &["1.2.3.4".to_string()],
            ttl: None,
            cluster_name: "prod",
            service_namespace: "default",
            service_name: "my-svc",
            zone: "example.com",
        });

        let labels = arecord.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get(LABEL_SOURCE_NAME).map(String::as_str),
            Some("my-svc")
        );
    }

    #[test]
    fn test_build_service_arecord_sets_all_expected_labels() {
        let arecord = build_service_arecord(ServiceARecordParams {
            name: "scout-prod-default-my-svc",
            target_namespace: "bindy-system",
            record_name: "my-svc",
            ips: &["10.0.0.1".to_string()],
            ttl: Some(300),
            cluster_name: "prod",
            service_namespace: "default",
            service_name: "my-svc",
            zone: "example.com",
        });

        let labels = arecord.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get(LABEL_MANAGED_BY).map(String::as_str),
            Some(LABEL_MANAGED_BY_SCOUT)
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_CLUSTER).map(String::as_str),
            Some("prod")
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_NAMESPACE).map(String::as_str),
            Some("default")
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_NAME).map(String::as_str),
            Some("my-svc")
        );
        assert_eq!(
            labels.get(LABEL_ZONE).map(String::as_str),
            Some("example.com")
        );
    }

    #[test]
    fn test_build_service_arecord_uses_record_name_in_spec() {
        let arecord = build_service_arecord(ServiceARecordParams {
            name: "scout-prod-default-my-svc",
            target_namespace: "bindy-system",
            record_name: "my-svc",
            ips: &["1.2.3.4".to_string()],
            ttl: None,
            cluster_name: "prod",
            service_namespace: "default",
            service_name: "my-svc",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.name, "my-svc");
        assert_eq!(arecord.spec.ipv4_addresses, vec!["1.2.3.4".to_string()]);
        assert_eq!(arecord.spec.ttl, None);
    }

    // =========================================================================
    // HTTPRoute & TLSRoute (Gateway API) — Phase 2 feature
    // =========================================================================

    #[test]
    fn test_httproute_arecord_cr_name_single_hostname() {
        // Arrange & Act
        let name = crate::scout::httproute_arecord_cr_name("prod", "default", "api-route", 0);

        // Assert
        assert!(name.starts_with("scout-"));
        assert!(name.contains("prod"));
        assert!(name.contains("default"));
        assert!(name.contains("api-route"));
        assert!(name.contains("0")); // index suffix
        assert!(!name.ends_with('-'));
        assert!(name.len() <= 253); // K8s name length limit
    }

    #[test]
    fn test_httproute_arecord_cr_name_multiple_indices() {
        // Arrange & Act
        let name0 = crate::scout::httproute_arecord_cr_name("prod", "default", "api-route", 0);
        let name1 = crate::scout::httproute_arecord_cr_name("prod", "default", "api-route", 1);
        let name2 = crate::scout::httproute_arecord_cr_name("prod", "default", "api-route", 2);

        // Assert — each index produces a different CR name
        assert_ne!(name0, name1);
        assert_ne!(name1, name2);
        // Verify indices are in the names
        assert!(name0.contains("0"));
        assert!(name1.contains("1"));
        assert!(name2.contains("2"));
    }

    #[test]
    fn test_httproute_arecord_cr_name_sanitization() {
        // Arrange & Act
        let name =
            crate::scout::httproute_arecord_cr_name("prod_cluster", "kube-system", "My-Route", 5);

        // Assert — underscores and uppercase should be sanitized
        assert!(!name.contains("_"));
        assert!(!name.contains('K'));
        assert!(!name.contains('M'));
        assert!(!name.ends_with('-'));
    }

    #[test]
    fn test_tlsroute_arecord_cr_name_single_hostname() {
        // Arrange & Act
        let name = crate::scout::tlsroute_arecord_cr_name("prod", "default", "secure-route", 0);

        // Assert
        assert!(name.starts_with("scout-"));
        assert!(name.contains("prod"));
        assert!(name.contains("default"));
        assert!(name.contains("secure-route"));
        assert!(name.contains("0"));
        assert!(!name.ends_with('-'));
        assert!(name.len() <= 253);
    }

    #[test]
    fn test_tlsroute_arecord_cr_name_multiple_indices() {
        // Arrange & Act
        let name0 = crate::scout::tlsroute_arecord_cr_name("prod", "default", "secure-route", 0);
        let name1 = crate::scout::tlsroute_arecord_cr_name("prod", "default", "secure-route", 1);

        // Assert
        assert_ne!(name0, name1);
        assert!(name0.contains("0"));
        assert!(name1.contains("1"));
    }

    #[test]
    fn test_httproute_arecord_label_selector() {
        // Arrange & Act
        let selector =
            crate::scout::httproute_arecord_label_selector("prod", "default", "api-route");

        // Assert
        assert!(selector.contains(LABEL_MANAGED_BY));
        assert!(selector.contains(LABEL_MANAGED_BY_SCOUT));
        assert!(selector.contains("prod"));
        assert!(selector.contains("default"));
        assert!(selector.contains("api-route"));
        assert!(selector.contains("source-name"));
    }

    #[test]
    fn test_tlsroute_arecord_label_selector() {
        // Arrange & Act
        let selector =
            crate::scout::tlsroute_arecord_label_selector("prod", "default", "secure-route");

        // Assert
        assert!(selector.contains(LABEL_MANAGED_BY));
        assert!(selector.contains(LABEL_MANAGED_BY_SCOUT));
        assert!(selector.contains("prod"));
        assert!(selector.contains("default"));
        assert!(selector.contains("secure-route"));
        assert!(selector.contains("source-name"));
    }

    #[test]
    fn test_httproute_and_tlsroute_selectors_share_source_name_label() {
        // Arrange & Act
        let http_selector =
            crate::scout::httproute_arecord_label_selector("prod", "default", "route");
        let tls_selector =
            crate::scout::tlsroute_arecord_label_selector("prod", "default", "route");

        // Both use the unified source-name label — selectors are structurally identical
        assert_eq!(http_selector, tls_selector);
        assert!(http_selector.contains("source-name=route"));
    }

    #[test]
    fn test_stale_httproute_arecord_label_selector_uses_not_equal() {
        // Arrange & Act
        let selector = crate::scout::stale_httproute_arecord_label_selector(
            "new-cluster",
            "default",
            "api-route",
        );

        // Assert
        assert!(selector.contains(&format!("{}!=new-cluster", LABEL_SOURCE_CLUSTER)));
        assert!(selector.contains("source-name=api-route"));
        assert!(selector.contains("default"));
    }

    #[test]
    fn test_stale_tlsroute_arecord_label_selector_uses_not_equal() {
        // Arrange & Act
        let selector = crate::scout::stale_tlsroute_arecord_label_selector(
            "new-cluster",
            "default",
            "secure-route",
        );

        // Assert
        assert!(selector.contains(&format!("{}!=new-cluster", LABEL_SOURCE_CLUSTER)));
        assert!(selector.contains("source-name=secure-route"));
        assert!(selector.contains("default"));
    }

    #[test]
    fn test_build_httproute_arecord_sets_expected_labels() {
        // Arrange
        let params = crate::scout::HTTPRouteARecordParams {
            name: "scout-prod-default-api-route-0",
            target_namespace: "bindy-system",
            record_name: "api",
            ips: &["10.0.0.1".to_string()],
            ttl: Some(300),
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "api-route",
            zone: "example.com",
        };

        // Act
        let arecord = crate::scout::build_httproute_arecord(params);

        // Assert
        let labels = arecord.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get(LABEL_MANAGED_BY).map(String::as_str),
            Some(LABEL_MANAGED_BY_SCOUT)
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_CLUSTER).map(String::as_str),
            Some("prod")
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_NAMESPACE).map(String::as_str),
            Some("default")
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_NAME).map(String::as_str),
            Some("api-route")
        );
        assert_eq!(
            labels.get(LABEL_ZONE).map(String::as_str),
            Some("example.com")
        );
    }

    #[test]
    fn test_build_httproute_arecord_uses_record_name_in_spec() {
        // Arrange
        let params = crate::scout::HTTPRouteARecordParams {
            name: "scout-prod-default-api-route-0",
            target_namespace: "bindy-system",
            record_name: "api",
            ips: &["1.2.3.4".to_string()],
            ttl: None,
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "api-route",
            zone: "example.com",
        };

        // Act
        let arecord = crate::scout::build_httproute_arecord(params);

        // Assert
        assert_eq!(arecord.spec.name, "api");
        assert_eq!(arecord.spec.ipv4_addresses, vec!["1.2.3.4".to_string()]);
        assert_eq!(arecord.spec.ttl, None);
    }

    #[test]
    fn test_build_tlsroute_arecord_sets_expected_labels() {
        // Arrange
        let params = crate::scout::TLSRouteARecordParams {
            name: "scout-prod-default-secure-route-0",
            target_namespace: "bindy-system",
            record_name: "secure",
            ips: &["10.0.0.2".to_string()],
            ttl: Some(600),
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "secure-route",
            zone: "example.com",
        };

        // Act
        let arecord = crate::scout::build_tlsroute_arecord(params);

        // Assert
        let labels = arecord.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get(LABEL_MANAGED_BY).map(String::as_str),
            Some(LABEL_MANAGED_BY_SCOUT)
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_CLUSTER).map(String::as_str),
            Some("prod")
        );
        assert_eq!(
            labels.get(LABEL_SOURCE_NAME).map(String::as_str),
            Some("secure-route")
        );
        assert_eq!(
            labels.get(LABEL_ZONE).map(String::as_str),
            Some("example.com")
        );
    }

    #[test]
    fn test_build_tlsroute_arecord_uses_record_name_in_spec() {
        // Arrange
        let params = crate::scout::TLSRouteARecordParams {
            name: "scout-prod-default-secure-route-0",
            target_namespace: "bindy-system",
            record_name: "secure",
            ips: &["5.6.7.8".to_string()],
            ttl: Some(900),
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "secure-route",
            zone: "example.com",
        };

        // Act
        let arecord = crate::scout::build_tlsroute_arecord(params);

        // Assert
        assert_eq!(arecord.spec.name, "secure");
        assert_eq!(arecord.spec.ipv4_addresses, vec!["5.6.7.8".to_string()]);
        assert_eq!(arecord.spec.ttl, Some(900));
    }

    #[test]
    fn test_httproute_arecord_cr_name_respects_length_limit() {
        // Arrange — a very long HTTPRoute name
        let long_route_name = "my-long-route-name-that-is-very-very-very-very-very-very-very-very-very-very-very-very-very-very-long";

        // Act
        let name = crate::scout::httproute_arecord_cr_name("prod", "default", long_route_name, 99);

        // Assert
        assert!(
            name.len() <= 253,
            "HTTPRoute CR name must not exceed 253 chars"
        );
    }

    #[test]
    fn test_tlsroute_arecord_cr_name_respects_length_limit() {
        // Arrange — a very long TLSRoute name
        let long_route_name = "my-long-route-name-that-is-very-very-very-very-very-very-very-very-very-very-very-very-very-very-long";

        // Act
        let name = crate::scout::tlsroute_arecord_cr_name("prod", "default", long_route_name, 99);

        // Assert
        assert!(
            name.len() <= 253,
            "TLSRoute CR name must not exceed 253 chars"
        );
    }
}
