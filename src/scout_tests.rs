// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `scout.rs` — pure helper functions, plus
//! `gateway_api_available`, which is exercised against a `wiremock` server
//! rather than a live cluster.

#[cfg(test)]
mod tests {
    use crate::crd::DNSZone;
    use crate::scout::{
        arecord_cr_name, arecord_label_selector, build_service_arecord, build_tcproute_arecord,
        check_zone_authorization, cleanup_grace_expired, derive_record_name,
        gateway_addresses_as_ips, gateway_parent_refs, get_record_name_annotation,
        get_zone_annotation, has_finalizer, is_arecord_enabled, is_being_deleted,
        is_loadbalancer_service, is_scout_opted_in, parse_gateway_service_entry,
        parse_gateway_services, resolve_ip_from_service_lb_status, resolve_ips,
        resolve_ips_from_annotation, resolve_record_name, resolve_zone, service_arecord_cr_name,
        service_arecord_label_selector, service_ref_from_str, stale_arecord_label_selector,
        stale_tcproute_arecord_label_selector, tcproute_arecord_cr_name,
        tcproute_arecord_label_selector, zone_allows_source_namespace, zone_namespace_grant,
        Gateway, GatewayServiceTarget, NamespaceGrant, NamespacedName, ParentReference,
        ServiceARecordParams, TCPRouteARecordParams, ZoneAuthz, FINALIZER_SCOUT, LABEL_MANAGED_BY,
        LABEL_MANAGED_BY_SCOUT, LABEL_SOURCE_CLUSTER, LABEL_SOURCE_NAME, LABEL_SOURCE_NAMESPACE,
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

    #[test]
    fn test_resolve_ips_annotation_multiple_wins_over_defaults() {
        // Multi-IP annotation flows through resolve_ips for the Ingress path
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/ip".to_string(),
            "1.2.3.4,5.6.7.8".to_string(),
        );
        let defaults = vec!["9.9.9.9".to_string()];
        let ingress = Ingress::default();
        assert_eq!(
            resolve_ips(&annotations, &defaults, &ingress),
            Some(vec!["1.2.3.4".to_string(), "5.6.7.8".to_string()])
        );
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
    fn test_tcproute_arecord_cr_name_single_hostname() {
        let name = tcproute_arecord_cr_name("prod", "default", "database-route", 0);

        assert!(name.starts_with("scout-"));
        assert!(name.contains("prod"));
        assert!(name.contains("default"));
        assert!(name.contains("database-route"));
        assert!(name.contains("0"));
        assert!(!name.ends_with('-'));
        assert!(name.len() <= 253);
    }

    #[test]
    fn test_tcproute_arecord_label_selector_uses_source_labels() {
        let selector = tcproute_arecord_label_selector("prod", "default", "database-route");

        assert!(selector.contains(LABEL_MANAGED_BY));
        assert!(selector.contains(LABEL_MANAGED_BY_SCOUT));
        assert!(selector.contains("prod"));
        assert!(selector.contains("default"));
        assert!(selector.contains("database-route"));
        assert!(selector.contains("source-name"));
    }

    #[test]
    fn test_stale_tcproute_arecord_label_selector_uses_not_equal() {
        let selector =
            stale_tcproute_arecord_label_selector("new-cluster", "default", "database-route");

        assert!(selector.contains(&format!("{}!=new-cluster", LABEL_SOURCE_CLUSTER)));
        assert!(selector.contains("source-name=database-route"));
        assert!(selector.contains("default"));
    }

    #[test]
    fn test_build_tcproute_arecord_sets_expected_labels() {
        let params = crate::scout::TCPRouteARecordParams {
            name: "scout-prod-default-database-route-0",
            target_namespace: "bindy-system",
            record_name: "db",
            ips: &["10.0.0.99".to_string()],
            ttl: Some(120),
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "database-route",
            zone: "example.com",
        };

        let arecord = crate::scout::build_tcproute_arecord(params);

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
            Some("database-route")
        );
        assert_eq!(
            labels.get(LABEL_ZONE).map(String::as_str),
            Some("example.com")
        );
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
            ingress_namespace: "default",
            ingress_name: "my-ingress",
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
            ingress_namespace: "default",
            ingress_name: "apex",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.name, "@");
    }

    #[test]
    fn test_build_arecord_propagates_multi_ip_in_order() {
        // Simulates resolve_ips_from_annotation returning a 3-IP list — order preserved
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
            ingress_namespace: "default",
            ingress_name: "multi",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.ipv4_addresses, ips);
    }

    #[test]
    fn test_build_service_arecord_propagates_multi_ip_in_order() {
        let ips = vec!["1.2.3.4".to_string(), "5.6.7.8".to_string()];
        let arecord = build_service_arecord(ServiceARecordParams {
            name: "scout-prod-default-my-svc",
            target_namespace: "bindy-system",
            record_name: "my-svc",
            ips: &ips,
            ttl: None,
            cluster_name: "prod",
            service_namespace: "default",
            service_name: "my-svc",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.ipv4_addresses, ips);
    }

    #[test]
    fn test_build_service_arecord_accepts_apex_record_name() {
        let arecord = build_service_arecord(ServiceARecordParams {
            name: "scout-prod-default-apex-svc",
            target_namespace: "bindy-system",
            record_name: "@",
            ips: &["1.2.3.4".to_string()],
            ttl: None,
            cluster_name: "prod",
            service_namespace: "default",
            service_name: "apex-svc",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.name, "@");
    }

    #[test]
    fn test_build_httproute_arecord_propagates_multi_ip_in_order() {
        let ips = vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()];
        let arecord = crate::scout::build_httproute_arecord(crate::scout::HTTPRouteARecordParams {
            name: "scout-prod-default-api-route-0",
            target_namespace: "bindy-system",
            record_name: "api",
            ips: &ips,
            ttl: None,
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "api-route",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.ipv4_addresses, ips);
    }

    #[test]
    fn test_build_httproute_arecord_accepts_apex_record_name() {
        let arecord = crate::scout::build_httproute_arecord(crate::scout::HTTPRouteARecordParams {
            name: "scout-prod-default-apex-route-0",
            target_namespace: "bindy-system",
            record_name: "@",
            ips: &["10.0.0.1".to_string()],
            ttl: None,
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "apex-route",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.name, "@");
    }

    #[test]
    fn test_build_tlsroute_arecord_propagates_multi_ip_in_order() {
        let ips = vec!["5.6.7.8".to_string(), "9.10.11.12".to_string()];
        let arecord = crate::scout::build_tlsroute_arecord(crate::scout::TLSRouteARecordParams {
            name: "scout-prod-default-secure-route-0",
            target_namespace: "bindy-system",
            record_name: "secure",
            ips: &ips,
            ttl: Some(900),
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "secure-route",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.ipv4_addresses, ips);
        assert_eq!(arecord.spec.ttl, Some(900));
    }

    #[test]
    fn test_build_tlsroute_arecord_accepts_apex_record_name() {
        let arecord = crate::scout::build_tlsroute_arecord(crate::scout::TLSRouteARecordParams {
            name: "scout-prod-default-apex-tls-0",
            target_namespace: "bindy-system",
            record_name: "@",
            ips: &["5.6.7.8".to_string()],
            ttl: None,
            cluster_name: "prod",
            route_namespace: "default",
            route_name: "apex-tls",
            zone: "example.com",
        });

        assert_eq!(arecord.spec.name, "@");
    }

    // ========================================================================
    // Gateway-chain IP resolution — pure helpers
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
        let m = parse_gateway_services("traefik=traefik/traefik,cilium=kube-system/cilium-gw");
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
        let m = parse_gateway_services("traefik=traefik/app.kubernetes.io/name=traefik");
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
        // Multi-label selectors contain commas — parsed per-entry (the CLI path)
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
    fn test_parse_gateway_services_skips_malformed_entries() {
        // "bad" has no target; "=x/y" has empty class; "dup=onlyname" has no ns/name
        // slash and no `=` selector; only the good entry survives.
        let m = parse_gateway_services("traefik=traefik/traefik,bad,=x/y,dup=onlyname");
        assert_eq!(m.len(), 1);
        assert!(matches!(
            m.get("traefik"),
            Some(GatewayServiceTarget::Name(_))
        ));
    }

    #[test]
    fn test_parse_gateway_services_empty() {
        assert!(parse_gateway_services("").is_empty());
        assert!(parse_gateway_services("   ").is_empty());
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
        let services = parse_gateway_services("traefik=traefik/traefik-ssc-01");
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
        let services = parse_gateway_services("cilium=kube-system/cilium-gw");
        assert!(
            !services.contains_key(&gw.spec.gateway_class_name),
            "traefik class absent from services map — no fallback"
        );
    }

    #[test]
    fn test_reconcile_tcproute_smoke_builds_arecord_from_annotation() {
        // Smoke test: verify reconcile_tcproute's code path — get_record_name_annotation drives
        // the record name and build_tcproute_arecord produces the expected ARecord CR.
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "bindy.firestoned.io/record-name".to_string(),
            "db".to_string(),
        );

        let zone = "example.com";
        let record_name = get_record_name_annotation(&annotations).expect("annotation present");

        let ips = vec!["10.0.0.5".to_string()];
        // reconcile_tcproute always uses index 0 — TCPRoute produces exactly one ARecord.
        let cr_name = tcproute_arecord_cr_name("test-cluster", "default", "my-tcproute", 0);

        let arecord = build_tcproute_arecord(TCPRouteARecordParams {
            name: &cr_name,
            target_namespace: "bindy-system",
            record_name: &record_name,
            ips: &ips,
            ttl: Some(300),
            cluster_name: "test-cluster",
            route_namespace: "default",
            route_name: "my-tcproute",
            zone,
        });

        assert_eq!(arecord.spec.name, record_name);
        assert_eq!(arecord.spec.ipv4_addresses, ips);
        assert_eq!(arecord.spec.ttl, Some(300));
    }

    // ------------------------------------------------------------------
    // kind_served
    //
    // The only tests in this file that speak to an API. Gateway API is not
    // installed by default in Kubernetes, and a Controller started against an
    // absent CRD retries forever rather than failing — so the detection has to
    // be right, and its three outcomes are covered here.
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
}
