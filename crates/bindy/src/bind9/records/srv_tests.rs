// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for SRV record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::{Bind9Manager, RndcKeyData, SRVRecordData};

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;
    const TEST_PRIORITY: u16 = 10;
    const TEST_WEIGHT: u16 = 60;
    const TEST_PORT: u16 = 5060;
    const OTHER_PORT: u16 = 5061;

    fn make_srv_record(priority: u16, weight: u16, port: u16, target: &str, ttl: u32) -> Record {
        let name = Name::from_str("_sip._tcp.example.com.").expect("valid test name");
        let target_name = Name::from_str(target).expect("valid test target");
        Record::from_rdata(
            name,
            ttl,
            RData::SRV(rdata::SRV::new(priority, weight, port, target_name)),
        )
    }

    // ========== compare_srv_rrset ==========

    #[test]
    fn test_compare_srv_rrset_matches_same_values_and_ttl() {
        let existing = vec![make_srv_record(
            TEST_PRIORITY,
            TEST_WEIGHT,
            TEST_PORT,
            "sip.example.com.",
            TEST_TTL,
        )];
        assert!(compare_srv_rrset(
            &existing,
            TEST_PRIORITY,
            TEST_WEIGHT,
            TEST_PORT,
            "sip.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_srv_rrset_detects_port_mismatch() {
        let existing = vec![make_srv_record(
            TEST_PRIORITY,
            TEST_WEIGHT,
            TEST_PORT,
            "sip.example.com.",
            TEST_TTL,
        )];
        assert!(!compare_srv_rrset(
            &existing,
            TEST_PRIORITY,
            TEST_WEIGHT,
            OTHER_PORT,
            "sip.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_srv_rrset_detects_target_mismatch() {
        let existing = vec![make_srv_record(
            TEST_PRIORITY,
            TEST_WEIGHT,
            TEST_PORT,
            "sip1.example.com.",
            TEST_TTL,
        )];
        assert!(!compare_srv_rrset(
            &existing,
            TEST_PRIORITY,
            TEST_WEIGHT,
            TEST_PORT,
            "sip2.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_srv_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_srv_record(
            TEST_PRIORITY,
            TEST_WEIGHT,
            TEST_PORT,
            "sip.example.com.",
            TEST_TTL,
        )];
        assert!(!compare_srv_rrset(
            &existing,
            TEST_PRIORITY,
            TEST_WEIGHT,
            TEST_PORT,
            "sip.example.com.",
            OTHER_TTL
        ));
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_srv_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sip.example.com.".to_string(),
            ttl: Some(3600),
        };

        let result = manager
            .add_srv_record(
                "example.com",
                "_sip._tcp",
                &srv_data,
                "localhost:9530",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }
}
