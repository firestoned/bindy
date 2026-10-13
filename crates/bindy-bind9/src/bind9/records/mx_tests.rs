// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for MX record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::records::needs_update;
    use crate::bind9::RndcKeyData;
    use hickory_proto::rr::{rdata, Name, RData, Record};
    use std::str::FromStr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;
    const TEST_PRIORITY: i32 = 10;
    const OTHER_PRIORITY: i32 = 20;

    fn make_mx_record(priority: u16, mail_server: &str, ttl: u32) -> Record {
        let name = Name::from_str("example.com.").expect("valid test name");
        let exchange = Name::from_str(mail_server).expect("valid test exchange");
        Record::from_rdata(name, ttl, RData::MX(rdata::MX::new(priority, exchange)))
    }

    // ========== mx_rdata ==========

    #[test]
    fn test_mx_rdata_is_one_value() {
        let rdata = mx_rdata(TEST_PRIORITY, "mail.example.com.").expect("valid");
        assert_eq!(
            rdata,
            vec![RData::MX(rdata::MX::new(
                10,
                Name::from_str("mail.example.com.").expect("valid")
            ))]
        );
    }

    #[test]
    fn test_mx_rdata_accepts_the_u16_bounds() {
        assert!(mx_rdata(0, "mail.example.com.").is_ok());
        assert!(mx_rdata(i32::from(u16::MAX), "mail.example.com.").is_ok());
    }

    /// Behaviour change decided by ADR-0021: an out-of-range priority used
    /// to be silently coerced to 10; it is now rejected like SRV's fields.
    #[test]
    fn test_mx_rdata_rejects_an_out_of_range_priority() {
        for priority in [-1, i32::from(u16::MAX) + 1] {
            let err = mx_rdata(priority, "mail.example.com.").expect_err("out of range");
            assert!(
                format!("{err:#}").contains(&format!("Invalid MX priority: {priority}")),
                "{err:#}"
            );
        }
    }

    #[test]
    fn test_mx_rdata_rejects_a_bad_mail_server() {
        let err = mx_rdata(TEST_PRIORITY, "mail..example.com").expect_err("empty label");
        assert!(format!("{err:#}").contains("mail..example.com"), "{err:#}");
    }

    /// The priority is validated before any DNS traffic: the server here
    /// does not exist.
    #[tokio::test]
    async fn test_add_mx_record_with_bad_priority_fails_before_any_network_call() {
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            add_mx_record(
                "example.com",
                "@",
                -1,
                "mail.example.com.",
                Some(300),
                "127.0.0.1:9",
                &key_data,
            ),
        )
        .await
        .expect("validation must not wait on the network");
        let err = result.expect_err("a bad priority must fail the write");
        assert!(
            format!("{err:#}").contains("Invalid MX priority: -1"),
            "unexpected error: {err:#}"
        );
    }

    // ========== value comparison through needs_update ==========

    #[test]
    fn test_mx_same_values_and_ttl_need_no_update() {
        let existing = vec![make_mx_record(10, "mail.example.com.", TEST_TTL)];
        let desired = mx_rdata(TEST_PRIORITY, "Mail.example.com").expect("valid");
        assert!(!needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_mx_changed_mail_server_needs_update() {
        let existing = vec![make_mx_record(10, "mail1.example.com.", TEST_TTL)];
        let desired = mx_rdata(TEST_PRIORITY, "mail2.example.com.").expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_mx_changed_priority_needs_update() {
        let existing = vec![make_mx_record(10, "mail.example.com.", TEST_TTL)];
        let desired = mx_rdata(OTHER_PRIORITY, "mail.example.com.").expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_mx_ttl_only_change_needs_update() {
        let existing = vec![make_mx_record(10, "mail.example.com.", TEST_TTL)];
        let desired = mx_rdata(TEST_PRIORITY, "mail.example.com.").expect("valid");
        assert!(needs_update(&existing, &desired, OTHER_TTL));
    }
}
