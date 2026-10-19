// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `scout.rs` — pure helper functions, plus
//! `gateway_api_available`, which is exercised against a `wiremock` server
//! rather than a live cluster.

#[cfg(test)]
mod tests {
    use crate::crd::DNSZone;
    use crate::pipeline::{
        add_scout_finalizer, log_unscoped_stale_cluster_arecord_candidates, planned_records,
        remove_scout_finalizer, SourceRef,
    };
    use crate::scout::{
        arecord_cr_name, arecord_label_selector, authorizing_zone, check_zone_authorization,
        cleanup_grace_expired, derive_record_name, env_or, gateway_addresses_as_ips,
        gateway_parent_refs, get_record_name_annotation, get_zone_annotation, is_arecord_enabled,
        is_being_deleted, is_loadbalancer_service, is_scout_opted_in, is_valid_zone_label_value,
        legacy_arecord_label_selector, parse_gateway_service_entry, parse_gateway_services,
        parse_ttl_annotation, resolve_ip_from_lb_status, resolve_ip_from_service_lb_status,
        resolve_ips_from_annotation, resolve_record_name, resolve_usable_zone, resolve_zone,
        retain_ipv4, service_ref_from_str, source_identity_hash, stale_arecord_label_selector,
        stale_cleanup_zones, stale_selector_base, zone_allows_source_namespace,
        zone_namespace_grant, Gateway, GatewayServiceTarget, NamespaceGrant, NamespacedName,
        ParentReference, ScoutCli, ScoutConfig, StaleKind, ZoneAuthz, ZoneResolution,
        ANNOTATION_TTL, FINALIZER_SCOUT, LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT,
        LABEL_SOURCE_CLUSTER, LABEL_SOURCE_KIND, LABEL_SOURCE_NAME, LABEL_SOURCE_NAMESPACE,
        LABEL_ZONE, REMOTE_CLEANUP_GRACE_SECS,
    };
    use crate::scout::{kind_served, HTTPRoute, TLSRoute};
    use k8s_openapi::jiff::{SignedDuration, Timestamp};
    use kube::Api;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Fixed reference instant (2026-07-19T12:00:00Z) for deterministic
    /// grace-period tests.
    fn fixed_now() -> Timestamp {
        "2026-07-19T12:00:00Z".parse().unwrap()
    }

    /// `now` shifted by `secs` seconds (negative = into the past).
    fn shifted(now: Timestamp, secs: i64) -> Timestamp {
        now + SignedDuration::from_secs(secs)
    }

    #[test]
    fn test_cleanup_grace_not_expired_when_not_being_deleted() {
        // No deletion timestamp → nothing is terminating, so never "expired".
        assert!(!cleanup_grace_expired(None, fixed_now()));
    }

    #[test]
    fn test_cleanup_grace_not_expired_within_window() {
        let now = fixed_now();
        let started = shifted(now, -(REMOTE_CLEANUP_GRACE_SECS - 1));
        assert!(!cleanup_grace_expired(Some(&Time(started)), now));
    }

    #[test]
    fn test_cleanup_grace_expired_at_boundary() {
        // Exactly at the grace boundary counts as expired (>=).
        let now = fixed_now();
        let started = shifted(now, -REMOTE_CLEANUP_GRACE_SECS);
        assert!(cleanup_grace_expired(Some(&Time(started)), now));
    }

    #[test]
    fn test_cleanup_grace_expired_past_window() {
        let now = fixed_now();
        let started = shifted(now, -(REMOTE_CLEANUP_GRACE_SECS + 60));
        assert!(cleanup_grace_expired(Some(&Time(started)), now));
    }

    #[test]
    fn test_cleanup_grace_not_expired_for_future_timestamp() {
        // Clock skew: a deletion timestamp in the future must not read as expired.
        let now = fixed_now();
        let started = shifted(now, 10);
        assert!(!cleanup_grace_expired(Some(&Time(started)), now));
    }

