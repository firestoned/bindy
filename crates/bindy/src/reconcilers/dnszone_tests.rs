// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `dnszone.rs`

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::crd::{DNSZoneSpec, NameServer, SOARecord};
    use std::collections::HashMap;

    /// Helper function to create a test SOA record
    fn create_test_soa() -> SOARecord {
        SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin.example.com.".to_string(),
            serial: 2_025_012_101,
            refresh: 3600,
            retry: 600,
            expire: 604_800,
            negative_ttl: 86_400,
        }
    }

    /// Helper function to create a minimal DNSZoneSpec for testing
    fn create_test_spec() -> DNSZoneSpec {
        DNSZoneSpec {
            zone_name: "example.com".to_string(),
            cluster_ref: Some("test-cluster".to_string()),
            soa_record: create_test_soa(),
            ttl: Some(3600),
            name_servers: None,
            #[allow(deprecated)]
            name_server_ips: None,
            bind9_instances_from: None,
            records_from: None,
            dnssec_policy: None,
        }
    }

    #[test]
    fn test_get_effective_name_servers_new_field() {
        // Test: new nameServers field is used when present
        let mut spec = create_test_spec();

        spec.name_servers = Some(vec![
            NameServer {
                hostname: "ns2.example.com.".to_string(),
                ipv4_address: Some("192.0.2.2".to_string()),
                ipv6_address: None,
            },
            NameServer {
                hostname: "ns3.example.com.".to_string(),
                ipv4_address: Some("192.0.2.3".to_string()),
                ipv6_address: Some("2001:db8::3".to_string()),
            },
        ]);

        let result = get_effective_name_servers(&spec);
        assert!(result.is_some());

        let servers = result.unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].hostname, "ns2.example.com.");
        assert_eq!(servers[0].ipv4_address, Some("192.0.2.2".to_string()));
        assert_eq!(servers[0].ipv6_address, None);
        assert_eq!(servers[1].hostname, "ns3.example.com.");
        assert_eq!(servers[1].ipv4_address, Some("192.0.2.3".to_string()));
        assert_eq!(servers[1].ipv6_address, Some("2001:db8::3".to_string()));
    }

    #[test]
    #[allow(deprecated)]
    fn test_get_effective_name_servers_deprecated_field() {
        // Test: backward compatibility with old nameServerIps field
        let mut spec = create_test_spec();

        let mut name_server_ips = HashMap::new();
        name_server_ips.insert("ns2.example.com.".to_string(), "192.0.2.2".to_string());
        name_server_ips.insert("ns3.example.com.".to_string(), "192.0.2.3".to_string());
        spec.name_server_ips = Some(name_server_ips);

        let result = get_effective_name_servers(&spec);
        assert!(result.is_some());

        let servers = result.unwrap();
        assert_eq!(servers.len(), 2);

        // Find the servers by hostname (HashMap ordering is not guaranteed)
        let ns2 = servers
            .iter()
            .find(|s| s.hostname == "ns2.example.com.")
            .unwrap();
        let ns3 = servers
            .iter()
            .find(|s| s.hostname == "ns3.example.com.")
            .unwrap();

        assert_eq!(ns2.ipv4_address, Some("192.0.2.2".to_string()));
        assert_eq!(ns2.ipv6_address, None); // Old format doesn't support IPv6
        assert_eq!(ns3.ipv4_address, Some("192.0.2.3".to_string()));
        assert_eq!(ns3.ipv6_address, None);
    }

    #[test]
    #[allow(deprecated)]
    fn test_get_effective_name_servers_precedence() {
        // Test: new field takes precedence when both fields are present
        let mut spec = create_test_spec();

        // Set both old and new fields
        spec.name_servers = Some(vec![NameServer {
            hostname: "ns-new.example.com.".to_string(),
            ipv4_address: Some("192.0.2.10".to_string()),
            ipv6_address: None,
        }]);

        let mut name_server_ips = HashMap::new();
        name_server_ips.insert("ns-old.example.com.".to_string(), "192.0.2.20".to_string());
        spec.name_server_ips = Some(name_server_ips);

        let result = get_effective_name_servers(&spec);
        assert!(result.is_some());

        let servers = result.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].hostname, "ns-new.example.com."); // New field wins
        assert_eq!(servers[0].ipv4_address, Some("192.0.2.10".to_string()));
    }

    #[test]
    fn test_get_effective_name_servers_none() {
        // Test: returns None when no nameservers are specified
        let spec = create_test_spec();

        let result = get_effective_name_servers(&spec);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_effective_name_servers_empty_vec() {
        // Test: returns Some with empty vec when nameServers is explicitly empty
        let mut spec = create_test_spec();

        spec.name_servers = Some(vec![]);

        let result = get_effective_name_servers(&spec);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_get_effective_name_servers_ipv6_only() {
        // Test: nameserver with only IPv6 address (no IPv4)
        let mut spec = create_test_spec();

        spec.name_servers = Some(vec![NameServer {
            hostname: "ns-ipv6.example.com.".to_string(),
            ipv4_address: None,
            ipv6_address: Some("2001:db8::1".to_string()),
        }]);

        let result = get_effective_name_servers(&spec);
        assert!(result.is_some());

        let servers = result.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].hostname, "ns-ipv6.example.com.");
        assert_eq!(servers[0].ipv4_address, None);
        assert_eq!(servers[0].ipv6_address, Some("2001:db8::1".to_string()));
    }

    #[test]
    fn test_get_effective_name_servers_dual_stack() {
        // Test: nameserver with both IPv4 and IPv6 addresses
        let mut spec = create_test_spec();

        spec.name_servers = Some(vec![NameServer {
            hostname: "ns-dual.example.com.".to_string(),
            ipv4_address: Some("192.0.2.5".to_string()),
            ipv6_address: Some("2001:db8::5".to_string()),
        }]);

        let result = get_effective_name_servers(&spec);
        assert!(result.is_some());

        let servers = result.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].hostname, "ns-dual.example.com.");
        assert_eq!(servers[0].ipv4_address, Some("192.0.2.5".to_string()));
        assert_eq!(servers[0].ipv6_address, Some("2001:db8::5".to_string()));
    }

    #[test]
    fn test_get_effective_name_servers_no_ip_addresses() {
        // Test: nameserver without any IP addresses (out-of-zone NS)
        let mut spec = create_test_spec();

        spec.name_servers = Some(vec![NameServer {
            hostname: "ns.external-provider.net.".to_string(),
            ipv4_address: None,
            ipv6_address: None,
        }]);

        let result = get_effective_name_servers(&spec);
        assert!(result.is_some());

        let servers = result.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].hostname, "ns.external-provider.net.");
        assert_eq!(servers[0].ipv4_address, None);
        assert_eq!(servers[0].ipv6_address, None);
    }

    #[test]
    fn test_nameserver_struct_cloning() {
        // Test: NameServer struct can be cloned
        let original = NameServer {
            hostname: "ns.example.com.".to_string(),
            ipv4_address: Some("192.0.2.1".to_string()),
            ipv6_address: Some("2001:db8::1".to_string()),
        };

        let cloned = original.clone();

        assert_eq!(original.hostname, cloned.hostname);
        assert_eq!(original.ipv4_address, cloned.ipv4_address);
        assert_eq!(original.ipv6_address, cloned.ipv6_address);
    }
}

