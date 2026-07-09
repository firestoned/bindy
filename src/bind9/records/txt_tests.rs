// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for TXT record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::{Bind9Manager, RndcKeyData};

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_txt_record(texts: &[&str], ttl: u32) -> Record {
        let name = Name::from_str("example.com.").expect("valid test name");
        let txt_data: Vec<String> = texts.iter().map(ToString::to_string).collect();
        Record::from_rdata(name, ttl, RData::TXT(rdata::TXT::new(txt_data)))
    }

    // ========== compare_txt_rrset ==========

    #[test]
    fn test_compare_txt_rrset_matches_same_texts_and_ttl() {
        let existing = vec![make_txt_record(&["v=spf1 mx ~all"], TEST_TTL)];
        let desired = vec!["v=spf1 mx ~all".to_string()];
        assert!(compare_txt_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_txt_rrset_detects_text_mismatch() {
        let existing = vec![make_txt_record(&["v=spf1 mx ~all"], TEST_TTL)];
        let desired = vec!["v=spf1 -all".to_string()];
        assert!(!compare_txt_rrset(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_compare_txt_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_txt_record(&["v=spf1 mx ~all"], TEST_TTL)];
        let desired = vec!["v=spf1 mx ~all".to_string()];
        assert!(!compare_txt_rrset(&existing, &desired, OTHER_TTL));
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_txt_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let texts = vec!["v=spf1 mx ~all".to_string()];
        let result = manager
            .add_txt_record(
                "example.com",
                "@",
                &texts,
                Some(3600),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        let _ = result;
    }
}
