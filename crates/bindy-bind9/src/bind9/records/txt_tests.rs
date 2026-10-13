// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for TXT record operations.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bind9::records::needs_update;
    use hickory_proto::rr::{rdata, Name, RData, Record};
    use std::str::FromStr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;
    /// One byte longer than an RFC 1035 character-string may be.
    const TOO_LONG: usize = 256;
    /// The longest RFC 1035 character-string.
    const LONGEST: usize = 255;

    fn make_txt_record(texts: &[&str], ttl: u32) -> Record {
        let name = Name::from_str("example.com.").expect("valid test name");
        let txt_data: Vec<String> = texts.iter().map(ToString::to_string).collect();
        Record::from_rdata(name, ttl, RData::TXT(rdata::TXT::new(txt_data)))
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    // ========== txt_rdata ==========

    /// Today's writer puts every spec string in ONE TXT record, in order;
    /// that stays.
    #[test]
    fn test_txt_rdata_is_one_record_with_every_string_in_order() {
        let rdata = txt_rdata(&s(&["v=spf1", "mx ~all"])).expect("valid");
        assert_eq!(
            rdata,
            vec![RData::TXT(rdata::TXT::new(s(&["v=spf1", "mx ~all"])))]
        );
    }

    #[test]
    fn test_txt_rdata_accepts_the_longest_string() {
        assert!(txt_rdata(&["x".repeat(LONGEST)]).is_ok());
    }

    /// Before ADR-0021 an over-long string failed only when the append was
    /// encoded, after the delete had already removed the live `RRset`.
    #[test]
    fn test_txt_rdata_rejects_a_string_over_255_bytes() {
        let err = txt_rdata(&["x".repeat(TOO_LONG)]).expect_err("too long");
        assert!(format!("{err:#}").contains("255"), "{err:#}");
    }

    // ========== value comparison through needs_update ==========

    #[test]
    fn test_txt_same_texts_and_ttl_need_no_update() {
        let existing = vec![make_txt_record(&["v=spf1 mx ~all"], TEST_TTL)];
        let desired = txt_rdata(&s(&["v=spf1 mx ~all"])).expect("valid");
        assert!(!needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_txt_changed_text_needs_update() {
        let existing = vec![make_txt_record(&["v=spf1 mx ~all"], TEST_TTL)];
        let desired = txt_rdata(&s(&["v=spf1 -all"])).expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }

    #[test]
    fn test_txt_ttl_only_change_needs_update() {
        let existing = vec![make_txt_record(&["v=spf1 mx ~all"], TEST_TTL)];
        let desired = txt_rdata(&s(&["v=spf1 mx ~all"])).expect("valid");
        assert!(needs_update(&existing, &desired, OTHER_TTL));
    }

    #[test]
    fn test_txt_reordered_strings_inside_one_record_need_update() {
        // The strings of one TXT record are ordered (readers such as SPF
        // concatenate them), unlike the records of an RRset.
        let existing = vec![make_txt_record(&["a", "b"], TEST_TTL)];
        let desired = txt_rdata(&s(&["b", "a"])).expect("valid");
        assert!(needs_update(&existing, &desired, TEST_TTL));
    }
}
