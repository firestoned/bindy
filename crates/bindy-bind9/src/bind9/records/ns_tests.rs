// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for NS record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::records::needs_update;
    use hickory_proto::rr::{rdata, Name, RData, Record};
    use std::str::FromStr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_ns_record(nameserver: &str, ttl: u32) -> Record {
        let name = Name::from_str("sub.example.com.").expect("valid test name");
        let ns_name = Name::from_str(nameserver).expect("valid test nameserver");
        Record::from_rdata(name, ttl, RData::NS(rdata::NS(ns_name)))
    }

    // ========== ns_rdata ==========

    #[test]
    fn test_ns_rdata_is_one_value() {
        let rdata = ns_rdata("ns1.example.com.").expect("valid");
        assert_eq!(
            rdata,
            vec![RData::NS(rdata::NS(
                Name::from_str("ns1.example.com.").expect("valid")
            ))]
        );
    }

    #[test]
    fn test_ns_rdata_rejects_a_bad_name() {
        let err = ns_rdata("ns1..example.com").expect_err("empty label");
        assert!(format!("{err:#}").contains("ns1..example.com"), "{err:#}");
    }

    // ========== value comparison through needs_update ==========

    #[test]
    fn test_ns_same_nameserver_and_ttl_need_no_update() {
        let existing = vec![make_ns_record("ns1.example.com.", TEST_TTL)];
        let desired = ns_rdata("ns1.example.com").expect("valid");
        assert!(!needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_ns_changed_nameserver_needs_update() {
        let existing = vec![make_ns_record("ns1.example.com.", TEST_TTL)];
        let desired = ns_rdata("ns2.example.com.").expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_ns_ttl_only_change_needs_update() {
        let existing = vec![make_ns_record("ns1.example.com.", TEST_TTL)];
        let desired = ns_rdata("ns1.example.com.").expect("valid");
        assert!(needs_update(&existing, &desired, OTHER_TTL));
    }
}
