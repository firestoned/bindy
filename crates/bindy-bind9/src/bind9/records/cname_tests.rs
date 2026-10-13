// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for CNAME record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::records::needs_update;
    use hickory_proto::rr::{rdata, Name, RData, Record};
    use std::str::FromStr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn make_cname_record(target: &str, ttl: u32) -> Record {
        let name = Name::from_str("alias.example.com.").expect("valid test name");
        let target_name = Name::from_str(target).expect("valid test target");
        Record::from_rdata(name, ttl, RData::CNAME(rdata::CNAME(target_name)))
    }

    // ========== cname_rdata ==========

    #[test]
    fn test_cname_rdata_is_one_value() {
        let rdata = cname_rdata("target.example.com.").expect("valid target");
        assert_eq!(
            rdata,
            vec![RData::CNAME(rdata::CNAME(
                Name::from_str("target.example.com.").expect("valid")
            ))]
        );
    }

    #[test]
    fn test_cname_rdata_rejects_a_bad_name() {
        let err = cname_rdata("bad..name").expect_err("empty label");
        assert!(format!("{err:#}").contains("bad..name"), "{err:#}");
    }

    // ========== value comparison through needs_update ==========

    #[test]
    fn test_cname_same_target_and_ttl_need_no_update() {
        let existing = vec![make_cname_record("target.example.com.", TEST_TTL)];
        let desired = cname_rdata("target.example.com.").expect("valid");
        assert!(!needs_update(&existing, &desired, TEST_TTL));
    }

    /// The reconcile loop ADR-0021 fixes: case and trailing dot only.
    #[test]
    fn test_cname_target_differing_in_case_or_trailing_dot_needs_no_update() {
        let existing = vec![make_cname_record("target.example.com.", TEST_TTL)];
        for spec in [
            "target.example.com",
            "TARGET.example.com.",
            "Target.Example.Com",
        ] {
            let desired = cname_rdata(spec).expect("valid");
            assert!(!needs_update(&existing, &desired, TEST_TTL), "{spec}");
        }
    }

    #[test]
    fn test_cname_changed_target_needs_update() {
        let existing = vec![make_cname_record("target.example.com.", TEST_TTL)];
        let desired = cname_rdata("other.example.com.").expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_cname_ttl_only_change_needs_update() {
        let existing = vec![make_cname_record("target.example.com.", TEST_TTL)];
        let desired = cname_rdata("target.example.com.").expect("valid");
        assert!(needs_update(&existing, &desired, OTHER_TTL));
    }
}
