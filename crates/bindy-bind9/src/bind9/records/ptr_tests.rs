// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for PTR record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::records::needs_update;
    use hickory_proto::rr::{rdata, Name, RData, Record};
    use std::str::FromStr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_ptr_record(target: &str, ttl: u32) -> Record {
        let name = Name::from_str("10.0.168.192.in-addr.arpa.").expect("valid test name");
        let target_name = Name::from_str(target).expect("valid test target");
        Record::from_rdata(name, ttl, RData::PTR(rdata::PTR(target_name)))
    }

    fn ptr(target: &str) -> PTRRecordData {
        PTRRecordData {
            target: target.to_string(),
            ttl: None,
        }
    }

    // ========== ptr_rdata ==========

    #[test]
    fn test_ptr_rdata_is_one_value() {
        let rdata = ptr_rdata(&ptr("host10.example.com.")).expect("valid");
        assert_eq!(
            rdata,
            vec![RData::PTR(rdata::PTR(
                Name::from_str("host10.example.com.").expect("valid")
            ))]
        );
    }

    #[test]
    fn test_ptr_rdata_rejects_a_bad_target() {
        let err = ptr_rdata(&ptr("host..example.com")).expect_err("empty label");
        assert!(format!("{err:#}").contains("host..example.com"), "{err:#}");
    }

    // ========== value comparison through needs_update ==========

    #[test]
    fn test_ptr_same_target_and_ttl_need_no_update() {
        let existing = vec![make_ptr_record("host10.example.com.", TEST_TTL)];
        let desired = ptr_rdata(&ptr("Host10.example.com")).expect("valid");
        assert!(!needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_ptr_changed_target_needs_update() {
        let existing = vec![make_ptr_record("host10.example.com.", TEST_TTL)];
        let desired = ptr_rdata(&ptr("host11.example.com.")).expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_ptr_ttl_only_change_needs_update() {
        let existing = vec![make_ptr_record("host10.example.com.", TEST_TTL)];
        let desired = ptr_rdata(&ptr("host10.example.com.")).expect("valid");
        assert!(needs_update(&existing, &desired, OTHER_TTL));
    }

    #[test]
    fn test_ptr_extra_live_record_needs_update() {
        let existing = vec![
            make_ptr_record("host10.example.com.", TEST_TTL),
            make_ptr_record("host10-alt.example.com.", TEST_TTL),
        ];
        let desired = ptr_rdata(&ptr("host10.example.com.")).expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }
}
