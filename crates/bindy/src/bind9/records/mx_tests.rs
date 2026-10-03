// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for MX record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::{Bind9Manager, RndcKeyData};
    use hickory_proto::rr::rdata;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;
    const TEST_PRIORITY: u16 = 10;
    const OTHER_PRIORITY: u16 = 20;

    fn make_mx_record(priority: u16, mail_server: &str, ttl: u32) -> Record {
        let name = Name::from_str("example.com.").expect("valid test name");
        let exchange = Name::from_str(mail_server).expect("valid test exchange");
        Record::from_rdata(name, ttl, RData::MX(rdata::MX::new(priority, exchange)))
    }

    // ========== compare_mx_rrset ==========

    #[test]
    fn test_compare_mx_rrset_matches_same_values_and_ttl() {
        let existing = vec![make_mx_record(TEST_PRIORITY, "mail.example.com.", TEST_TTL)];
        assert!(compare_mx_rrset(
            &existing,
            TEST_PRIORITY,
            "mail.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_mx_rrset_detects_mail_server_mismatch() {
        let existing = vec![make_mx_record(
            TEST_PRIORITY,
            "mail1.example.com.",
            TEST_TTL,
        )];
        assert!(!compare_mx_rrset(
            &existing,
            TEST_PRIORITY,
            "mail2.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_mx_rrset_detects_priority_mismatch() {
        let existing = vec![make_mx_record(TEST_PRIORITY, "mail.example.com.", TEST_TTL)];
        assert!(!compare_mx_rrset(
            &existing,
            OTHER_PRIORITY,
            "mail.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_mx_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_mx_record(TEST_PRIORITY, "mail.example.com.", TEST_TTL)];
        assert!(!compare_mx_rrset(
            &existing,
            TEST_PRIORITY,
            "mail.example.com.",
            OTHER_TTL
        ));
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_mx_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_mx_record(
                "example.com",
                "@",
                10,
                "mail.example.com.",
                Some(3600),
                "localhost:9530",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }
}
