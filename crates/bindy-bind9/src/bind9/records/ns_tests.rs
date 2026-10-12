// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for NS record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use hickory_proto::rr::rdata;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_ns_record(nameserver: &str, ttl: u32) -> Record {
        let name = Name::from_str("sub.example.com.").expect("valid test name");
        let ns_name = Name::from_str(nameserver).expect("valid test nameserver");
        Record::from_rdata(name, ttl, RData::NS(rdata::NS(ns_name)))
    }

    // ========== compare_ns_rrset ==========

    #[test]
    fn test_compare_ns_rrset_matches_same_nameserver_and_ttl() {
        let existing = vec![make_ns_record("ns1.example.com.", TEST_TTL)];
        assert!(compare_ns_rrset(&existing, "ns1.example.com.", TEST_TTL));
    }

    #[test]
    fn test_compare_ns_rrset_detects_nameserver_mismatch() {
        let existing = vec![make_ns_record("ns1.example.com.", TEST_TTL)];
        assert!(!compare_ns_rrset(&existing, "ns2.example.com.", TEST_TTL));
    }

    #[test]
    fn test_compare_ns_rrset_ttl_only_change_triggers_update() {
        let existing = vec![make_ns_record("ns1.example.com.", TEST_TTL)];
        assert!(!compare_ns_rrset(&existing, "ns1.example.com.", OTHER_TTL));
    }
}
