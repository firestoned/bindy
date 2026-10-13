// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for DNS record management functions.
//!
//! These tests focus on testable logic and error handling: the UPDATE message
//! [`replace_rrset_message`] builds (ADR-0021) is checked field by field, and
//! the value comparison [`needs_update`] is checked without a DNS server.
//! Full integration tests with real DNS servers are in tests/ directory.

#[cfg(test)]
mod tests {
    use super::super::*;
    use hickory_proto::op::{MessageType, OpCode, DEFAULT_MAX_PAYLOAD_LEN};
    use hickory_proto::rr::rdata;
    use hickory_proto::rr::RecordType;
    use std::net::Ipv4Addr;

    const TEST_TTL: u32 = 300;
    const OTHER_TTL: u32 = 600;

    fn name(s: &str) -> Name {
        Name::from_str(s).expect("valid test name")
    }

    fn a(ip: [u8; 4]) -> RData {
        RData::A(Ipv4Addr::from(ip).into())
    }

    fn record(owner: &str, ttl: u32, data: RData) -> Record {
        Record::from_rdata(name(owner), ttl, data)
    }

    fn cname(target: &str) -> RData {
        RData::CNAME(rdata::CNAME(name(target)))
    }

    // ========== Tests for is_idempotent_delete_response_code() ==========

    #[test]
    fn test_is_idempotent_delete_response_code_no_error() {
        assert!(is_idempotent_delete_response_code(ResponseCode::NoError));
    }

    #[test]
    fn test_is_idempotent_delete_response_code_nxdomain() {
        assert!(is_idempotent_delete_response_code(ResponseCode::NXDomain));
    }

    #[test]
    fn test_is_idempotent_delete_response_code_nxrrset() {
        assert!(is_idempotent_delete_response_code(ResponseCode::NXRRSet));
    }

    #[test]
    fn test_is_idempotent_delete_response_code_rejects_failures() {
        let failure_codes = [
            ResponseCode::Refused,
            ResponseCode::NotAuth,
            ResponseCode::NotZone,
            ResponseCode::ServFail,
            ResponseCode::FormErr,
            ResponseCode::NotImp,
            ResponseCode::YXRRSet,
        ];
        for code in failure_codes {
            assert!(
                !is_idempotent_delete_response_code(code),
                "{code:?} must NOT be treated as idempotent delete success"
            );
        }
    }

    // ========== Tests for effective_record_ttl() ==========

    #[test]
    fn test_effective_record_ttl_uses_default_when_none() {
        assert_eq!(effective_record_ttl(None), TEST_TTL);
    }

    #[test]
    fn test_effective_record_ttl_uses_spec_value() {
        assert_eq!(effective_record_ttl(Some(3600)), 3600);
    }

    #[test]
    fn test_effective_record_ttl_falls_back_on_negative_value() {
        assert_eq!(effective_record_ttl(Some(-5)), TEST_TTL);
    }

    // ========== Tests for build_delete_rrset_record() ==========

    #[test]
    fn test_build_delete_rrset_record_shape() {
        let fqdn = name("mail.example.com.");
        let record = build_delete_rrset_record(&fqdn, RecordType::MX);
        assert_eq!(record.name, fqdn);
        assert_eq!(record.ttl, 0);
        assert_eq!(record.data, RData::Update0(RecordType::MX));
    }

    // ========== Tests for update_transport() (ADR-0021) ==========
    //
    // hickory's UDP client has no TCP fallback, and one UPDATE now carries a
    // whole RRset: a message too large for the EDNS payload must go over TCP.

    #[test]
    fn a_small_update_goes_over_udp() {
        assert_eq!(update_transport(64), UpdateTransport::Udp);
    }

    #[test]
    fn an_update_larger_than_the_edns_payload_goes_over_tcp() {
        let edns_payload = usize::from(DEFAULT_MAX_PAYLOAD_LEN);
        assert_eq!(update_transport(edns_payload + 1), UpdateTransport::Tcp);
    }

    #[test]
    fn the_tsig_record_counts_toward_the_payload() {
        // A message that fits only without its TSIG record must still use TCP.
        let edns_payload = usize::from(DEFAULT_MAX_PAYLOAD_LEN);
        assert_eq!(update_transport(edns_payload - 1), UpdateTransport::Tcp);
    }