    /// Build a minimal valid `DNSZone` fixture in `namespace` with the given
    /// annotations JSON object (`serde_json::Value::Null` for none).
    fn zone_fixture(namespace: &str, annotations: serde_json::Value) -> DNSZone {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "DNSZone",
            "metadata": { "name": "z", "namespace": namespace, "annotations": annotations },
            "spec": {
                "zoneName": "example.com",
                "soaRecord": {
                    "primaryNs": "ns1.example.com.",
                    "adminEmail": "admin.example.com.",
                    "serial": 2_024_010_101_u32,
                    "refresh": 3600,
                    "retry": 600,
                    "expire": 604_800,
                    "negativeTtl": 86_400
                }
            }
        }))
        .expect("valid DNSZone json")
    }

    // =========================================================================
    // H1: Scout zone-ownership gate — zone_allows_source_namespace
    // =========================================================================

    #[test]
    fn zone_in_same_namespace_is_authorized() {
        let zone = zone_fixture("tenant-a", serde_json::Value::Null);
        assert!(zone_allows_source_namespace(&zone, "tenant-a"));
    }

    #[test]
    fn zone_in_other_namespace_without_annotation_is_denied() {
        let zone = zone_fixture("bindy-system", serde_json::Value::Null);
        assert!(!zone_allows_source_namespace(&zone, "tenant-a"));
    }

    #[test]
    fn zone_annotation_listing_namespace_authorizes() {
        let zone = zone_fixture(
            "bindy-system",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "tenant-x,tenant-a" }),
        );
        assert!(zone_allows_source_namespace(&zone, "tenant-a"));
        assert!(!zone_allows_source_namespace(&zone, "tenant-b"));
    }

    #[test]
    fn zone_annotation_wildcard_authorizes_any_namespace() {
        let zone = zone_fixture(
            "bindy-system",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "*" }),
        );
        assert!(zone_allows_source_namespace(&zone, "any-tenant"));
    }

    // =========================================================================
    // H1: check_zone_authorization
    // =========================================================================

    #[test]
    fn check_zone_authorization_authorized_when_annotation_allows() {
        let zones = vec![Arc::new(zone_fixture(
            "bindy-system",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "tenant-a" }),
        ))];
        assert_eq!(
            check_zone_authorization(&zones, "example.com", "tenant-a"),
            ZoneAuthz::Authorized
        );
    }

    #[test]
    fn check_zone_authorization_forbidden_when_zone_exists_but_not_allowed() {
        let zones = vec![Arc::new(zone_fixture(
            "bindy-system",
            serde_json::Value::Null,
        ))];
        assert_eq!(
            check_zone_authorization(&zones, "example.com", "tenant-a"),
            ZoneAuthz::Forbidden
        );
    }

    #[test]
    fn check_zone_authorization_not_found_when_no_matching_zone() {
        let zones = vec![Arc::new(zone_fixture(
            "bindy-system",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "*" }),
        ))];
        assert_eq!(
            check_zone_authorization(&zones, "other.com", "tenant-a"),
            ZoneAuthz::NotFound
        );
    }
    use k8s_openapi::api::core::v1::{
        LoadBalancerIngress as ServiceLoadBalancerIngress, LoadBalancerStatus, Service,
        ServiceSpec, ServiceStatus,
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
    // resolve_ips_from_annotation
    // =========================================================================

    #[test]
    fn test_resolve_ips_from_annotation_single() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/ip".to_string(), "10.0.0.1".to_string());
        assert_eq!(
            resolve_ips_from_annotation(&annotations),
            Some(vec!["10.0.0.1".to_string()])
        );
    }

    #[test]
    fn test_resolve_ips_from_annotation_missing() {
        let annotations = BTreeMap::new();
        assert_eq!(resolve_ips_from_annotation(&annotations), None);
    }

    #[test]
    fn test_resolve_ips_from_annotation_empty_value() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/ip".to_string(), "".to_string());
        assert_eq!(resolve_ips_from_annotation(&annotations), None);
    }

    // --- P3-4: identify WHICH zone granted access, so it can be re-verified ---

    #[test]
    fn test_authorizing_zone_returns_the_granting_zone() {
        let same_ns = zone_fixture("tenant-a", serde_json::json!({}));
        let zones = vec![std::sync::Arc::new(same_ns)];
        let got = authorizing_zone(&zones, "example.com", "tenant-a");
        assert!(got.is_some(), "a same-namespace zone must be the grantor");
    }

    #[test]
    fn test_authorizing_zone_none_when_forbidden() {
        // Zone exists but does not authorize this source namespace — there is no
        // grantor to re-verify, so the caller keeps the Forbidden decision.
        let zone = zone_fixture("platform", serde_json::json!({}));
        let zones = vec![std::sync::Arc::new(zone)];
        assert!(authorizing_zone(&zones, "example.com", "tenant-a").is_none());
    }

    #[test]
    fn test_authorizing_zone_none_when_zone_name_does_not_match() {
        let zone = zone_fixture("tenant-a", serde_json::json!({}));
        let zones = vec![std::sync::Arc::new(zone)];
        assert!(authorizing_zone(&zones, "other.com", "tenant-a").is_none());
    }

    #[test]
    fn test_authorizing_zone_picks_the_wildcard_grantor() {
        let zone = zone_fixture(
            "platform",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "*" }),
        );
        let zones = vec![std::sync::Arc::new(zone)];
        let got = authorizing_zone(&zones, "example.com", "any-tenant");
        assert!(
            got.is_some(),
            "a wildcard grant still identifies a grantor to re-verify"
        );
        assert_eq!(got.unwrap().metadata.namespace.as_deref(), Some("platform"));
    }

    // --- P3-3: make the cross-namespace wildcard grant observable ---
    //
    // `allow-zone-namespaces: "*"` re-opens the cross-tenant path that H1 closed.
    // It stays supported, but the operator must be able to tell WHY a grant was
    // made so a wildcard can be warned about (and audited) rather than looking
    // identical to an explicit grant.

    #[test]
    fn test_zone_namespace_grant_same_namespace() {
        let zone = zone_fixture("tenant-a", serde_json::json!({}));
        assert_eq!(
            zone_namespace_grant(&zone, "tenant-a"),
            NamespaceGrant::SameNamespace
        );
    }

    #[test]
    fn test_zone_namespace_grant_explicit_listing() {
        let zone = zone_fixture(
            "platform",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "tenant-a,tenant-b" }),
        );
        assert_eq!(
            zone_namespace_grant(&zone, "tenant-a"),
            NamespaceGrant::ExplicitlyListed
        );
        assert_eq!(
            zone_namespace_grant(&zone, "tenant-c"),
            NamespaceGrant::Denied
        );
    }

    #[test]
    fn test_zone_namespace_grant_wildcard_is_distinguishable() {
        let zone = zone_fixture(
            "platform",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "*" }),
        );
        // The grant is allowed, but reported as Wildcard so it can be warned about.
        assert_eq!(
            zone_namespace_grant(&zone, "any-tenant"),
            NamespaceGrant::Wildcard
        );
        assert!(zone_allows_source_namespace(&zone, "any-tenant"));
    }

    #[test]
    fn test_zone_namespace_grant_same_namespace_wins_over_wildcard() {
        // A zone in the source's own namespace is authorized on that basis, not the
        // wildcard — so it must NOT produce a cross-namespace warning.
        let zone = zone_fixture(
            "tenant-a",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "*" }),
        );
        assert_eq!(
            zone_namespace_grant(&zone, "tenant-a"),
            NamespaceGrant::SameNamespace
        );
    }

    #[test]
    fn test_zone_namespace_grant_explicit_wins_over_wildcard() {
        // "*" alongside an explicit entry: the explicit match is reported, since the
        // grant would have been made regardless of the wildcard.
        let zone = zone_fixture(
            "platform",
            serde_json::json!({ "bindy.firestoned.io/allow-zone-namespaces": "tenant-a,*" }),
        );
        assert_eq!(
            zone_namespace_grant(&zone, "tenant-a"),
            NamespaceGrant::ExplicitlyListed
        );
        assert_eq!(
            zone_namespace_grant(&zone, "tenant-z"),
            NamespaceGrant::Wildcard
        );
    }

    #[test]
    fn test_zone_namespace_grant_no_annotation_is_denied() {
        let zone = zone_fixture("platform", serde_json::json!({}));
        assert_eq!(
            zone_namespace_grant(&zone, "tenant-a"),
            NamespaceGrant::Denied
        );
    }

    // --- P2-7: the `record-name` override is tenant-controlled ---
    //
    // The override deliberately bypasses host->zone matching (the annotator is
    // trusted to pick the name), but the VALUE still lands in a zone file, so it
    // must at least be a syntactically legal DNS name.

    fn record_name_annotations(value: &str) -> BTreeMap<String, String> {
        let mut a = BTreeMap::new();
        a.insert(
            "bindy.firestoned.io/record-name".to_string(),
            value.to_string(),
        );
        a
    }

    #[test]
    fn test_resolve_record_name_accepts_legal_overrides() {
        for good in ["myapp", "@", "a.b", "web-01", "*", "*.api", "_sip._tcp"] {
            let got = resolve_record_name(
                &record_name_annotations(good),
                "x.example.com",
                "example.com",
            );
            assert!(got.is_ok(), "{good:?} should be accepted, got {got:?}");
            assert_eq!(got.unwrap(), good);
        }
    }

    #[test]
    fn test_resolve_record_name_rejects_zone_file_injection() {
        // A newline lets the value close the current record and start another.
        let err = resolve_record_name(
            &record_name_annotations("ok\n@ IN NS evil.example.com."),
            "x.example.com",
            "example.com",
        );
        assert!(err.is_err(), "newline in override must be rejected");
    }

    #[test]
    fn test_resolve_record_name_rejects_spaces_and_control_chars() {
        for bad in ["has space", "tab\there", "semi;colon", "quote\"here"] {
            assert!(
                resolve_record_name(
                    &record_name_annotations(bad),
                    "x.example.com",
                    "example.com"
                )
                .is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn test_resolve_record_name_rejects_malformed_labels() {
        for bad in ["-leading", "trailing-", "a..b", ".leading-dot", "a.-b"] {
            assert!(
                resolve_record_name(
                    &record_name_annotations(bad),
                    "x.example.com",
                    "example.com"
                )
                .is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn test_resolve_record_name_rejects_overlong_label_and_name() {
        let long_label = "a".repeat(64);
        assert!(
            resolve_record_name(
                &record_name_annotations(&long_label),
                "x.example.com",
                "example.com"
            )
            .is_err(),
            "a 64-character label exceeds the DNS limit of 63"
        );

        // 63-char labels are fine individually; 4 of them exceed the 253-char name cap.
        let long_name = vec!["a".repeat(63); 4].join(".");
        assert!(
            resolve_record_name(
                &record_name_annotations(&long_name),
                "x.example.com",
                "example.com"
            )
            .is_err(),
            "a name over 253 characters must be rejected"
        );
    }

    #[test]
    fn test_resolve_record_name_without_override_still_derives() {
        // No annotation -> unchanged behaviour: derive from host, error if out of zone.
        let empty = BTreeMap::new();
        assert_eq!(
            resolve_record_name(&empty, "web.example.com", "example.com").unwrap(),
            "web"
        );
        assert!(resolve_record_name(&empty, "web.other.com", "example.com").is_err());
    }

    // --- P2-6: the `bindy.firestoned.io/ip` annotation is tenant-controlled ---
    //
    // Whatever survives here is written verbatim into `ARecordSpec.ipv4Addresses`
    // and rendered into a zone file. A tenant who can annotate an Ingress/Service
    // must not be able to put arbitrary text there.

    #[test]
    fn test_resolve_ips_from_annotation_rejects_non_ipv4() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "not-an-ip".to_string(),
        );
        assert_eq!(
            resolve_ips_from_annotation(&annotations),
            None,
            "a non-IP annotation must not reach ARecordSpec"
        );
    }

    #[test]
    fn test_resolve_ips_from_annotation_rejects_out_of_range_octets() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "999.999.999.999".to_string(),
        );
        assert_eq!(resolve_ips_from_annotation(&annotations), None);
    }

    #[test]
    fn test_resolve_ips_from_annotation_rejects_ipv6_for_a_record() {
        // ARecord is IPv4-only; an IPv6 literal here would render an invalid A record.
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "2001:db8::1".to_string(),
        );
        assert_eq!(resolve_ips_from_annotation(&annotations), None);
    }

    #[test]
    fn test_resolve_ips_from_annotation_drops_invalid_keeps_valid() {
        // A partially-bad list keeps the good entries rather than failing the whole
        // record — one fat-fingered entry should not take down valid DNS.
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "10.0.0.1,bogus,10.0.0.2".to_string(),
        );
        assert_eq!(
            resolve_ips_from_annotation(&annotations),
            Some(vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()])
        );
    }

    #[test]
    fn test_resolve_ips_from_annotation_all_invalid_is_none() {
        // All-invalid must be None, not Some(vec![]) — None lets resolve_ips fall
        // through to default_ips / LB status; an empty Vec would create a record
        // with no addresses.
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "bogus,also-bogus".to_string(),
        );
        assert_eq!(resolve_ips_from_annotation(&annotations), None);
    }

    #[test]
    fn test_resolve_ips_from_annotation_rejects_injection_text() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "10.0.0.1 IN NS evil.example.com.".to_string(),
        );
        assert_eq!(resolve_ips_from_annotation(&annotations), None);
    }

    #[test]
    fn test_resolve_ips_from_annotation_comma_separated() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "10.0.0.1,10.0.0.2,10.0.0.3".to_string(),
        );
        assert_eq!(
            resolve_ips_from_annotation(&annotations),
            Some(vec![
                "10.0.0.1".to_string(),
                "10.0.0.2".to_string(),
                "10.0.0.3".to_string(),
            ])
        );
    }

    #[test]
    fn test_resolve_ips_from_annotation_trims_whitespace_around_entries() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "  10.0.0.1 ,  10.0.0.2  , 10.0.0.3 ".to_string(),
        );
        assert_eq!(
            resolve_ips_from_annotation(&annotations),
            Some(vec![
                "10.0.0.1".to_string(),
                "10.0.0.2".to_string(),
                "10.0.0.3".to_string(),
            ])
        );
    }

    #[test]
    fn test_resolve_ips_from_annotation_skips_empty_entries() {
        // Trailing/leading commas and double commas produce no IPs for those slots
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            ",10.0.0.1,,10.0.0.2,".to_string(),
        );
        assert_eq!(
            resolve_ips_from_annotation(&annotations),
            Some(vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()])
        );
    }

    #[test]
    fn test_resolve_ips_from_annotation_only_commas_returns_none() {
        let mut annotations = BTreeMap::new();
        annotations.insert("bindy.firestoned.io/ip".to_string(), ",,,".to_string());
        assert_eq!(resolve_ips_from_annotation(&annotations), None);
    }

    #[test]
    fn test_resolve_ips_from_annotation_preserves_order_and_duplicates() {
        // Order is significant for round-robin DNS; duplicates are passed through verbatim
        // (validation lives in the ARecord layer).
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "10.0.0.2,10.0.0.1,10.0.0.2".to_string(),
        );
        assert_eq!(
            resolve_ips_from_annotation(&annotations),
            Some(vec![
                "10.0.0.2".to_string(),
                "10.0.0.1".to_string(),
                "10.0.0.2".to_string(),
            ])
        );
    }

    // =========================================================================
    // arecord_cr_name
    // =========================================================================

    // =========================================================================
    // resolve_ips (priority: annotation > default_ips > lb_status)
    // =========================================================================

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

    // =========================================================================
    // stale_arecord_label_selector
    // =========================================================================

    // ------------------------------------------------------------------
    // is_valid_zone_label_value
    //
    // The resolved zone (from an unvalidated annotation) is interpolated
    // directly into a labelSelector on the stale-cluster delete/opt-out
    // paths. A value that is legal DNS but not a legal Kubernetes label
    // value (trailing dot, over 63 chars) must be rejected here so the
    // caller falls back to the skip path instead of sending a `list` that
    // the API server rejects with a 400.
    // ------------------------------------------------------------------

    #[test]
    fn test_is_valid_zone_label_value_accepts_plain_domain() {
        assert!(is_valid_zone_label_value("example.com"));
    }

    #[test]
    fn test_is_valid_zone_label_value_accepts_hyphens_and_underscores() {
        assert!(is_valid_zone_label_value("my-zone_1.example.com"));
    }

    #[test]
    fn test_is_valid_zone_label_value_rejects_empty() {
        assert!(!is_valid_zone_label_value(""));
    }

    #[test]
    fn test_is_valid_zone_label_value_rejects_trailing_dot() {
        // DNS-idiomatic (fully-qualified), but not a legal label value.
        assert!(!is_valid_zone_label_value("example.com."));
    }

    #[test]
    fn test_is_valid_zone_label_value_rejects_leading_dot() {
        assert!(!is_valid_zone_label_value(".example.com"));
    }

    #[test]
    fn test_is_valid_zone_label_value_rejects_over_63_chars() {
        let too_long = format!("{}.com", "a".repeat(60));
        assert!(!is_valid_zone_label_value(&too_long));
    }

    #[test]
    fn test_is_valid_zone_label_value_accepts_exactly_63_chars() {
        let exactly = format!("{}m", "a".repeat(62));
        assert_eq!(exactly.len(), 63);
        assert!(is_valid_zone_label_value(&exactly));
    }

    #[test]
    fn test_is_valid_zone_label_value_rejects_illegal_characters() {
        assert!(!is_valid_zone_label_value("example.com/evil"));
        assert!(!is_valid_zone_label_value("example.com,other=value"));
        assert!(!is_valid_zone_label_value("example.com "));
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
    fn test_opt_out_edit_removing_all_annotations_also_removes_zone() {
        // A user opting out by deleting every bindy.firestoned.io/* annotation
        // in one edit removes the opt-in and the zone in the same patch. The
        // reconciler must see both "not opted in" and "no zone" together on
        // this reconcile — there is no in-between state where the opt-out is
        // detected but the old zone annotation still lingers — which is
        // exactly the case that sends stale-cluster cleanup down the
        // no-usable-zone path (log_unscoped_stale_cluster_arecord_candidates)
        // instead of a normal zone-scoped delete.
        let annotations_after_edit: BTreeMap<String, String> = BTreeMap::new();
        assert!(!is_scout_opted_in(&annotations_after_edit));
        assert_eq!(
            resolve_zone(&annotations_after_edit, /* default_zone */ None),
            None
        );
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

    // =========================================================================
    // service_arecord_label_selector
    // =========================================================================

    // =========================================================================
    // build_service_arecord
    // =========================================================================

    // =========================================================================
    // HTTPRoute & TLSRoute (Gateway API), Phase 2 feature
    // =========================================================================

    // =========================================================================
    // Builder ↔ override-annotation contract
    //
    // These tests pin down what the reconcilers actually produce when the
    // `bindy.firestoned.io/record-name` and comma-separated `bindy.firestoned.io/ip`
    // annotations are resolved upstream and threaded into the builders. They
    // protect against regressions in `spec.name` and `spec.ipv4_addresses`
    // for all four source kinds (Ingress, Service, HTTPRoute, TLSRoute).
    // =========================================================================

    #[test]
    fn test_build_arecord_propagates_record_name_override() {
        // Simulates resolve_record_name returning the annotation override "myapp"
        let arecord = crate::scout::build_arecord(crate::scout::ARecordParams {
            name: "scout-prod-default-my-ingress-0",
            target_namespace: "bindy-system",
            record_name: "myapp",
            ips: &["10.0.0.1".to_string()],
            ttl: None,
            cluster_name: "prod",
            source_kind: "Ingress",
            source_namespace: "default",
            source_name: "my-ingress",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.name, "myapp");
        assert_eq!(arecord.spec.ipv4_addresses, vec!["10.0.0.1".to_string()]);
    }

    #[test]
    fn test_build_arecord_accepts_apex_record_name() {
        // record-name="@" must pass through to spec.name unchanged
        let arecord = crate::scout::build_arecord(crate::scout::ARecordParams {
            name: "scout-prod-default-apex-0",
            target_namespace: "bindy-system",
            record_name: "@",
            ips: &["10.0.0.1".to_string()],
            ttl: None,
            cluster_name: "prod",
            source_kind: "Ingress",
            source_namespace: "default",
            source_name: "apex",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.name, "@");
    }

    #[test]
    fn test_build_arecord_propagates_multi_ip_in_order() {
        // Simulates resolve_ips_from_annotation returning a 3-IP list; order preserved
        let ips = vec![
            "10.0.0.1".to_string(),
            "10.0.0.2".to_string(),
            "10.0.0.3".to_string(),
        ];
        let arecord = crate::scout::build_arecord(crate::scout::ARecordParams {
            name: "scout-prod-default-multi-0",
            target_namespace: "bindy-system",
            record_name: "multi",
            ips: &ips,
            ttl: None,
            cluster_name: "prod",
            source_kind: "Ingress",
            source_namespace: "default",
            source_name: "multi",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.ipv4_addresses, ips);
    }

    // ========================================================================
    // Gateway-chain IP resolution: pure helpers
    // ========================================================================

    fn gateway_fixture(class: &str, addresses: serde_json::Value) -> Gateway {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "gateway.networking.k8s.io/v1",
            "kind": "Gateway",
            "metadata": { "name": "gw", "namespace": "traefik" },
            "spec": { "gatewayClassName": class },
            "status": { "addresses": addresses },
        }))
        .expect("valid Gateway fixture")
    }

    #[test]
    fn test_service_ref_from_str_valid() {
        let r = service_ref_from_str("traefik/traefik").expect("parses");
        assert_eq!(r.namespace, "traefik");
        assert_eq!(r.name, "traefik");
    }

    #[test]
    fn test_service_ref_from_str_trims_whitespace() {
        let r = service_ref_from_str("  traefik / traefik  ").expect("parses");
        assert_eq!(r.namespace, "traefik");
        assert_eq!(r.name, "traefik");
    }

    #[test]
    fn test_service_ref_from_str_rejects_malformed() {
        assert!(service_ref_from_str("traefik").is_none()); // no slash
        assert!(service_ref_from_str("a/b/c").is_none()); // too many segments
        assert!(service_ref_from_str("/traefik").is_none()); // empty namespace
        assert!(service_ref_from_str("traefik/").is_none()); // empty name
        assert!(service_ref_from_str("").is_none());
    }

    #[test]
    fn test_parse_gateway_services_valid_map_by_name() {
        let m = parse_gateway_services("traefik=traefik/traefik,cilium=kube-system/cilium-gw")
            .expect("valid map");
        assert_eq!(m.len(), 2);
        assert_eq!(
            m.get("traefik"),
            Some(&GatewayServiceTarget::Name(NamespacedName {
                namespace: "traefik".to_string(),
                name: "traefik".to_string()
            }))
        );
        assert_eq!(
            m.get("cilium"),
            Some(&GatewayServiceTarget::Name(NamespacedName {
                namespace: "kube-system".to_string(),
                name: "cilium-gw".to_string()
            }))
        );
    }

    #[test]
    fn test_parse_gateway_services_by_label_selector() {
        // A target whose Service part contains `=` is a label selector, scoped to
        // the namespace before the first `/`. The selector's own `/` is preserved.
        let m = parse_gateway_services("traefik=traefik/app.kubernetes.io/name=traefik")
            .expect("valid map");
        assert_eq!(
            m.get("traefik"),
            Some(&GatewayServiceTarget::Labeled {
                namespace: "traefik".to_string(),
                selector: "app.kubernetes.io/name=traefik".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_gateway_service_entry_multi_label_selector() {
        // Multi-label selectors contain commas; parsed per-entry (the CLI path)
        // so the commas stay inside the selector.
        let (class, target) =
            parse_gateway_service_entry("traefik=traefik/app=traefik,tier=edge").expect("parses");
        assert_eq!(class, "traefik");
        assert_eq!(
            target,
            GatewayServiceTarget::Labeled {
                namespace: "traefik".to_string(),
                selector: "app=traefik,tier=edge".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_gateway_services_rejects_malformed_entries() {
        // ADR-0022 Decision 6: a malformed entry fails startup instead of being
        // silently dropped. "bad" has no target; "=x/y" has an empty class;
        // "dup=onlyname" has no ns/name slash and no `=` selector.
        for bad in ["bad", "=x/y", "dup=onlyname"] {
            let err = parse_gateway_services(&format!("traefik=traefik/traefik,{bad}"))
                .expect_err("a malformed entry must be rejected");
            assert!(err.to_string().contains(bad), "{err}");
        }
    }

    #[test]
    fn test_parse_gateway_services_empty() {
        assert!(parse_gateway_services("")
            .expect("empty is valid")
            .is_empty());
        assert!(parse_gateway_services("   ")
            .expect("blank is valid")
            .is_empty());
    }

    #[test]
    fn test_gateway_addresses_as_ips_typed_ipaddress() {
        let gw = gateway_fixture(
            "traefik",
            serde_json::json!([{ "type": "IPAddress", "value": "203.0.113.5" }]),
        );
        assert_eq!(
            gateway_addresses_as_ips(&gw),
            vec!["203.0.113.5".to_string()]
        );
    }

    #[test]
    fn test_gateway_addresses_as_ips_untyped_but_parses_as_ip() {
        // type omitted; value is a valid IP → accepted.
        let gw = gateway_fixture("traefik", serde_json::json!([{ "value": "198.51.100.7" }]));
        assert_eq!(
            gateway_addresses_as_ips(&gw),
            vec!["198.51.100.7".to_string()]
        );
    }

    #[test]
    fn test_gateway_addresses_as_ips_skips_hostname() {
        let gw = gateway_fixture(
            "traefik",
            serde_json::json!([
                { "type": "Hostname", "value": "lb.example.com" },
                { "type": "IPAddress", "value": "203.0.113.9" }
            ]),
        );
        // Hostname entry ignored; only the IP is returned.
        assert_eq!(
            gateway_addresses_as_ips(&gw),
            vec!["203.0.113.9".to_string()]
        );
    }

    #[test]
    fn test_gateway_addresses_as_ips_empty_when_no_status() {
        let gw = gateway_fixture("traefik", serde_json::json!([]));
        assert!(gateway_addresses_as_ips(&gw).is_empty());
    }

    #[test]
    fn test_gateway_parent_refs_defaults_namespace_to_route() {
        let refs = vec![ParentReference {
            group: Some("gateway.networking.k8s.io".to_string()),
            kind: Some("Gateway".to_string()),
            namespace: None,
            name: "gw".to_string(),
        }];
        let out = gateway_parent_refs(&refs, "app-ns");
        assert_eq!(
            out,
            vec![NamespacedName {
                namespace: "app-ns".to_string(),
                name: "gw".to_string()
            }]
        );
    }

    #[test]
    fn test_gateway_parent_refs_honors_explicit_namespace() {
        let refs = vec![ParentReference {
            group: None,
            kind: None, // kind defaults to Gateway
            namespace: Some("traefik".to_string()),
            name: "gw".to_string(),
        }];
        let out = gateway_parent_refs(&refs, "app-ns");
        assert_eq!(out[0].namespace, "traefik");
        assert_eq!(out[0].name, "gw");
    }

    #[test]
    fn test_gateway_parent_refs_skips_non_gateway_kinds() {
        let refs = vec![
            ParentReference {
                group: Some("gateway.networking.k8s.io".to_string()),
                kind: Some("Mesh".to_string()),
                namespace: None,
                name: "mesh".to_string(),
            },
            ParentReference {
                group: Some("example.com".to_string()),
                kind: Some("Gateway".to_string()),
                namespace: None,
                name: "other".to_string(),
            },
        ];
        // Non-Gateway kind and non-Gateway-API group are both excluded.
        assert!(gateway_parent_refs(&refs, "app-ns").is_empty());
    }

    // -------------------------------------------------------------------------
    // resolve_ips_from_gateways — pure sub-logic tests
    //
    // The full async function requires a kube::Client and is not unit-tested
    // here. These tests verify the two decision paths it now takes:
    //
    //   Path 1 — Gateway has status.addresses → use them directly.
    //             No gateway_services configuration required.
    //   Path 2 — status.addresses is empty → consult gateway_services map.
    //             The class must be present in the map for fallback to work.
    //
    // Previously the function returned None immediately when gateway_services
    // was empty, blocking Path 1 entirely.
    // -------------------------------------------------------------------------

    #[test]
    fn test_gateway_status_addresses_resolved_without_gateway_services() {
        // Path 1: gateway has IPs in status.addresses — gateway_addresses_as_ips
        // must return them regardless of whether gateway_services is configured.
        let gw = gateway_fixture(
            "traefik",
            serde_json::json!([{"type": "IPAddress", "value": "10.186.195.24"}]),
        );
        let empty_services: std::collections::BTreeMap<String, GatewayServiceTarget> =
            std::collections::BTreeMap::new();

        let ips = gateway_addresses_as_ips(&gw);

        assert!(
            !ips.is_empty(),
            "status.addresses should be read even when gateway_services is empty"
        );
        assert_eq!(ips, vec!["10.186.195.24".to_string()]);
        // Empty map provides no fallback — but the status.addresses path doesn't need it.
        assert!(!empty_services.contains_key(&gw.spec.gateway_class_name));
    }

    #[test]
    fn test_gateway_fallback_to_services_when_no_status_addresses() {
        // Path 2: status.addresses is empty — the class must be in gateway_services
        // for the fallback LB-service lookup to proceed.
        let gw = gateway_fixture("traefik", serde_json::json!([]));

        assert!(
            gateway_addresses_as_ips(&gw).is_empty(),
            "fixture has no status.addresses"
        );
        let services = parse_gateway_services("traefik=traefik/traefik-ssc-01").expect("valid map");
        assert!(
            services.contains_key(&gw.spec.gateway_class_name),
            "class must be in gateway_services for LB-service fallback"
        );
    }

    #[test]
    fn test_gateway_fallback_skipped_when_class_not_in_services() {
        // Path 2 miss: status.addresses empty AND class absent from map → no IP.
        let gw = gateway_fixture("traefik", serde_json::json!([]));

        assert!(gateway_addresses_as_ips(&gw).is_empty());
        let services = parse_gateway_services("cilium=kube-system/cilium-gw").expect("valid map");
        assert!(
            !services.contains_key(&gw.spec.gateway_class_name),
            "traefik class absent from services map — no fallback"
        );
    }

    // ------------------------------------------------------------------
    // log_unscoped_stale_cluster_arecord_candidates
    //
    // Exercised on the delete/opt-out paths when no usable zone resolves.
    // It must ask for candidates with an unscoped (no zone clause) selector
    // (there is no zone to scope by), once for the source's kind and once
    // for legacy records, and it must never issue a DELETE: it is a
    // diagnostic, not a cleanup.
    // ------------------------------------------------------------------

    /// An ARecord, as the API server would return it in a list.
    fn arecord_item(name: &str, source_cluster: &str, zone: &str) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "ARecord",
            "metadata": {
                "name": name,
                "namespace": "bindy-system",
                "labels": {
                    LABEL_MANAGED_BY: LABEL_MANAGED_BY_SCOUT,
                    LABEL_SOURCE_CLUSTER: source_cluster,
                    LABEL_SOURCE_KIND: "Ingress",
                    LABEL_SOURCE_NAMESPACE: "team-checkout",
                    LABEL_SOURCE_NAME: "web-frontend",
                    LABEL_ZONE: zone,
                },
            },
            "spec": { "name": "web-frontend", "ipv4Addresses": ["10.0.0.1"] }
        })
    }

    fn arecord_list_body(items: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "ARecordList",
            "metadata": {},
            "items": items,
        })
    }

    const ARECORD_LIST_PATH: &str =
        "/apis/bindy.firestoned.io/v1beta1/namespaces/bindy-system/arecords";

    fn web_frontend() -> SourceRef {
        SourceRef {
            kind: "Ingress",
            namespace: "team-checkout".to_string(),
            name: "web-frontend".to_string(),
        }
    }

    #[tokio::test]
    async fn log_unscoped_stale_cluster_arecord_candidates_omits_zone_clause() {
        let server = MockServer::start().await;
        for kind_clause in [
            format!("{LABEL_SOURCE_KIND}=Ingress"),
            format!("!{LABEL_SOURCE_KIND}"),
        ] {
            Mock::given(method("GET"))
                .and(path(ARECORD_LIST_PATH))
                .and(wiremock::matchers::query_param(
                    "labelSelector",
                    format!(
                        "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}!=south,\
                         {kind_clause},{LABEL_SOURCE_NAMESPACE}=team-checkout,\
                         {LABEL_SOURCE_NAME}=web-frontend"
                    ),
                ))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(arecord_list_body(vec![
                        arecord_item("scout-north-x-0", "north", "zone-beta.example.internal"),
                    ])),
                )
                .expect(1)
                .mount(&server)
                .await;
        }

        log_unscoped_stale_cluster_arecord_candidates(
            &client_for(&server),
            "bindy-system",
            "south",
            &web_frontend(),
        )
        .await;
        // `.expect(1)` on both mocks is verified when `server` drops.
    }

    #[tokio::test]
    async fn log_unscoped_stale_cluster_arecord_candidates_never_deletes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ARECORD_LIST_PATH))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(arecord_list_body(vec![arecord_item(
                    "scout-north-x-0",
                    "north",
                    "zone-beta.example.internal",
                )])),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        log_unscoped_stale_cluster_arecord_candidates(
            &client_for(&server),
            "bindy-system",
            "south",
            &web_frontend(),
        )
        .await;
    }

    #[tokio::test]
    async fn log_unscoped_stale_cluster_arecord_candidates_swallows_list_errors() {
        // A best-effort diagnostic must never panic or propagate an error
        // that could block the finalizer removal it's called alongside.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ARECORD_LIST_PATH))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        log_unscoped_stale_cluster_arecord_candidates(
            &client_for(&server),
            "bindy-system",
            "south",
            &web_frontend(),
        )
        .await;
        // No panic: that's the assertion.
    }

    // =========================================================================
    // ADR-0022: record names, selectors, TTL, IPv4, record-name planning
    // =========================================================================

    #[test]
    fn arecord_cr_name_is_readable_prefix_hash_and_index() {
        let hash = source_identity_hash("prod", "Ingress", "default", "my-ingress");
        assert_eq!(hash.len(), 8);
        assert!(hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(
            arecord_cr_name("prod", "Ingress", "default", "my-ingress", 2),
            format!("scout-prod-default-my-ingress-{hash}-2")
        );
    }

    #[test]
    fn source_identity_hash_is_the_sha256_of_the_nul_joined_identity() {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(b"prod\0Ingress\0default\0my-ingress");
        let expected: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            source_identity_hash("prod", "Ingress", "default", "my-ingress"),
            expected
        );
    }

    #[test]
    fn arecord_cr_name_differs_by_kind() {
        let names: std::collections::BTreeSet<String> =
            ["Ingress", "Service", "HTTPRoute", "TLSRoute", "TCPRoute"]
                .iter()
                .map(|k| arecord_cr_name("prod", k, "default", "web", 0))
                .collect();
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn arecord_cr_name_differs_when_joined_namespace_and_name_collide() {
        let a = arecord_cr_name("prod", "Ingress", "team-a", "web", 0);
        let b = arecord_cr_name("prod", "Ingress", "team", "a-web", 0);
        assert_ne!(a, b);
        // The readable parts are equal: only the hash tells them apart.
        assert_eq!(a.rsplitn(3, '-').nth(2), b.rsplitn(3, '-').nth(2));
    }

    #[test]
    fn arecord_cr_name_sanitizes_and_lowercases() {
        let name = arecord_cr_name("PROD", "Ingress", "My_Namespace", "My.Ingress", 1);
        assert!(name.starts_with("scout-prod-my-namespace-my-ingress-"));
        assert_eq!(name, name.to_lowercase());
    }

    #[test]
    fn arecord_cr_name_fits_253_chars_and_keeps_hash_and_index() {
        let long = "c".repeat(300);
        let name = arecord_cr_name("prod", "Ingress", "default", &long, 7);
        assert_eq!(name.len(), 253);
        let hash = source_identity_hash("prod", "Ingress", "default", &long);
        assert!(name.ends_with(&format!("-{hash}-7")));
    }

    #[test]
    fn arecord_cr_name_never_leaves_a_hyphen_at_the_truncation_point() {
        // Try every cut position: a hyphen-separated readable part lands a
        // hyphen on the boundary for one of them.
        for pad in 0..20 {
            let name_part = format!("{}-{}", "a".repeat(200 + pad), "b".repeat(60));
            let name = arecord_cr_name("prod", "Ingress", "default", &name_part, 0);
            assert!(name.len() <= 253, "{} chars", name.len());
            assert!(!name.contains("--"), "double hyphen in {name}");
            assert!(name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
            assert!(!name.starts_with('-') && !name.ends_with('-'));
        }
    }

    #[test]
    fn own_selector_carries_the_source_kind() {
        assert_eq!(
            arecord_label_selector("prod", "HTTPRoute", "team-a", "web"),
            format!(
                "{LABEL_MANAGED_BY}={LABEL_MANAGED_BY_SCOUT},{LABEL_SOURCE_CLUSTER}=prod,\
                 {LABEL_SOURCE_KIND}=HTTPRoute,{LABEL_SOURCE_NAMESPACE}=team-a,{LABEL_SOURCE_NAME}=web"
            )
        );
    }

    #[test]
    fn legacy_selector_requires_the_source_kind_to_be_absent() {
        let selector = legacy_arecord_label_selector("prod", "team-a", "web");
        assert!(selector.contains(&format!(",!{LABEL_SOURCE_KIND},")));
        assert!(selector.contains(&format!("{LABEL_SOURCE_CLUSTER}=prod")));
        assert!(selector.contains(&format!("{LABEL_SOURCE_NAME}=web")));
    }

    #[test]
    fn stale_selector_excludes_the_current_cluster_and_scopes_kind_and_zone() {
        let s = stale_arecord_label_selector(
            "south",
            StaleKind::Kind("Service"),
            "team-a",
            "web",
            "example.com",
        );
        assert_eq!(
            s,
            format!(
                "{},{LABEL_ZONE}=example.com",
                stale_selector_base("south", StaleKind::Kind("Service"), "team-a", "web")
            )
        );
        assert!(s.contains(&format!("{LABEL_SOURCE_CLUSTER}!=south")));
        assert!(s.contains(&format!("{LABEL_SOURCE_KIND}=Service")));
        assert!(
            !stale_selector_base("south", StaleKind::Legacy, "team-a", "web").contains(LABEL_ZONE)
        );
        assert!(
            stale_selector_base("south", StaleKind::Legacy, "team-a", "web")
                .contains(&format!("!{LABEL_SOURCE_KIND}"))
        );
    }

    #[test]
    fn build_arecord_stamps_every_ownership_label() {
        let ar = crate::scout::build_arecord(crate::scout::ARecordParams {
            name: "n",
            target_namespace: "bindy-system",
            record_name: "api",
            ips: &["10.0.0.1".to_string()],
            ttl: Some(300),
            cluster_name: "prod",
            source_kind: "TLSRoute",
            source_namespace: "team-a",
            source_name: "web",
            zone: "example.com",
        });
        let labels = ar.metadata.labels.expect("labels");
        assert_eq!(labels[LABEL_MANAGED_BY], LABEL_MANAGED_BY_SCOUT);
        assert_eq!(labels[LABEL_SOURCE_CLUSTER], "prod");
        assert_eq!(labels[LABEL_SOURCE_KIND], "TLSRoute");
        assert_eq!(labels[LABEL_SOURCE_NAMESPACE], "team-a");
        assert_eq!(labels[LABEL_SOURCE_NAME], "web");
        assert_eq!(labels[LABEL_ZONE], "example.com");
        assert_eq!(ar.spec.ttl, Some(300));
    }

    fn ttl(value: &str) -> std::collections::BTreeMap<String, String> {
        [(ANNOTATION_TTL.to_string(), value.to_string())].into()
    }

    #[test]
    fn parse_ttl_annotation_accepts_absent_and_positive_values() {
        assert_eq!(
            parse_ttl_annotation(&BTreeMap::new()).expect("absent"),
            None
        );
        assert_eq!(parse_ttl_annotation(&ttl("300")).expect("300"), Some(300));
        assert_eq!(
            parse_ttl_annotation(&ttl(" 60 ")).expect("trimmed"),
            Some(60)
        );
    }

    #[test]
    fn parse_ttl_annotation_rejects_anything_but_a_positive_integer() {
        for bad in ["", "abc", "0", "-5", "1.5", "99999999999"] {
            assert!(
                parse_ttl_annotation(&ttl(bad)).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn retain_ipv4_keeps_only_ipv4_in_order() {
        let ips = ["10.0.0.2", "2001:db8::1", "not-an-ip", "10.0.0.1"].map(ToString::to_string);
        assert_eq!(retain_ipv4(ips, "test"), vec!["10.0.0.2", "10.0.0.1"]);
    }

    #[test]
    fn lb_status_ips_skip_non_ipv4_entries() {
        let ingress: Ingress = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "web"},
            "status": {"loadBalancer": {"ingress": [{"ip": "2001:db8::1"}, {"ip": "10.0.0.9"}]}}
        }))
        .expect("valid Ingress");
        assert_eq!(
            resolve_ip_from_lb_status(&ingress),
            Some("10.0.0.9".to_string())
        );
        let svc: Service = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "db"},
            "status": {"loadBalancer": {"ingress": [{"ip": "2001:db8::1"}]}}
        }))
        .expect("valid Service");
        assert_eq!(resolve_ip_from_service_lb_status(&svc), None);
    }

    #[test]
    fn gateway_addresses_skip_non_ipv4_entries() {
        let gw = gateway_fixture(
            "traefik",
            serde_json::json!([
                { "type": "IPAddress", "value": "2001:db8::1" },
                { "value": "2001:db8::2" },
                { "type": "IPAddress", "value": "203.0.113.5" }
            ]),
        );
        assert_eq!(
            gateway_addresses_as_ips(&gw),
            vec!["203.0.113.5".to_string()]
        );
    }

    fn hosts(v: &[(usize, &str)]) -> Vec<(usize, String)> {
        v.iter().map(|(i, h)| (*i, (*h).to_string())).collect()
    }

    #[test]
    fn planned_records_derive_names_and_skip_out_of_zone_hosts() {
        let planned = planned_records(
            "Ingress",
            hosts(&[
                (0, "api.example.com"),
                (2, "www.other.org"),
                (3, "example.com"),
            ]),
            &BTreeMap::new(),
            "example.com",
            false,
        )
        .expect("valid");
        assert_eq!(planned, vec![(0, "api".to_string()), (3, "@".to_string())]);
    }

    #[test]
    fn planned_records_fail_the_whole_source_on_an_invalid_override() {
        let ann = record_name_annotations("bad name");
        assert!(planned_records(
            "Ingress",
            hosts(&[(0, "api.example.com")]),
            &ann,
            "example.com",
            false
        )
        .is_err());
    }

    #[test]
    fn planned_records_require_an_override_when_the_kind_does() {
        assert!(planned_records(
            "TCPRoute",
            Vec::new(),
            &BTreeMap::new(),
            "example.com",
            true
        )
        .is_err());
        assert_eq!(
            planned_records(
                "TCPRoute",
                Vec::new(),
                &record_name_annotations("db"),
                "example.com",
                true
            )
            .expect("valid"),
            vec![(0, "db".to_string())]
        );
    }

    // =========================================================================
    // ADR-0022 Decision 6: startup configuration fails fast
    // =========================================================================

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: BTreeMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |var: &str| map.get(var).cloned()
    }

    fn base_env() -> Vec<(&'static str, &'static str)> {
        vec![
            ("BINDY_SCOUT_CLUSTER_NAME", "prod"),
            ("POD_NAMESPACE", "scout"),
        ]
    }

    fn config(vars: &[(&str, &str)], cli: ScoutCli) -> anyhow::Result<ScoutConfig> {
        let mut all = base_env();
        all.extend_from_slice(vars);
        ScoutConfig::from_lookup(cli, &env(&all))
    }

    #[test]
    fn env_or_prefers_the_flag_then_the_variable() {
        let lookup = env(&[("X", "from-env")]);
        assert_eq!(
            env_or(Some("from-cli".into()), "X", &lookup).expect("ok"),
            Some("from-cli".to_string())
        );
        assert_eq!(
            env_or(None, "X", &lookup).expect("ok"),
            Some("from-env".to_string())
        );
        assert_eq!(env_or(None, "UNSET", &lookup).expect("ok"), None);
    }

    #[test]
    fn env_or_rejects_empty_values() {
        let lookup = env(&[("X", "  ")]);
        assert!(env_or(None, "X", &lookup).is_err());
        assert!(env_or(Some(String::new()), "Y", &lookup).is_err());
    }

    #[test]
    fn a_valid_configuration_loads() {
        let c = config(
            &[
                ("BINDY_SCOUT_DEFAULT_IPS", "10.0.0.1, 10.0.0.2,"),
                ("BINDY_SCOUT_GATEWAY_SERVICES", "traefik=traefik/traefik,"),
                ("BINDY_SCOUT_EXCLUDE_NAMESPACES", "kube-system"),
            ],
            ScoutCli::default(),
        )
        .expect("valid configuration");
        assert_eq!(c.cluster_name, "prod");
        assert_eq!(c.default_ips, vec!["10.0.0.1", "10.0.0.2"]);
        assert!(c.gateway_services.contains_key("traefik"));
        assert_eq!(c.excluded_namespaces, vec!["kube-system", "scout"]);
        assert_eq!(c.target_namespace, "bindy-system");
    }

    #[test]
    fn an_empty_environment_value_fails_startup() {
        for var in [
            "BINDY_SCOUT_NAMESPACE",
            "BINDY_SCOUT_DEFAULT_ZONE",
            "BINDY_SCOUT_NAMESPACE_SELECTOR",
            "BINDY_SCOUT_DEFAULT_IPS",
            "BINDY_SCOUT_GATEWAY_SERVICES",
            "BINDY_SCOUT_EXCLUDE_NAMESPACES",
            "BINDY_SCOUT_REMOTE_SECRET",
            "BINDY_SCOUT_REMOTE_ENDPOINT",
        ] {
            let err = config(&[(var, "")], ScoutCli::default())
                .expect_err("an empty value must fail startup");
            assert!(err.to_string().contains(var), "{var}: {err}");
        }
    }

    #[test]
    fn an_empty_cluster_name_fails_startup() {
        let lookup = env(&[("BINDY_SCOUT_CLUSTER_NAME", "")]);
        assert!(ScoutConfig::from_lookup(ScoutCli::default(), &lookup).is_err());
    }

    #[test]
    fn a_malformed_gateway_service_fails_startup() {
        assert!(config(
            &[(
                "BINDY_SCOUT_GATEWAY_SERVICES",
                "traefik=traefik/traefik,bad"
            )],
            ScoutCli::default()
        )
        .is_err());
        let cli = ScoutCli {
            gateway_services: vec!["traefik=traefik/traefik".into(), "=x/y".into()],
            ..ScoutCli::default()
        };
        assert!(config(&[], cli).is_err());
    }

    #[test]
    fn a_non_ipv4_default_ip_fails_startup() {
        assert!(config(
            &[("BINDY_SCOUT_DEFAULT_IPS", "10.0.0.1,2001:db8::1")],
            ScoutCli::default()
        )
        .is_err());
        let cli = ScoutCli {
            default_ips: vec!["nope".into()],
            ..ScoutCli::default()
        };
        assert!(config(&[], cli).is_err());
    }

    #[test]
    fn a_cli_flag_overrides_its_variable() {
        let cli = ScoutCli {
            default_zone: Some("cli.example.com".into()),
            ..ScoutCli::default()
        };
        let c = config(&[("BINDY_SCOUT_DEFAULT_ZONE", "env.example.com")], cli).expect("valid");
        assert_eq!(c.default_zone.as_deref(), Some("cli.example.com"));
    }

    // =========================================================================
    // ADR-0025 decisions 1 and 2: zone and address allowlists
    // =========================================================================

    #[test]
    fn allowlists_are_unrestricted_when_unset() {
        let c = config(&[], ScoutCli::default()).expect("valid");
        assert_eq!(c.allowed_zones, None);
        assert!(c.allowed_ip_ranges.is_none());
    }

    #[test]
    fn allowlists_load_from_the_environment() {
        let c = config(
            &[
                (
                    "BINDY_SCOUT_ALLOWED_ZONES",
                    "Foo.Example.com., bar.example.com",
                ),
                ("BINDY_SCOUT_ALLOWED_IP_RANGES", "10.0.0.0/24,192.0.2.7"),
            ],
            ScoutCli::default(),
        )
        .expect("valid");
        assert_eq!(
            c.allowed_zones,
            Some(vec![
                "foo.example.com".to_string(),
                "bar.example.com".to_string()
            ])
        );
        assert_eq!(c.allowed_ip_ranges.map(|r| r.len()), Some(2));
    }

    #[test]
    fn allowlist_flags_override_their_variables() {
        let cli = ScoutCli {
            allowed_zones: Some("cli.example.com".into()),
            allowed_ip_ranges: Some("10.1.0.0/16".into()),
            ..ScoutCli::default()
        };
        let c = config(
            &[
                ("BINDY_SCOUT_ALLOWED_ZONES", "env.example.com"),
                ("BINDY_SCOUT_ALLOWED_IP_RANGES", "10.0.0.0/24,10.2.0.0/24"),
            ],
            cli,
        )
        .expect("valid");
        assert_eq!(c.allowed_zones, Some(vec!["cli.example.com".to_string()]));
        assert_eq!(c.allowed_ip_ranges.map(|r| r.len()), Some(1));
    }

    #[test]
    fn an_empty_allowlist_variable_fails_startup() {
        for var in ["BINDY_SCOUT_ALLOWED_ZONES", "BINDY_SCOUT_ALLOWED_IP_RANGES"] {
            let err = config(&[(var, "")], ScoutCli::default())
                .expect_err("an empty value must fail startup");
            assert!(err.to_string().contains(var), "{var}: {err}");
        }
    }

    #[test]
    fn a_wildcard_or_malformed_allowlist_fails_startup() {
        assert!(config(&[("BINDY_SCOUT_ALLOWED_ZONES", "*")], ScoutCli::default()).is_err());
        assert!(config(
            &[("BINDY_SCOUT_ALLOWED_IP_RANGES", "10.0.0.0/40")],
            ScoutCli::default()
        )
        .is_err());
    }

    #[test]
    fn a_default_zone_outside_the_allowed_zones_fails_startup() {
        let err = config(
            &[
                ("BINDY_SCOUT_ALLOWED_ZONES", "foo.example.com"),
                ("BINDY_SCOUT_DEFAULT_ZONE", "bar.example.com"),
            ],
            ScoutCli::default(),
        )
        .expect_err("default zone must be allowed");
        assert!(err.to_string().contains("bar.example.com"), "{err}");

        assert!(config(
            &[
                ("BINDY_SCOUT_ALLOWED_ZONES", "foo.example.com"),
                ("BINDY_SCOUT_DEFAULT_ZONE", "api.foo.example.com"),
            ],
            ScoutCli::default(),
        )
        .is_ok());
    }

    #[test]
    fn a_default_ip_outside_the_allowed_ranges_fails_startup() {
        let err = config(
            &[
                ("BINDY_SCOUT_ALLOWED_IP_RANGES", "10.0.0.0/24"),
                ("BINDY_SCOUT_DEFAULT_IPS", "10.0.0.1,10.9.9.9"),
            ],
            ScoutCli::default(),
        )
        .expect_err("default IPs must be inside the ranges");
        assert!(err.to_string().contains("10.9.9.9"), "{err}");
    }

    // ------------------------------------------------------------------
    // kind_served
    //
    // Gateway API is not installed by default in Kubernetes, and a
    // Controller started against an absent CRD retries forever rather than
    // failing — so the detection has to be right, and its three outcomes are
    // covered here.
    //
    // Note the probe is PER KIND. Gateway API ships in two channels and the
    // route kinds graduated at different times (HTTPRoute Standard since v1.0,
    // TLSRoute v1.5, TCPRoute v1.6), so one kind cannot stand in for another.
    // ------------------------------------------------------------------

    /// Paths kube derives for a cluster-wide list of each route kind.
    const HTTPROUTE_LIST_PATH: &str = "/apis/gateway.networking.k8s.io/v1/httproutes";
    const TLSROUTE_LIST_PATH: &str = "/apis/gateway.networking.k8s.io/v1alpha2/tlsroutes";

    /// A client pointed at a mock API server.
    fn client_for(server: &MockServer) -> kube::Client {
        let config = kube::Config::new(server.uri().parse().expect("mock server uri"));
        kube::Client::try_from(config).expect("client from mock config")
    }

    /// Mount a canned response for one path on the mock server.
    async fn mount(server: &MockServer, path_str: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(path_str))
            .respond_with(response)
            .mount(server)
            .await;
    }

    /// An empty list body for the given kind, as the API server would return.
    fn empty_list(kind: &str) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "gateway.networking.k8s.io/v1",
            "kind": format!("{kind}List"),
            "metadata": {},
            "items": [],
        })
    }

    #[tokio::test]
    async fn a_kind_is_absent_when_the_api_server_returns_a_plain_text_404() {
        // The REAL shape of this failure. A cluster without the CRDs answers
        // with `404 page not found` as plain text, not a JSON Status — which
        // is why kube logs "Unsuccessful data error parse" before it
        // reconstructs a Status carrying the code. Matching on the code has to
        // survive that reconstruction, so the body here is deliberately not
        // JSON.
        let server = MockServer::start().await;
        mount(
            &server,
            HTTPROUTE_LIST_PATH,
            ResponseTemplate::new(404).set_body_string("404 page not found"),
        )
        .await;

        let api: Api<HTTPRoute> = Api::all(client_for(&server));
        assert!(!kind_served(&api).await);
    }

    #[tokio::test]
    async fn a_kind_is_absent_when_the_404_is_a_json_status() {
        // The same conclusion via the other encoding, so the check does not
        // depend on which form the API server happens to use.
        let server = MockServer::start().await;
        mount(
            &server,
            HTTPROUTE_LIST_PATH,
            ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "kind": "Status",
                "apiVersion": "v1",
                "status": "Failure",
                "message": "the server could not find the requested resource",
                "reason": "NotFound",
                "code": 404,
            })),
        )
        .await;

        let api: Api<HTTPRoute> = Api::all(client_for(&server));
        assert!(!kind_served(&api).await);
    }

    #[tokio::test]
    async fn a_kind_is_present_when_the_list_succeeds() {
        let server = MockServer::start().await;
        mount(
            &server,
            HTTPROUTE_LIST_PATH,
            ResponseTemplate::new(200).set_body_json(empty_list("HTTPRoute")),
        )
        .await;

        let api: Api<HTTPRoute> = Api::all(client_for(&server));
        assert!(kind_served(&api).await);
    }

    #[tokio::test]
    async fn a_transient_server_error_does_not_disable_watching() {
        // Fail open. Treating a 500 as "absent" would silently stop watching
        // for the lifetime of the process because the API server was briefly
        // unwell at startup — a far worse outcome than the noise this
        // detection exists to remove.
        let server = MockServer::start().await;
        mount(
            &server,
            HTTPROUTE_LIST_PATH,
            ResponseTemplate::new(500).set_body_string("internal server error"),
        )
        .await;

        let api: Api<HTTPRoute> = Api::all(client_for(&server));
        assert!(kind_served(&api).await);
    }

    #[tokio::test]
    async fn each_route_kind_is_probed_independently() {
        // THE STANDARD-CHANNEL CASE, and the reason this probe is per kind.
        //
        // Gateway API ships in two channels. HTTPRoute has been Standard since
        // v1.0; TLSRoute only became Standard in v1.5 and TCPRoute in v1.6. A
        // standard-channel install older than those serves HTTPRoute and NOT
        // the others — common, since Traefik, Envoy Gateway and Cilium all pin
        // CRD versions.
        //
        // Probing HTTPRoute alone would report the Gateway API "present",
        // start all three controllers, and leave the TLS and TCP ones retrying
        // a 404 forever: precisely the failure this check was added to remove.
        let server = MockServer::start().await;
        mount(
            &server,
            HTTPROUTE_LIST_PATH,
            ResponseTemplate::new(200).set_body_json(empty_list("HTTPRoute")),
        )
        .await;
        mount(
            &server,
            TLSROUTE_LIST_PATH,
            ResponseTemplate::new(404).set_body_string("404 page not found"),
        )
        .await;

        let client = client_for(&server);
        let httproute_api: Api<HTTPRoute> = Api::all(client.clone());
        let tlsroute_api: Api<TLSRoute> = Api::all(client);

        assert!(kind_served(&httproute_api).await);
        assert!(!kind_served(&tlsroute_api).await);
    }

    // ========================================================================
    // Zone usability guard (reconcile paths)
    // ========================================================================

    /// A zone long enough to be a legal DNS name (each label <= 63) but longer
    /// than the 63-char Kubernetes label-value limit. This passes the DNSZone
    /// CRD's `zoneName` pattern, so it can reach Scout from a real cluster.
    const OVERLONG_ZONE: &str =
        "payments-gateway.team-checkout.production.eu-west-1.example.internal";

    fn annotations_with_zone(zone: &str) -> std::collections::BTreeMap<String, String> {
        let mut a = std::collections::BTreeMap::new();
        a.insert("bindy.firestoned.io/zone".to_string(), zone.to_string());
        a
    }

    #[test]
    fn resolve_usable_zone_accepts_a_legal_zone() {
        let annotations = annotations_with_zone("example.com");
        assert_eq!(
            resolve_usable_zone(&annotations, None),
            ZoneResolution::Usable("example.com".to_string())
        );
    }

    #[test]
    fn resolve_usable_zone_falls_back_to_the_operator_default() {
        let annotations = std::collections::BTreeMap::new();
        assert_eq!(
            resolve_usable_zone(&annotations, Some("default.example.com")),
            ZoneResolution::Usable("default.example.com".to_string())
        );
    }

    #[test]
    fn resolve_usable_zone_reports_missing_when_nothing_resolves() {
        let annotations = std::collections::BTreeMap::new();
        assert_eq!(
            resolve_usable_zone(&annotations, None),
            ZoneResolution::Missing
        );
    }

    #[test]
    fn resolve_usable_zone_rejects_a_zone_too_long_for_a_label_value() {
        // Regression: an over-long zone previously flowed straight into the
        // stale-cluster `labelSelector`, which the API server rejects with a
        // 400 — retried forever by the reconcile error path.
        assert!(OVERLONG_ZONE.len() > 63);
        let annotations = annotations_with_zone(OVERLONG_ZONE);
        assert_eq!(
            resolve_usable_zone(&annotations, None),
            ZoneResolution::Invalid(OVERLONG_ZONE.to_string())
        );
    }

    #[test]
    fn resolve_usable_zone_rejects_a_trailing_dot_zone() {
        let annotations = annotations_with_zone("example.com.");
        assert_eq!(
            resolve_usable_zone(&annotations, None),
            ZoneResolution::Invalid("example.com.".to_string())
        );
    }

    #[test]
    fn resolve_usable_zone_rejects_an_invalid_operator_default() {
        let annotations = std::collections::BTreeMap::new();
        assert_eq!(
            resolve_usable_zone(&annotations, Some(OVERLONG_ZONE)),
            ZoneResolution::Invalid(OVERLONG_ZONE.to_string())
        );
    }

    // ========================================================================
    // Stale-cleanup zone selection (delete / opt-out paths)
    // ========================================================================

    fn zone_set(zones: &[&str]) -> std::collections::BTreeSet<String> {
        zones.iter().map(|z| (*z).to_string()).collect()
    }

    #[test]
    fn stale_cleanup_zones_uses_the_resolved_zone_when_present() {
        let from_records = zone_set(&["example.com"]);
        assert_eq!(
            stale_cleanup_zones(Some("example.com"), &from_records),
            zone_set(&["example.com"])
        );
    }

    #[test]
    fn stale_cleanup_zones_falls_back_to_record_labels_when_annotations_are_gone() {
        // The opt-out case: the user stripped every bindy.firestoned.io/*
        // annotation in one edit, so no zone resolves — but the ARecords we
        // just deleted still carry an authoritative `zone` label.
        let from_records = zone_set(&["example.com"]);
        assert_eq!(
            stale_cleanup_zones(None, &from_records),
            zone_set(&["example.com"])
        );
    }

    #[test]
    fn stale_cleanup_zones_unions_resolved_and_record_zones() {
        // The operator default was changed after the records were written:
        // both the old zone (from labels) and the new one need cleaning.
        let from_records = zone_set(&["old.example.com"]);
        assert_eq!(
            stale_cleanup_zones(Some("new.example.com"), &from_records),
            zone_set(&["new.example.com", "old.example.com"])
        );
    }

    #[test]
    fn stale_cleanup_zones_drops_label_illegal_values() {
        let from_records = zone_set(&[OVERLONG_ZONE, "example.com"]);
        assert_eq!(
            stale_cleanup_zones(Some("example.com."), &from_records),
            zone_set(&["example.com"])
        );
    }

    #[test]
    fn stale_cleanup_zones_is_empty_when_nothing_is_usable() {
        let from_records = zone_set(&[OVERLONG_ZONE]);
        assert!(stale_cleanup_zones(None, &from_records).is_empty());
    }

    // ========================================================================
    // Shared stale-selector construction
    // ========================================================================

    // ========================================================================
    // delete_arecords_for_* reports the zones it removed
    // ========================================================================

    // ------------------------------------------------------------------
    // ADR-0008: remote transport resolution (roadmap 12 Phase 3)
    // ------------------------------------------------------------------

    use crate::scout::{build_endpoint_kubeconfig, resolve_remote_transport, RemoteTransport};

    const EP: &str = "https://bindy-api.mirror.svc.cluster.local:6443";
    const TOKEN_FILE: &str = "/var/run/secrets/bindy/remote-token";
    const CA_FILE: &str = "/var/run/secrets/bindy/remote-ca.crt";

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn test_resolve_remote_transport_same_cluster_default() {
        let t = resolve_remote_transport(None, None, None, None, "bindy-system")
            .expect("no remote config is valid");
        assert_eq!(t, RemoteTransport::SameCluster);
    }

    #[test]
    fn test_resolve_remote_transport_kubeconfig_secret() {
        let t = resolve_remote_transport(None, None, None, some("remote-kubeconfig"), "scout-ns")
            .expect("secret mode is valid");
        assert_eq!(
            t,
            RemoteTransport::KubeconfigSecret {
                name: "remote-kubeconfig".to_string(),
                namespace: "scout-ns".to_string(),
            }
        );
    }

    #[test]
    fn test_resolve_remote_transport_endpoint_with_token() {
        let t = resolve_remote_transport(some(EP), some(TOKEN_FILE), some(CA_FILE), None, "ns")
            .expect("endpoint mode is valid");
        assert_eq!(
            t,
            RemoteTransport::Endpoint {
                endpoint: EP.to_string(),
                token_file: TOKEN_FILE.to_string(),
                ca_file: Some(CA_FILE.to_string()),
            }
        );
    }

    #[test]
    fn test_resolve_remote_transport_endpoint_ca_optional() {
        let t = resolve_remote_transport(some(EP), some(TOKEN_FILE), None, None, "ns")
            .expect("CA file is optional (webpki roots)");
        assert_eq!(
            t,
            RemoteTransport::Endpoint {
                endpoint: EP.to_string(),
                token_file: TOKEN_FILE.to_string(),
                ca_file: None,
            }
        );
    }

    #[test]
    fn test_resolve_remote_transport_endpoint_requires_token_file() {
        let err = resolve_remote_transport(some(EP), None, None, None, "ns")
            .expect_err("endpoint without token file must fail closed");
        assert!(
            err.to_string().contains("BINDY_SCOUT_REMOTE_TOKEN_FILE"),
            "error must name the missing variable, got: {err}"
        );
    }

    #[test]
    fn test_resolve_remote_transport_endpoint_and_secret_conflict() {
        let err = resolve_remote_transport(
            some(EP),
            some(TOKEN_FILE),
            None,
            some("remote-kubeconfig"),
            "ns",
        )
        .expect_err("two remote modes at once is ambiguous and must fail closed");
        assert!(
            err.to_string().contains("BINDY_SCOUT_REMOTE_SECRET")
                && err.to_string().contains("BINDY_SCOUT_REMOTE_ENDPOINT"),
            "error must name both conflicting variables, got: {err}"
        );
    }

    #[test]
    fn test_resolve_remote_transport_credentials_without_endpoint() {
        let err = resolve_remote_transport(None, some(TOKEN_FILE), None, None, "ns")
            .expect_err("token file without an endpoint is a misconfiguration");
        assert!(err.to_string().contains("BINDY_SCOUT_REMOTE_ENDPOINT"));

        let err = resolve_remote_transport(None, None, some(CA_FILE), None, "ns")
            .expect_err("CA file without an endpoint is a misconfiguration");
        assert!(err.to_string().contains("BINDY_SCOUT_REMOTE_ENDPOINT"));
    }

    #[test]
    fn test_build_endpoint_kubeconfig_shape() {
        let kc = build_endpoint_kubeconfig(EP, TOKEN_FILE, Some(CA_FILE));

        let cluster = &kc.clusters[0];
        assert_eq!(
            cluster.cluster.as_ref().unwrap().server.as_deref(),
            Some(EP)
        );
        assert_eq!(
            cluster
                .cluster
                .as_ref()
                .unwrap()
                .certificate_authority
                .as_deref(),
            Some(CA_FILE)
        );

        let user = &kc.auth_infos[0];
        assert_eq!(
            user.auth_info.as_ref().unwrap().token_file.as_deref(),
            Some(TOKEN_FILE),
            "token must be file-based so kube-rs re-reads it on rotation"
        );

        // The context must tie the cluster and user together and be current.
        let context = &kc.contexts[0];
        let ctx = context.context.as_ref().unwrap();
        assert_eq!(ctx.cluster, cluster.name);
        assert_eq!(ctx.user.as_deref(), Some(user.name.as_str()));
        assert_eq!(kc.current_context.as_deref(), Some(context.name.as_str()));
    }

    #[test]
    fn test_build_endpoint_kubeconfig_without_ca() {
        let kc = build_endpoint_kubeconfig(EP, TOKEN_FILE, None);
        assert!(
            kc.clusters[0]
                .cluster
                .as_ref()
                .unwrap()
                .certificate_authority
                .is_none(),
            "no CA file means webpki public roots"
        );
    }

    #[tokio::test]
    async fn test_build_remote_client_from_endpoint_constructs() {
        // Client construction is lazy — no connection until a request goes out.
        let dir = std::env::temp_dir().join("scout-adr0008-test");
        std::fs::create_dir_all(&dir).unwrap();
        let token_path = dir.join("token");
        std::fs::write(&token_path, "test-token").unwrap();

        let client = crate::scout::build_remote_client_from_endpoint(
            "https://127.0.0.1:9",
            token_path.to_str().unwrap(),
            None,
        )
        .await;
        assert!(client.is_ok(), "client must build: {:?}", client.err());
    }

    // ------------------------------------------------------------------
    // B1: TCPRoute record-name override must pass the P2-7 syntax check.
    // TCPRoute has no hostnames, so the override is its only source of a
    // record name; it must get the same validation every other kind gets
    // through `resolve_record_name`.
    // ------------------------------------------------------------------

    #[test]
    fn tcproute_record_name_rejects_override_with_newline() {
        // A newline would let a tenant close the record and append another.
        let annotations = record_name_annotations("db\nevil 300 IN A 6.6.6.6");
        let result = crate::scout::tcproute_record_name(&annotations);
        assert!(
            result.is_err(),
            "an illegal override must not yield a record name, got {result:?}"
        );
    }

    #[test]
    fn tcproute_record_name_rejects_override_with_space() {
        let annotations = record_name_annotations("db evil");
        assert!(crate::scout::tcproute_record_name(&annotations).is_err());
    }

    #[test]
    fn tcproute_record_name_rejects_label_starting_with_hyphen() {
        let annotations = record_name_annotations("-db");
        assert!(crate::scout::tcproute_record_name(&annotations).is_err());
    }

    #[test]
    fn tcproute_record_name_accepts_valid_override() {
        let annotations = record_name_annotations("db.internal");
        assert_eq!(
            crate::scout::tcproute_record_name(&annotations).unwrap(),
            Some("db.internal".to_string())
        );
    }

    #[test]
    fn tcproute_record_name_accepts_apex() {
        let annotations = record_name_annotations("@");
        assert_eq!(
            crate::scout::tcproute_record_name(&annotations).unwrap(),
            Some("@".to_string())
        );
    }

    #[test]
    fn tcproute_record_name_absent_override_is_none() {
        let annotations = std::collections::BTreeMap::new();
        assert_eq!(
            crate::scout::tcproute_record_name(&annotations).unwrap(),
            None
        );
    }

    // ------------------------------------------------------------------
    // B11.1: prune ARecords for hosts that were not applied this pass.
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // B11.2: a Service that stops being LoadBalancer must be released like
    // an opt-out, not left publishing its old IP.
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // B11.3: Service stale-cluster cleanup after a --cluster-name change.
    // ------------------------------------------------------------------

    // =========================================================================
    // Scout finalizer patches are guarded (no clobbering a racing writer)
    //
    // The old helpers sent an unguarded merge patch of the whole
    // `metadata.finalizers` list, which overwrote any finalizer another
    // controller added or removed between Scout's read and its write. The
    // finalizer now goes through the SDK's JSON Patch, which `test`s the
    // state it read before changing it, so a concurrent edit makes the
    // server reject the patch instead of losing the other writer's change.
    // =========================================================================

    /// Path of one Ingress in the mock API server.
    const INGRESS_ITEM_PATH: &str = "/apis/networking.k8s.io/v1/namespaces/team-a/ingresses/web";

    /// Path of one HTTPRoute in the mock API server.
    const HTTPROUTE_ITEM_PATH: &str =
        "/apis/gateway.networking.k8s.io/v1/namespaces/team-a/httproutes/web";

    /// A finalizer some other controller owns.
    const FOREIGN_FINALIZER: &str = "other.example.io/finalizer";

    /// The `resourceVersion` Scout read the object at.
    const READ_RESOURCE_VERSION: &str = "42";

    /// The content type kube sends for a `Patch::Json`.
    const JSON_PATCH_CONTENT_TYPE: &str = "application/json-patch+json";

    /// An Ingress `team-a/web` carrying `finalizers` at `READ_RESOURCE_VERSION`.
    fn ingress_with_finalizers(finalizers: Option<Vec<String>>) -> Ingress {
        Ingress {
            metadata: kube::api::ObjectMeta {
                name: Some("web".to_string()),
                namespace: Some("team-a".to_string()),
                resource_version: Some(READ_RESOURCE_VERSION.to_string()),
                finalizers,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// An HTTPRoute `team-a/web` carrying `finalizers`.
    fn httproute_with_finalizers(finalizers: Option<Vec<String>>) -> HTTPRoute {
        HTTPRoute {
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            kind: "HTTPRoute".to_string(),
            metadata: kube::api::ObjectMeta {
                name: Some("web".to_string()),
                namespace: Some("team-a".to_string()),
                finalizers,
                ..Default::default()
            },
            spec: None,
        }
    }

    /// The API server's answer to a successful patch of Ingress `team-a/web`.
    fn patched_ingress_body() -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "networking.k8s.io/v1",
            "kind": "Ingress",
            "metadata": { "name": "web", "namespace": "team-a" },
        })
    }

    #[tokio::test]
    async fn adding_the_scout_finalizer_tests_the_resource_version_it_read() {
        // Another controller's finalizer is already there: JSON Patch cannot
        // test for "the list is unchanged", so the patch pins the
        // resourceVersion Scout read and appends, never rewriting the list.
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(INGRESS_ITEM_PATH))
            .and(wiremock::matchers::header(
                "content-type",
                JSON_PATCH_CONTENT_TYPE,
            ))
            .and(wiremock::matchers::body_json(serde_json::json!([
                { "op": "test", "path": "/metadata/resourceVersion", "value": READ_RESOURCE_VERSION },
                { "op": "add", "path": "/metadata/finalizers/-", "value": FINALIZER_SCOUT },
            ])))
            .respond_with(ResponseTemplate::new(200).set_body_json(patched_ingress_body()))
            .expect(1)
            .mount(&server)
            .await;

        let ingress = ingress_with_finalizers(Some(vec![FOREIGN_FINALIZER.to_string()]));
        let result = add_scout_finalizer(&client_for(&server), &ingress).await;

        assert!(result.is_ok(), "got {result:?}");
    }

    #[tokio::test]
    async fn adding_the_first_finalizer_tests_that_there_are_none() {
        // No finalizers yet: the patch tests the list is still absent, so a
        // writer that created it first is not overwritten.
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(INGRESS_ITEM_PATH))
            .and(wiremock::matchers::body_json(serde_json::json!([
                { "op": "test", "path": "/metadata/finalizers", "value": null },
                { "op": "add", "path": "/metadata/finalizers", "value": [FINALIZER_SCOUT] },
            ])))
            .respond_with(ResponseTemplate::new(200).set_body_json(patched_ingress_body()))
            .expect(1)
            .mount(&server)
            .await;

        let ingress = ingress_with_finalizers(None);
        let result = add_scout_finalizer(&client_for(&server), &ingress).await;

        assert!(result.is_ok(), "got {result:?}");
    }

    #[tokio::test]
    async fn adding_the_scout_finalizer_when_present_sends_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(200).set_body_json(patched_ingress_body()))
            .expect(0)
            .mount(&server)
            .await;

        let ingress = ingress_with_finalizers(Some(vec![FINALIZER_SCOUT.to_string()]));
        let result = add_scout_finalizer(&client_for(&server), &ingress).await;

        assert!(result.is_ok(), "got {result:?}");
    }

    #[tokio::test]
    async fn removing_the_scout_finalizer_from_a_route_tests_the_index_it_removes() {
        // The local Gateway API types go through the same SDK path: the patch
        // removes Scout's entry by index only if that index still holds it,
        // so a foreign finalizer that shifted the list is never dropped.
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(HTTPROUTE_ITEM_PATH))
            .and(wiremock::matchers::body_json(serde_json::json!([
                { "op": "test", "path": "/metadata/finalizers/1", "value": FINALIZER_SCOUT },
                { "op": "remove", "path": "/metadata/finalizers/1" },
            ])))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "apiVersion": "gateway.networking.k8s.io/v1",
                "kind": "HTTPRoute",
                "metadata": { "name": "web", "namespace": "team-a" },
            })))
            .expect(1)
            .mount(&server)
            .await;

        let route = httproute_with_finalizers(Some(vec![
            FOREIGN_FINALIZER.to_string(),
            FINALIZER_SCOUT.to_string(),
        ]));
        let result = remove_scout_finalizer(&client_for(&server), &route).await;

        assert!(result.is_ok(), "got {result:?}");
    }

    #[tokio::test]
    async fn removing_an_absent_scout_finalizer_sends_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(200).set_body_json(patched_ingress_body()))
            .expect(0)
            .mount(&server)
            .await;

        let route = httproute_with_finalizers(Some(vec![FOREIGN_FINALIZER.to_string()]));
        let result = remove_scout_finalizer(&client_for(&server), &route).await;

        assert!(result.is_ok(), "got {result:?}");
    }

    #[tokio::test]
    async fn a_rejected_finalizer_patch_is_an_error_the_reconcile_returns() {
        // A concurrent edit makes the server reject the guarded patch. That
        // must surface as an error (so the reconcile fails and its error
        // policy requeues with fresh state), carrying the API status so it
        // can still be classified.
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(INGRESS_ITEM_PATH))
            .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
                "kind": "Status",
                "apiVersion": "v1",
                "metadata": {},
                "status": "Failure",
                "message": "the object has been modified",
                "reason": "Conflict",
                "code": 409,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ingress = ingress_with_finalizers(Some(vec![FOREIGN_FINALIZER.to_string()]));
        let err = add_scout_finalizer(&client_for(&server), &ingress)
            .await
            .expect_err("a rejected patch must be an error");

        assert!(
            bindy_controller_sdk::kube_errors::is_conflict_anyhow(&err),
            "got {err:?}"
        );
    }

    #[test]
    fn is_being_deleted_works_for_any_kind() {
        let mut route = httproute_with_finalizers(None);
        assert!(!is_being_deleted(&route));

        route.metadata.deletion_timestamp = Some(Time(Timestamp::now()));
        assert!(is_being_deleted(&route));
    }
}
