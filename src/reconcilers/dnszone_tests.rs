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
