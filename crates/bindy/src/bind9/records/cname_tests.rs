// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for CNAME record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::{Bind9Manager, RndcKeyData};
    use hickory_proto::rr::rdata;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_cname_record(target: &str, ttl: u32) -> Record {
        let name = Name::from_str("alias.example.com.").expect("valid test name");
        let target_name = Name::from_str(target).expect("valid test target");
        Record::from_rdata(name, ttl, RData::CNAME(rdata::CNAME(target_name)))
    }

    // ========== compare_cname_rrset ==========

    #[test]
    fn test_compare_cname_rrset_matches_same_target_and_ttl() {
        let existing = vec![make_cname_record("target.example.com.", TEST_TTL)];
        assert!(compare_cname_rrset(
            &existing,
            "target.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_cname_rrset_detects_target_mismatch() {
        let existing = vec![make_cname_record("target.example.com.", TEST_TTL)];
        assert!(!compare_cname_rrset(
            &existing,
            "other.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_cname_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_cname_record("target.example.com.", TEST_TTL)];
        assert!(!compare_cname_rrset(
            &existing,
            "target.example.com.",
            OTHER_TTL
        ));
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_cname_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_cname_record(
                "example.com",
                "blog",
                "www.example.com.",
                Some(300),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        let _ = result;
    }
}
