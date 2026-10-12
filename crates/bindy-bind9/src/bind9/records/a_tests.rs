// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for A and AAAA record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::{Bind9Manager, RndcKeyData};
    use hickory_proto::rr::{Name, RData, Record};
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::str::FromStr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_a_record(ip: Ipv4Addr, ttl: u32) -> Record {
        let name = Name::from_str("www.example.com.").expect("valid test name");
        Record::from_rdata(name, ttl, RData::A(ip.into()))
    }

    fn make_aaaa_record(ip: Ipv6Addr, ttl: u32) -> Record {
        let name = Name::from_str("www.example.com.").expect("valid test name");
        Record::from_rdata(name, ttl, RData::AAAA(ip.into()))
    }

    // ========== compare_ip_rrset (A records) ==========

    #[test]
    fn test_compare_ip_rrset_matches_same_ips_and_ttl() {
        let existing = vec![make_a_record(Ipv4Addr::new(192, 0, 2, 1), TEST_TTL)];
        let desired = vec!["192.0.2.1".to_string()];
        assert!(compare_ip_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_ip_rrset_detects_ip_mismatch() {
        let existing = vec![make_a_record(Ipv4Addr::new(192, 0, 2, 1), TEST_TTL)];
        let desired = vec!["192.0.2.2".to_string()];
        assert!(!compare_ip_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_ip_rrset_ttl_only_change_triggers_update() {
        // Same addresses, different TTL: must be reported as a mismatch.
        let existing = vec![make_a_record(Ipv4Addr::new(192, 0, 2, 1), TEST_TTL)];
        let desired = vec!["192.0.2.1".to_string()];
        assert!(!compare_ip_rrset(&existing, &desired, OTHER_TTL));
    }

    // ========== compare_ipv6_rrset (AAAA records) ==========

    #[test]
    fn test_compare_ipv6_rrset_matches_canonical_form() {
        let existing = vec![make_aaaa_record(
            Ipv6Addr::from_str("2001:db8::1").expect("valid test IP"),
            TEST_TTL,
        )];
        let desired = vec!["2001:db8::1".to_string()];
        assert!(compare_ipv6_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_ipv6_rrset_matches_uppercase_spec() {
        // `2001:DB8::1` is the same address as `2001:db8::1` and must NOT churn.
        let existing = vec![make_aaaa_record(
            Ipv6Addr::from_str("2001:db8::1").expect("valid test IP"),
            TEST_TTL,
        )];
        let desired = vec!["2001:DB8::1".to_string()];
        assert!(compare_ipv6_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_ipv6_rrset_matches_uncompressed_spec() {
        let existing = vec![make_aaaa_record(
            Ipv6Addr::from_str("2001:db8::1").expect("valid test IP"),
            TEST_TTL,
        )];
        let desired = vec!["2001:0db8:0000:0000:0000:0000:0000:0001".to_string()];
        assert!(compare_ipv6_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_ipv6_rrset_detects_address_mismatch() {
        let existing = vec![make_aaaa_record(
            Ipv6Addr::from_str("2001:db8::1").expect("valid test IP"),
            TEST_TTL,
        )];
        let desired = vec!["2001:db8::2".to_string()];
        assert!(!compare_ipv6_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_ipv6_rrset_unparseable_desired_is_mismatch() {
        // Invalid spec address must be treated as a mismatch, not a panic.
        let existing = vec![make_aaaa_record(
            Ipv6Addr::from_str("2001:db8::1").expect("valid test IP"),
            TEST_TTL,
        )];
        let desired = vec!["not-an-ipv6-address".to_string()];
        assert!(!compare_ipv6_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_ipv6_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_aaaa_record(
            Ipv6Addr::from_str("2001:db8::1").expect("valid test IP"),
            TEST_TTL,
        )];
        let desired = vec!["2001:db8::1".to_string()];
        assert!(!compare_ipv6_rrset(&existing, &desired, OTHER_TTL));
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_a_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        // This test now requires a real BIND9 server with TSIG authentication
        let result = manager
            .add_a_record(
                "example.com",
                "www",
                &["192.0.2.1".to_string()],
                Some(300),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        // Will fail without real server - test is ignored by default
        let _ = result;
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_aaaa_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_aaaa_record(
                "example.com",
                "www",
                &["2001:db8::1".to_string()],
                Some(300),
                "localhost:9530",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }

    // ========== parse_ipv4_addresses / parse_ipv6_addresses ==========
    //
    // Every address is validated before the live RRset is touched, so a spec
    // with one bad entry never wipes the existing records.

    #[test]
    fn test_parse_ipv4_addresses_accepts_valid_list() {
        let input = vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()];
        let parsed = parse_ipv4_addresses(&input).expect("valid IPv4 list");
        assert_eq!(
            parsed,
            vec![Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2)]
        );
    }

    #[test]
    fn test_parse_ipv4_addresses_rejects_list_with_a_bad_entry() {
        let input = vec!["10.0.0.1".to_string(), "bogus".to_string()];
        let err = parse_ipv4_addresses(&input).expect_err("a bad IPv4 must reject the list");
        assert!(
            err.to_string().contains("bogus"),
            "error must name the bad address: {err}"
        );
    }

    #[test]
    fn test_parse_ipv4_addresses_rejects_ipv6_entry() {
        let input = vec!["2001:db8::1".to_string()];
        assert!(parse_ipv4_addresses(&input).is_err());
    }

    #[test]
    fn test_parse_ipv4_addresses_empty_list_is_ok() {
        let parsed = parse_ipv4_addresses(&[]).expect("empty list parses");
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_parse_ipv6_addresses_accepts_valid_list() {
        let input = vec!["2001:db8::1".to_string(), "2001:DB8::2".to_string()];
        let parsed = parse_ipv6_addresses(&input).expect("valid IPv6 list");
        assert_eq!(
            parsed,
            vec![
                Ipv6Addr::from_str("2001:db8::1").expect("valid test IP"),
                Ipv6Addr::from_str("2001:db8::2").expect("valid test IP"),
            ]
        );
    }

    #[test]
    fn test_parse_ipv6_addresses_rejects_list_with_a_bad_entry() {
        let input = vec!["2001:db8::1".to_string(), "not-an-ip".to_string()];
        let err = parse_ipv6_addresses(&input).expect_err("a bad IPv6 must reject the list");
        assert!(
            err.to_string().contains("not-an-ip"),
            "error must name the bad address: {err}"
        );
    }

    /// An invalid address fails before any DNS traffic: the server here does
    /// not exist, so reaching the network would surface as a different error
    /// (or a hang, bounded by the timeout).
    #[tokio::test]
    async fn test_add_a_record_with_bad_ip_fails_before_any_network_call() {
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            add_a_record(
                "example.com",
                "www",
                &["10.0.0.1".to_string(), "bogus".to_string()],
                Some(300),
                "127.0.0.1:9",
                &key_data,
            ),
        )
        .await
        .expect("validation must not wait on the network");

        let err = result.expect_err("a bad IP must fail the write");
        assert!(
            format!("{err:#}").contains("Invalid IPv4 address: bogus"),
            "unexpected error: {err:#}"
        );
    }
}