#[cfg(test)]
mod notify_target_tests {
    use super::super::*;

    const INSTANCE_A: &str = "tlsdemo-primary-0";
    const INSTANCE_B: &str = "tlsdemo-primary-1";
    const NAMESPACE: &str = "bindy-system";
    const ENDPOINT_A: &str = "10.244.0.9:8080";
    const ENDPOINT_B: &str = "10.244.0.10:8080";

    /// The first endpoint seen is the one NOTIFY goes to.
    #[test]
    fn test_remember_first_notify_target_records_the_first_endpoint() {
        let mut slot = None;

        remember_first_notify_target(&mut slot, ENDPOINT_A, INSTANCE_A, NAMESPACE);

        let target = slot.expect("first endpoint should have been recorded");
        assert_eq!(target.endpoint, ENDPOINT_A);
        assert_eq!(target.instance_name, INSTANCE_A);
        assert_eq!(target.instance_namespace, NAMESPACE);
    }

    /// Endpoints are offered concurrently; only the first may win, or NOTIFY
    /// would chase a different endpoint on every reconcile.
    #[test]
    fn test_remember_first_notify_target_does_not_overwrite() {
        let mut slot = None;

        remember_first_notify_target(&mut slot, ENDPOINT_A, INSTANCE_A, NAMESPACE);
        remember_first_notify_target(&mut slot, ENDPOINT_B, INSTANCE_B, NAMESPACE);

        let target = slot.expect("a target should have been recorded");
        assert_eq!(target.endpoint, ENDPOINT_A);
        assert_eq!(target.instance_name, INSTANCE_A);
    }

