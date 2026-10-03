// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for CAA record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::{Bind9Manager, RndcKeyData};

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_caa_issue_record(issuer_critical: bool, ca_domain: &str, ttl: u32) -> Record {
        let name = Name::from_str("example.com.").expect("valid test name");
        let ca_name = Name::from_str(ca_domain).expect("valid test CA domain");
        Record::from_rdata(
            name,
            ttl,
            RData::CAA(rdata::CAA::new_issue(
                issuer_critical,
                Some(ca_name),
                Vec::new(),
            )),
        )
    }

    // ========== compare_caa_rrset ==========

    #[test]
    fn test_compare_caa_rrset_matches_same_values_and_ttl() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        assert!(compare_caa_rrset(
            &existing,
            false,
            "issue",
            "letsencrypt.org",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_caa_rrset_detects_value_mismatch() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        assert!(!compare_caa_rrset(
            &existing,
            false,
            "issue",
            "digicert.com",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_caa_rrset_detects_flags_mismatch() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        assert!(!compare_caa_rrset(
            &existing,
            true,
            "issue",
            "letsencrypt.org",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_caa_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        assert!(!compare_caa_rrset(
            &existing,
            false,
            "issue",
            "letsencrypt.org",
            OTHER_TTL
        ));
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_caa_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issue",
                "letsencrypt.org",
                Some(3600),
                "localhost:9530",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }
}
