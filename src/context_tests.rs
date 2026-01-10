// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for context.rs

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn test_record_ref_name() {
        let record = RecordRef::A("test-record".to_string(), "default".to_string());
        assert_eq!(record.name(), "test-record");

        let record = RecordRef::AAAA("ipv6-record".to_string(), "default".to_string());
        assert_eq!(record.name(), "ipv6-record");
    }

    #[test]
    fn test_record_ref_namespace() {
        let record = RecordRef::A("test-record".to_string(), "dns-system".to_string());
        assert_eq!(record.namespace(), "dns-system");

        let record = RecordRef::TXT("txt-record".to_string(), "other-ns".to_string());
        assert_eq!(record.namespace(), "other-ns");
    }

    #[test]
    fn test_record_ref_record_type() {
        assert_eq!(
            RecordRef::A("test".to_string(), "default".to_string()).record_type(),
            "A"
        );
        assert_eq!(
            RecordRef::AAAA("test".to_string(), "default".to_string()).record_type(),
            "AAAA"
        );
        assert_eq!(
            RecordRef::CNAME("test".to_string(), "default".to_string()).record_type(),
            "CNAME"
        );
        assert_eq!(
            RecordRef::TXT("test".to_string(), "default".to_string()).record_type(),
            "TXT"
        );
        assert_eq!(
            RecordRef::MX("test".to_string(), "default".to_string()).record_type(),
            "MX"
        );
        assert_eq!(
            RecordRef::NS("test".to_string(), "default".to_string()).record_type(),
            "NS"
        );
        assert_eq!(
            RecordRef::SRV("test".to_string(), "default".to_string()).record_type(),
            "SRV"
        );
        assert_eq!(
            RecordRef::CAA("test".to_string(), "default".to_string()).record_type(),
            "CAA"
        );
    }

    #[test]
    fn test_record_ref_equality() {
        let record1 = RecordRef::A("test".to_string(), "default".to_string());
        let record2 = RecordRef::A("test".to_string(), "default".to_string());
        let record3 = RecordRef::A("other".to_string(), "default".to_string());
        let record4 = RecordRef::AAAA("test".to_string(), "default".to_string());

        assert_eq!(record1, record2);
        assert_ne!(record1, record3);
        assert_ne!(record1, record4);
    }

    #[test]
    fn test_record_ref_clone() {
        let record = RecordRef::A("test".to_string(), "default".to_string());
        let cloned = record.clone();

        assert_eq!(record, cloned);
        assert_eq!(record.name(), cloned.name());
        assert_eq!(record.namespace(), cloned.namespace());
        assert_eq!(record.record_type(), cloned.record_type());
    }
}
