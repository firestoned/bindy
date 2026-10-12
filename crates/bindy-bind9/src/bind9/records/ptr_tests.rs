// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for PTR record operations.

#[cfg(test)]
mod tests {
    use super::super::*;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_ptr_record(target: &str, ttl: u32) -> Record {
        let name = Name::from_str("10.0.168.192.in-addr.arpa.").expect("valid test name");
        let target_name = Name::from_str(target).expect("valid test target");
        Record::from_rdata(name, ttl, RData::PTR(rdata::PTR(target_name)))
    }

    // ========== compare_ptr_rrset ==========

    #[test]
    fn test_compare_ptr_rrset_matches_same_target_and_ttl() {
        let existing = vec![make_ptr_record("host10.example.com.", TEST_TTL)];
        assert!(compare_ptr_rrset(
            &existing,
            "host10.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_ptr_rrset_detects_target_mismatch() {
        let existing = vec![make_ptr_record("host10.example.com.", TEST_TTL)];
        assert!(!compare_ptr_rrset(
            &existing,
            "host11.example.com.",
            TEST_TTL
        ));
    }

    #[test]
    fn test_compare_ptr_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_ptr_record("host10.example.com.", TEST_TTL)];
        assert!(!compare_ptr_rrset(
            &existing,
            "host10.example.com.",
            OTHER_TTL
        ));
    }

    #[test]
    fn test_compare_ptr_rrset_multiple_records_triggers_update() {
        let existing = vec![
            make_ptr_record("host10.example.com.", TEST_TTL),
            make_ptr_record("host10-alt.example.com.", TEST_TTL),
        ];
        assert!(!compare_ptr_rrset(
            &existing,
            "host10.example.com.",
            TEST_TTL
        ));
    }
}
