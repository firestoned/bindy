// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for CAA record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::records::needs_update;
    use hickory_proto::rr::{rdata, Name, RData, Record};
    use std::str::FromStr;
    use url::Url;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;
    /// The RFC 8659 issuer-critical flag byte.
    const CRITICAL_FLAGS: i32 = 128;

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

    // ========== caa_rdata ==========

    #[test]
    fn test_caa_rdata_issue() {
        let rdata = caa_rdata(0, "issue", "letsencrypt.org").expect("valid");
        assert_eq!(
            rdata,
            vec![RData::CAA(rdata::CAA::new_issue(
                false,
                Some(Name::from_str("letsencrypt.org").expect("valid")),
                Vec::new()
            ))]
        );
    }

    #[test]
    fn test_caa_rdata_issuewild_with_critical_flag() {
        let rdata = caa_rdata(CRITICAL_FLAGS, "issuewild", "letsencrypt.org").expect("valid");
        assert_eq!(
            rdata,
            vec![RData::CAA(rdata::CAA::new_issuewild(
                true,
                Some(Name::from_str("letsencrypt.org").expect("valid")),
                Vec::new()
            ))]
        );
    }

    #[test]
    fn test_caa_rdata_empty_issue_value_forbids_issuance() {
        let rdata = caa_rdata(0, "issue", "").expect("valid");
        assert_eq!(
            rdata,
            vec![RData::CAA(rdata::CAA::new_issue(false, None, Vec::new()))]
        );
    }

    #[test]
    fn test_caa_rdata_iodef() {
        let rdata = caa_rdata(0, "iodef", "mailto:security@example.com").expect("valid");
        assert_eq!(
            rdata,
            vec![RData::CAA(rdata::CAA::new_iodef(
                false,
                Url::parse("mailto:security@example.com").expect("valid")
            ))]
        );
    }

    #[test]
    fn test_caa_rdata_rejects_an_unsupported_tag() {
        let err = caa_rdata(0, "contactemail", "x@example.com").expect_err("bad tag");
        assert!(
            format!("{err:#}").contains("Unsupported CAA tag: contactemail"),
            "{err:#}"
        );
    }

    #[test]
    fn test_caa_rdata_rejects_a_bad_iodef_url() {
        let err = caa_rdata(0, "iodef", "not a url").expect_err("bad url");
        assert!(format!("{err:#}").contains("Invalid iodef URL"), "{err:#}");
    }

    #[test]
    fn test_caa_rdata_rejects_a_bad_issuer_domain() {
        let err = caa_rdata(0, "issue", "bad..ca").expect_err("bad name");
        assert!(format!("{err:#}").contains("bad..ca"), "{err:#}");
    }

    // ========== value comparison through needs_update ==========

    #[test]
    fn test_caa_same_values_and_ttl_need_no_update() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        let desired = caa_rdata(0, "issue", "letsencrypt.org").expect("valid");
        assert!(!needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_caa_changed_value_needs_update() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        let desired = caa_rdata(0, "issue", "digicert.com").expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_caa_changed_flags_needs_update() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        let desired = caa_rdata(CRITICAL_FLAGS, "issue", "letsencrypt.org").expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_caa_ttl_only_change_needs_update() {
        let existing = vec![make_caa_issue_record(false, "letsencrypt.org", TEST_TTL)];
        let desired = caa_rdata(0, "issue", "letsencrypt.org").expect("valid");
        assert!(needs_update(&existing, &desired, OTHER_TTL));
    }
}
