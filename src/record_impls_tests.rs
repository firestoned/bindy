// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for DNS record type trait implementations.
//!
//! These tests verify that all record types correctly implement the `DnsRecordType` trait
//! with the expected constants and behavior.

#[cfg(test)]
mod tests {
    use crate::crd::{
        AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord,
    };
    use crate::labels;
    use crate::record_controller::DnsRecordType;
    use hickory_client::rr::RecordType;

    // A Record Tests
    #[test]
    fn test_a_record_constants() {
        assert_eq!(ARecord::KIND, "ARecord");
        assert_eq!(ARecord::FINALIZER, labels::FINALIZER_A_RECORD);
        assert_eq!(ARecord::RECORD_TYPE_STR, "A");
    }

    #[test]
    fn test_a_record_hickory_type() {
        assert_eq!(ARecord::hickory_record_type(), RecordType::A);
    }

    // AAAA Record Tests
    #[test]
    fn test_aaaa_record_constants() {
        assert_eq!(AAAARecord::KIND, "AAAARecord");
        assert_eq!(AAAARecord::FINALIZER, labels::FINALIZER_AAAA_RECORD);
        assert_eq!(AAAARecord::RECORD_TYPE_STR, "AAAA");
    }

    #[test]
    fn test_aaaa_record_hickory_type() {
        assert_eq!(AAAARecord::hickory_record_type(), RecordType::AAAA);
    }

    // TXT Record Tests
    #[test]
    fn test_txt_record_constants() {
        assert_eq!(TXTRecord::KIND, "TXTRecord");
        assert_eq!(TXTRecord::FINALIZER, labels::FINALIZER_TXT_RECORD);
        assert_eq!(TXTRecord::RECORD_TYPE_STR, "TXT");
    }

    #[test]
    fn test_txt_record_hickory_type() {
        assert_eq!(TXTRecord::hickory_record_type(), RecordType::TXT);
    }

    // CNAME Record Tests
    #[test]
    fn test_cname_record_constants() {
        assert_eq!(CNAMERecord::KIND, "CNAMERecord");
        assert_eq!(CNAMERecord::FINALIZER, labels::FINALIZER_CNAME_RECORD);
        assert_eq!(CNAMERecord::RECORD_TYPE_STR, "CNAME");
    }

    #[test]
    fn test_cname_record_hickory_type() {
        assert_eq!(CNAMERecord::hickory_record_type(), RecordType::CNAME);
    }

    // MX Record Tests
    #[test]
    fn test_mx_record_constants() {
        assert_eq!(MXRecord::KIND, "MXRecord");
        assert_eq!(MXRecord::FINALIZER, labels::FINALIZER_MX_RECORD);
        assert_eq!(MXRecord::RECORD_TYPE_STR, "MX");
    }

    #[test]
    fn test_mx_record_hickory_type() {
        assert_eq!(MXRecord::hickory_record_type(), RecordType::MX);
    }

    // NS Record Tests
    #[test]
    fn test_ns_record_constants() {
        assert_eq!(NSRecord::KIND, "NSRecord");
        assert_eq!(NSRecord::FINALIZER, labels::FINALIZER_NS_RECORD);
        assert_eq!(NSRecord::RECORD_TYPE_STR, "NS");
    }

    #[test]
    fn test_ns_record_hickory_type() {
        assert_eq!(NSRecord::hickory_record_type(), RecordType::NS);
    }

    // SRV Record Tests
    #[test]
    fn test_srv_record_constants() {
        assert_eq!(SRVRecord::KIND, "SRVRecord");
        assert_eq!(SRVRecord::FINALIZER, labels::FINALIZER_SRV_RECORD);
        assert_eq!(SRVRecord::RECORD_TYPE_STR, "SRV");
    }

    #[test]
    fn test_srv_record_hickory_type() {
        assert_eq!(SRVRecord::hickory_record_type(), RecordType::SRV);
    }

    // CAA Record Tests
    #[test]
    fn test_caa_record_constants() {
        assert_eq!(CAARecord::KIND, "CAARecord");
        assert_eq!(CAARecord::FINALIZER, labels::FINALIZER_CAA_RECORD);
        assert_eq!(CAARecord::RECORD_TYPE_STR, "CAA");
    }

    #[test]
    fn test_caa_record_hickory_type() {
        assert_eq!(CAARecord::hickory_record_type(), RecordType::CAA);
    }

    // Cross-record validation tests
    #[test]
    fn test_all_finalizers_unique() {
        // Ensure all finalizers are unique to prevent conflicts
        let finalizers = [
            ARecord::FINALIZER,
            AAAARecord::FINALIZER,
            TXTRecord::FINALIZER,
            CNAMERecord::FINALIZER,
            MXRecord::FINALIZER,
            NSRecord::FINALIZER,
            SRVRecord::FINALIZER,
            CAARecord::FINALIZER,
        ];

        let unique_count = finalizers
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(
            unique_count,
            finalizers.len(),
            "All finalizers must be unique"
        );
    }

    #[test]
    fn test_all_kinds_unique() {
        // Ensure all KIND constants are unique
        let kinds = [
            ARecord::KIND,
            AAAARecord::KIND,
            TXTRecord::KIND,
            CNAMERecord::KIND,
            MXRecord::KIND,
            NSRecord::KIND,
            SRVRecord::KIND,
            CAARecord::KIND,
        ];

        let unique_count = kinds.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(
            unique_count,
            kinds.len(),
            "All KIND constants must be unique"
        );
    }

    #[test]
    fn test_all_record_type_strs_unique() {
        // Ensure all RECORD_TYPE_STR constants are unique
        let record_types = [
            ARecord::RECORD_TYPE_STR,
            AAAARecord::RECORD_TYPE_STR,
            TXTRecord::RECORD_TYPE_STR,
            CNAMERecord::RECORD_TYPE_STR,
            MXRecord::RECORD_TYPE_STR,
            NSRecord::RECORD_TYPE_STR,
            SRVRecord::RECORD_TYPE_STR,
            CAARecord::RECORD_TYPE_STR,
        ];

        let unique_count = record_types
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(
            unique_count,
            record_types.len(),
            "All RECORD_TYPE_STR constants must be unique"
        );
    }

    // NOTE: The following require integration testing with mocked Kubernetes API:
    //
    // reconcile_record() implementations for each record type:
    //   - Test successful reconciliation
    //   - Test reconciliation with missing zone
    //   - Test reconciliation with invalid data
    //   - Test error handling from underlying reconcile functions
    //   - Test proper error conversion to ReconcileError
    //
    // metadata() and status() accessor methods:
    //   - Test that metadata() returns correct ObjectMeta
    //   - Test that status() returns correct RecordStatus
    //   - Test behavior with None status
    //
    // These tests require:
    //   - Mock Kubernetes API client
    //   - Mock context with reflector stores
    //   - Mock Bind9Manager
    //   - Sample record specs with valid data
    //
    // These should be added to the integration test suite in /tests/ directory.
}
