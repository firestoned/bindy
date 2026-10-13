// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the record-kind table (ADR-0020).
//!
//! These pin, for every one of the nine record kinds, the facts the operator
//! has always used: the kind string, its position in `DNSRecordKind::all()`,
//! the hickory `RecordType`, the finalizer name, and the CRD kind kube derives.
//! They were written against the hand-written matches and must stay green
//! now that one table generates them.

#[cfg(test)]
mod tests {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, DNSRecordKind, MXRecord, NSRecord, PTRRecord,
        SRVRecord, TXTRecord, UnknownDNSRecordKind,
    };
    use hickory_proto::rr::RecordType;
    use kube::Resource;

    /// One kind's facts, in `DNSRecordKind::all()` order.
    struct Pinned {
        kind: DNSRecordKind,
        kind_str: &'static str,
        crd_kind: String,
        record_type: RecordType,
        finalizer_value: &'static str,
    }

    fn pinned() -> Vec<Pinned> {
        vec![
            Pinned {
                kind: DNSRecordKind::A,
                kind_str: "ARecord",
                crd_kind: ARecord::kind(&()).to_string(),
                record_type: RecordType::A,
                finalizer_value: "bindy.firestoned.io/arecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::AAAA,
                kind_str: "AAAARecord",
                crd_kind: AAAARecord::kind(&()).to_string(),
                record_type: RecordType::AAAA,
                finalizer_value: "bindy.firestoned.io/aaaarecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::TXT,
                kind_str: "TXTRecord",
                crd_kind: TXTRecord::kind(&()).to_string(),
                record_type: RecordType::TXT,
                finalizer_value: "bindy.firestoned.io/txtrecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::CNAME,
                kind_str: "CNAMERecord",
                crd_kind: CNAMERecord::kind(&()).to_string(),
                record_type: RecordType::CNAME,
                finalizer_value: "bindy.firestoned.io/cnamerecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::MX,
                kind_str: "MXRecord",
                crd_kind: MXRecord::kind(&()).to_string(),
                record_type: RecordType::MX,
                finalizer_value: "bindy.firestoned.io/mxrecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::NS,
                kind_str: "NSRecord",
                crd_kind: NSRecord::kind(&()).to_string(),
                record_type: RecordType::NS,
                finalizer_value: "bindy.firestoned.io/nsrecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::SRV,
                kind_str: "SRVRecord",
                crd_kind: SRVRecord::kind(&()).to_string(),
                record_type: RecordType::SRV,
                finalizer_value: "bindy.firestoned.io/srvrecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::CAA,
                kind_str: "CAARecord",
                crd_kind: CAARecord::kind(&()).to_string(),
                record_type: RecordType::CAA,
                finalizer_value: "bindy.firestoned.io/caarecord-finalizer",
            },
            Pinned {
                kind: DNSRecordKind::PTR,
                kind_str: "PTRRecord",
                crd_kind: PTRRecord::kind(&()).to_string(),
                record_type: RecordType::PTR,
                finalizer_value: "bindy.firestoned.io/ptrrecord-finalizer",
            },
        ]
    }

    #[test]
    fn all_lists_the_nine_kinds_in_discovery_order() {
        let expected: Vec<DNSRecordKind> = pinned().iter().map(|p| p.kind).collect();
        assert_eq!(DNSRecordKind::all(), expected.as_slice());
    }

    #[test]
    fn kind_string_matches_the_crd_kind_and_round_trips() {
        for p in pinned() {
            assert_eq!(p.kind.as_str(), p.kind_str);
            assert_eq!(p.kind.as_str(), p.crd_kind, "table kind vs kube derive");
            assert_eq!(p.kind.to_string(), p.kind_str, "Display");
            assert_eq!(DNSRecordKind::try_from(p.kind_str), Ok(p.kind));
            assert_eq!(DNSRecordKind::try_from(p.kind_str.to_string()), Ok(p.kind));
            assert_eq!(p.kind_str.parse::<DNSRecordKind>(), Ok(p.kind));
        }
    }

    #[test]
    fn hickory_record_type_is_pinned_per_kind() {
        for p in pinned() {
            assert_eq!(
                p.kind.to_hickory_record_type(),
                p.record_type,
                "{}",
                p.kind_str
            );
        }
    }

    #[test]
    fn finalizer_names_are_pinned_per_kind() {
        for p in pinned() {
            assert_eq!(p.kind.finalizer(), p.finalizer_value, "{}", p.kind_str);
        }
    }

    #[test]
    fn unknown_kind_error_message_is_unchanged() {
        let err = DNSRecordKind::try_from("UnknownRecord").expect_err("not a record kind");
        assert_eq!(err, UnknownDNSRecordKind("UnknownRecord".to_string()));
        assert_eq!(
            err.to_string(),
            "unknown DNS record kind \"UnknownRecord\" (expected one of: ARecord, AAAARecord, \
             TXTRecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, CAARecord, PTRRecord)"
        );
    }

    // The helper macros generated from the table (ADR-0020).

    /// Number of record kinds bindy serves.
    const NINE_KINDS: usize = 9;

    #[test]
    fn count_is_the_number_of_kinds() {
        assert_eq!(DNSRecordKind::COUNT, NINE_KINDS);
        assert_eq!(DNSRecordKind::all().len(), DNSRecordKind::COUNT);
    }

    #[test]
    fn each_record_kind_visits_every_kind_in_table_order() {
        let mut visited: Vec<String> = Vec::new();
        crate::each_record_kind!(K => visited.push(K::kind(&()).to_string()));
        let expected: Vec<&str> = DNSRecordKind::all().iter().map(|k| k.as_str()).collect();
        assert_eq!(visited, expected);
    }

    #[test]
    fn match_record_kind_binds_the_matching_type() {
        for kind in DNSRecordKind::all() {
            let crd_kind = crate::match_record_kind!(*kind, K => K::kind(&()).to_string());
            assert_eq!(crd_kind, kind.as_str());
        }
    }

    #[test]
    fn record_kind_array_has_one_entry_per_kind_in_table_order() {
        let types: [&str; DNSRecordKind::COUNT] =
            crate::record_kind_array!(K => std::any::type_name::<K>());
        let expected: Vec<String> = DNSRecordKind::all()
            .iter()
            .map(|k| format!("bindy_api::crd::{}", k.as_str()))
            .collect();
        assert_eq!(types.to_vec(), expected);
    }
}