    #[test]
    fn a_large_rrset_replacement_is_sent_over_tcp() {
        let zone = Name::from_str("example.com.").unwrap();
        let fqdn = Name::from_str("www.example.com.").unwrap();
        let desired: Vec<RData> = (0..=255u8)
            .map(|i| RData::A(Ipv4Addr::new(10, 0, 0, i).into()))
            .collect();
        let msg = replace_rrset_message(&zone, &fqdn, RecordType::A, 300, desired, true).unwrap();

        assert_eq!(
            update_transport(encoded_len(&msg).unwrap()),
            UpdateTransport::Tcp
        );
    }

    // ========== Tests for replace_rrset_message() (ADR-0021) ==========

    #[test]
    fn test_replace_rrset_message_is_an_update_without_recursion() {
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("www.example.com."),
            RecordType::A,
            TEST_TTL,
            vec![a([192, 0, 2, 1])],
            true,
        )
        .expect("message builds");
        assert_eq!(msg.metadata.op_code, OpCode::Update);
        assert_eq!(msg.metadata.message_type, MessageType::Query);
        assert!(!msg.metadata.recursion_desired);
    }

    #[test]
    fn test_replace_rrset_message_zone_section_is_the_zone_soa() {
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("www.example.com."),
            RecordType::A,
            TEST_TTL,
            vec![a([192, 0, 2, 1])],
            true,
        )
        .expect("message builds");
        assert_eq!(msg.queries.len(), 1, "exactly one zone");
        let zone = &msg.queries[0];
        assert_eq!(zone.name, name("example.com."));
        assert_eq!(zone.query_type, RecordType::SOA);
        assert_eq!(zone.query_class, DNSClass::IN);
    }

    #[test]
    fn test_replace_rrset_message_has_no_prerequisites() {
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("www.example.com."),
            RecordType::A,
            TEST_TTL,
            vec![a([192, 0, 2, 1])],
            true,
        )
        .expect("message builds");
        assert!(msg.answers.is_empty(), "no prerequisite section");
    }

    #[test]
    fn test_replace_rrset_message_first_update_deletes_the_rrset() {
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("www.example.com."),
            RecordType::A,
            TEST_TTL,
            vec![a([192, 0, 2, 1])],
            true,
        )
        .expect("message builds");
        let delete = &msg.authorities[0];
        assert_eq!(delete.name, name("www.example.com."));
        assert_eq!(delete.dns_class, DNSClass::ANY, "RFC 2136 2.5.2: class ANY");
        assert_eq!(delete.ttl, 0, "RFC 2136 2.5.2: TTL 0");
        assert_eq!(delete.data, RData::Update0(RecordType::A));
        assert_eq!(delete.record_type(), RecordType::A);
    }

    #[test]
    fn test_replace_rrset_message_adds_every_value_in_order() {
        let desired = vec![a([192, 0, 2, 1]), a([192, 0, 2, 2]), a([192, 0, 2, 3])];
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("www.example.com."),
            RecordType::A,
            TEST_TTL,
            desired.clone(),
            true,
        )
        .expect("message builds");
        assert_eq!(
            msg.authorities.len(),
            1 + desired.len(),
            "one delete plus one add per value"
        );
        for (add, want) in msg.authorities[1..].iter().zip(&desired) {
            assert_eq!(add.name, name("www.example.com."));
            assert_eq!(add.dns_class, DNSClass::IN, "RFC 2136 2.5.1: zone class");
            assert_eq!(add.ttl, TEST_TTL);
            assert_eq!(&add.data, want);
        }
    }

    #[test]
    fn test_replace_rrset_message_empty_desired_is_only_the_delete() {
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("txt.example.com."),
            RecordType::TXT,
            TEST_TTL,
            Vec::new(),
            true,
        )
        .expect("message builds");
        assert_eq!(msg.authorities.len(), 1);
        assert_eq!(msg.authorities[0].data, RData::Update0(RecordType::TXT));
        assert_eq!(msg.authorities[0].dns_class, DNSClass::ANY);
    }

    #[test]
    fn test_replace_rrset_message_sets_edns_like_the_hickory_builders() {
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("www.example.com."),
            RecordType::A,
            TEST_TTL,
            vec![a([192, 0, 2, 1])],
            true,
        )
        .expect("message builds");
        let edns = msg.edns.as_ref().expect("EDNS present when requested");
        assert_eq!(edns.max_payload(), DEFAULT_MAX_PAYLOAD_LEN);
        assert_eq!(edns.version(), 0);
    }

    #[test]
    fn test_replace_rrset_message_without_edns() {
        let msg = replace_rrset_message(
            &name("example.com."),
            &name("www.example.com."),
            RecordType::A,
            TEST_TTL,
            vec![a([192, 0, 2, 1])],
            false,
        )
        .expect("message builds");
        assert!(msg.edns.is_none());
    }

    #[test]
    fn test_replace_rrset_message_rejects_a_name_outside_the_zone() {
        let err = replace_rrset_message(
            &name("example.com."),
            &name("www.example.org."),
            RecordType::A,
            TEST_TTL,
            vec![a([192, 0, 2, 1])],
            true,
        )
        .expect_err("a name outside the zone must not build");
        assert!(
            err.to_string().contains("www.example.org"),
            "error must name the record: {err}"
        );
    }

    // ========== Tests for check_replace_response() ==========

    #[test]
    fn test_check_replace_response_accepts_no_error() {
        assert!(check_replace_response(
            ResponseCode::NoError,
            &name("www.example.com."),
            RecordType::A
        )
        .is_ok());
    }

    #[test]
    fn test_check_replace_response_rejects_every_other_code() {
        for code in [
            ResponseCode::Refused,
            ResponseCode::NotAuth,
            ResponseCode::NotZone,
            ResponseCode::ServFail,
            ResponseCode::NXRRSet,
            ResponseCode::NXDomain,
            ResponseCode::YXRRSet,
        ] {
            let err = check_replace_response(code, &name("mail.example.com."), RecordType::MX)
                .expect_err("only NOERROR is success");
            let msg = err.to_string();
            assert!(msg.contains(&format!("{code:?}")), "code missing: {msg}");
            assert!(msg.contains("MX"), "type missing: {msg}");
            assert!(msg.contains("mail.example.com"), "name missing: {msg}");
        }
    }

    // ========== Tests for needs_update() (ADR-0021, compare by value) ==========

    #[test]
    fn test_needs_update_nothing_present_nothing_desired() {
        assert!(!needs_update(&[], &[], TEST_TTL));
    }

    #[test]
    fn test_needs_update_missing_rrset_is_created() {
        assert!(needs_update(&[], &[a([192, 0, 2, 1])], TEST_TTL));
    }

    #[test]
    fn test_needs_update_empty_desired_deletes_existing() {
        let current = vec![record("www.example.com.", TEST_TTL, a([192, 0, 2, 1]))];
        assert!(needs_update(&current, &[], TEST_TTL));
    }

    #[test]
    fn test_needs_update_identical_rrset_sends_nothing() {
        let current = vec![
            record("www.example.com.", TEST_TTL, a([192, 0, 2, 1])),
            record("www.example.com.", TEST_TTL, a([192, 0, 2, 2])),
        ];
        let desired = vec![a([192, 0, 2, 1]), a([192, 0, 2, 2])];
        assert!(!needs_update(&current, &desired, TEST_TTL));
    }

    #[test]
    fn test_needs_update_reordered_values_send_nothing() {
        let current = vec![
            record("www.example.com.", TEST_TTL, a([192, 0, 2, 2])),
            record("www.example.com.", TEST_TTL, a([192, 0, 2, 1])),
        ];
        let desired = vec![a([192, 0, 2, 1]), a([192, 0, 2, 2])];
        assert!(!needs_update(&current, &desired, TEST_TTL));
    }

    #[test]
    fn test_needs_update_changed_value() {
        let current = vec![record("www.example.com.", TEST_TTL, a([192, 0, 2, 1]))];
        assert!(needs_update(&current, &[a([192, 0, 2, 9])], TEST_TTL));
    }

    #[test]
    fn test_needs_update_extra_live_value() {
        let current = vec![
            record("www.example.com.", TEST_TTL, a([192, 0, 2, 1])),
            record("www.example.com.", TEST_TTL, a([192, 0, 2, 2])),
        ];
        assert!(needs_update(&current, &[a([192, 0, 2, 1])], TEST_TTL));
    }

    #[test]
    fn test_needs_update_ttl_only_change() {
        let current = vec![record("www.example.com.", TEST_TTL, a([192, 0, 2, 1]))];
        assert!(needs_update(&current, &[a([192, 0, 2, 1])], OTHER_TTL));
    }

    #[test]
    fn test_needs_update_one_stale_ttl_in_the_rrset() {
        let current = vec![
            record("www.example.com.", TEST_TTL, a([192, 0, 2, 1])),
            record("www.example.com.", OTHER_TTL, a([192, 0, 2, 2])),
        ];
        let desired = vec![a([192, 0, 2, 1]), a([192, 0, 2, 2])];
        assert!(needs_update(&current, &desired, TEST_TTL));
    }

    /// The loop ADR-0021 fixes: BIND9 answers with the fully qualified,
    /// lowercase target; a spec without the trailing dot used to compare
    /// unequal as text and was rewritten on every reconcile.
    #[test]
    fn test_needs_update_cname_target_without_trailing_dot_is_equal() {
        let current = vec![record(
            "alias.example.com.",
            TEST_TTL,
            cname("target.example.com."),
        )];
        assert!(!needs_update(
            &current,
            &[cname("target.example.com")],
            TEST_TTL
        ));
    }

    #[test]
    fn test_needs_update_cname_target_differing_in_case_is_equal() {
        let current = vec![record(
            "alias.example.com.",
            TEST_TTL,
            cname("target.example.com."),
        )];
        assert!(!needs_update(
            &current,
            &[cname("Target.EXAMPLE.com")],
            TEST_TTL
        ));
    }

    #[test]
    fn test_needs_update_cname_changed_target() {
        let current = vec![record(
            "alias.example.com.",
            TEST_TTL,
            cname("target.example.com."),
        )];
        assert!(needs_update(
            &current,
            &[cname("other.example.com.")],
            TEST_TTL
        ));
    }

    #[test]
    fn test_needs_update_normalises_mx_srv_ns_ptr_names() {
        let mx_live = RData::MX(rdata::MX::new(10, name("mail.example.com.")));
        let mx_spec = RData::MX(rdata::MX::new(10, name("MAIL.example.com")));
        let srv_live = RData::SRV(rdata::SRV::new(10, 60, 5060, name("sip.example.com.")));
        let srv_spec = RData::SRV(rdata::SRV::new(10, 60, 5060, name("Sip.Example.Com")));
        let ns_live = RData::NS(rdata::NS(name("ns1.example.com.")));
        let ns_spec = RData::NS(rdata::NS(name("NS1.example.com")));
        let ptr_live = RData::PTR(rdata::PTR(name("host.example.com.")));
        let ptr_spec = RData::PTR(rdata::PTR(name("HOST.example.com")));
        for (live, spec) in [
            (mx_live, mx_spec),
            (srv_live, srv_spec),
            (ns_live, ns_spec),
            (ptr_live, ptr_spec),
        ] {
            let current = vec![record("x.example.com.", TEST_TTL, live)];
            assert!(
                !needs_update(&current, std::slice::from_ref(&spec), TEST_TTL),
                "{spec:?} must equal its live form"
            );
        }
    }

    #[test]
    fn test_needs_update_normalises_caa_issuer_domain() {
        let live = RData::CAA(rdata::CAA::new_issue(
            false,
            Some(name("letsencrypt.org")),
            Vec::new(),
        ));
        let spec = RData::CAA(rdata::CAA::new_issue(
            false,
            Some(name("LetsEncrypt.org.")),
            Vec::new(),
        ));
        let current = vec![record("example.com.", TEST_TTL, live)];
        assert!(!needs_update(&current, &[spec], TEST_TTL));
    }

    #[test]
    fn test_needs_update_different_types_never_equal() {
        let current = vec![record(
            "alias.example.com.",
            TEST_TTL,
            cname("target.example.com."),
        )];
        assert!(needs_update(&current, &[a([192, 0, 2, 1])], TEST_TTL));
    }

    // NOTE: The network halves (query_dns_record, send_rrset_update,
    // write_rrset and delete_dns_record against a live BIND9) are exercised
    // by the kind and chaos e2e suites, not here.
}
