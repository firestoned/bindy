// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `scout.rs` — pure helper functions (no Kubernetes API calls)

#[cfg(test)]
mod tests {
    use crate::scout::{
        arecord_cr_name, arecord_label_selector, derive_record_name, get_zone_annotation,
        has_finalizer, is_arecord_enabled, is_being_deleted, resolve_ip_from_annotation,
        FINALIZER_SCOUT, LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT, LABEL_SOURCE_CLUSTER,
        LABEL_SOURCE_INGRESS, LABEL_SOURCE_NAMESPACE,
    };
    use k8s_openapi::api::networking::v1::Ingress;
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
                LABEL_SOURCE_INGRESS,
            )
        );
    }

    #[test]
    fn test_arecord_label_selector_contains_all_keys() {
        let selector = arecord_label_selector("prod", "ns", "ing");
        assert!(selector.contains(LABEL_MANAGED_BY));
        assert!(selector.contains(LABEL_SOURCE_CLUSTER));
        assert!(selector.contains(LABEL_SOURCE_NAMESPACE));
        assert!(selector.contains(LABEL_SOURCE_INGRESS));
    }
}