    /// The regression this type exists for: the endpoint must stay bound to the
    /// instance that serves it. Losing that link is what made NOTIFY fall back
    /// to the shared startup manager — which carries no TLS config — and dial a
    /// TLS-only sidecar over plaintext `http://`.
    #[test]
    fn test_notify_target_keeps_instance_identity_for_its_endpoint() {
        let mut slot = None;

        remember_first_notify_target(&mut slot, ENDPOINT_B, INSTANCE_B, NAMESPACE);

        let target = slot.expect("a target should have been recorded");
        assert_eq!(
            (target.endpoint.as_str(), target.instance_name.as_str()),
            (ENDPOINT_B, INSTANCE_B),
            "the endpoint and the instance that serves it must travel together"
        );
    }

    // ------------------------------------------------------------------
    // ADR-0006: DNSSEC status decision logic (roadmap 07 Phase 5)
    // ------------------------------------------------------------------

    use crate::bind9::zone_ops::DsRecordInfo;

    fn test_ds_info() -> DsRecordInfo {
        DsRecordInfo {
            key_tag: 12345,
            algorithm: "ECDSAP256SHA256".to_string(),
            presentation: "example.com. IN DS 12345 13 2 ABCD".to_string(),
        }
    }

    #[test]
    fn test_build_dnssec_status_signed_zone() {
        let status = build_dnssec_status(Some("default"), &[test_ds_info()], None)
            .expect("a zone with DS records must report DNSSEC status");

        assert!(status.signed);
        assert_eq!(
            status.ds_records,
            vec!["example.com. IN DS 12345 13 2 ABCD".to_string()]
        );
        assert_eq!(status.key_tag, Some(12345));
        assert_eq!(status.algorithm, Some("ECDSAP256SHA256".to_string()));
        assert_eq!(status.next_key_rollover, None);
        assert_eq!(status.last_key_rollover, None);
    }

    #[test]
    fn test_build_dnssec_status_signed_without_explicit_policy() {
        // Cluster-global signing: no per-zone policy, but DNSKEYs exist.
        let status = build_dnssec_status(None, &[test_ds_info()], None)
            .expect("DS records present must win even without a per-zone policy");
        assert!(status.signed);
    }

    #[test]
    fn test_build_dnssec_status_pending_when_policy_set_but_unsigned() {
        // Policy requested but keys not generated yet: report signed=false.
        let status = build_dnssec_status(Some("default"), &[], None)
            .expect("an explicit policy must always yield a status");
        assert!(!status.signed);
        assert!(status.ds_records.is_empty());
        assert_eq!(status.key_tag, None);
    }

    #[test]
    fn test_build_dnssec_status_cleared_when_no_policy_and_unsigned() {
        assert!(
            build_dnssec_status(None, &[], None).is_none(),
            "no policy and no DNSKEYs means no DNSSEC status at all"
        );
    }

    #[test]
    fn test_build_dnssec_status_cleared_when_policy_none() {
        assert!(
            build_dnssec_status(Some("none"), &[], None).is_none(),
            "dnssecPolicy 'none' explicitly disables signing"
        );
        assert!(
            build_dnssec_status(Some("none"), &[test_ds_info()], None).is_none(),
            "'none' clears status even if stale DNSKEYs are still served"
        );
    }

    #[test]
    fn test_build_dnssec_status_multiple_ksks_reports_all_ds() {
        let mut second = test_ds_info();
        second.key_tag = 54321;
        second.presentation = "example.com. IN DS 54321 13 2 EF01".to_string();

        let status = build_dnssec_status(Some("default"), &[test_ds_info(), second], None)
            .expect("status must be reported");
        assert_eq!(
            status.ds_records.len(),
            2,
            "every KSK's DS record is published"
        );
        assert_eq!(status.key_tag, Some(12345), "keyTag reports the first KSK");
    }

    #[test]
    fn test_build_dnssec_status_carries_next_key_rollover() {
        let status = build_dnssec_status(
            Some("default"),
            &[test_ds_info()],
            Some("2027-09-27T00:00:00".to_string()),
        )
        .expect("signed zone must report status");
        assert_eq!(
            status.next_key_rollover.as_deref(),
            Some("2027-09-27T00:00:00")
        );
        assert_eq!(status.last_key_rollover, None, "no source in bindcar 0.8.x");
    }
}
