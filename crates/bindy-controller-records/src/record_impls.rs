// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Implementations of `DnsRecordType` trait for all DNS record types.
//!
//! Note: We cannot use `async fn` syntax in trait implementations that return
//! `impl Future` until Rust stabilizes return-position impl Trait in traits (RPITIT).
#![allow(clippy::manual_async_fn)]

use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, PTRRecord, RecordStatus,
    SRVRecord, TXTRecord,
};
use crate::record_operator::DnsRecordType;
use hickory_proto::rr::RecordType;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

// A Record Implementation
impl DnsRecordType for ARecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_A_RECORD;
    const RECORD_TYPE_STR: &'static str = "A";

    fn hickory_record_type() -> RecordType {
        RecordType::A
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// AAAA Record Implementation
impl DnsRecordType for AAAARecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_AAAA_RECORD;
    const RECORD_TYPE_STR: &'static str = "AAAA";

    fn hickory_record_type() -> RecordType {
        RecordType::AAAA
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// TXT Record Implementation
impl DnsRecordType for TXTRecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_TXT_RECORD;
    const RECORD_TYPE_STR: &'static str = "TXT";

    fn hickory_record_type() -> RecordType {
        RecordType::TXT
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// CNAME Record Implementation
impl DnsRecordType for CNAMERecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_CNAME_RECORD;
    const RECORD_TYPE_STR: &'static str = "CNAME";

    fn hickory_record_type() -> RecordType {
        RecordType::CNAME
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// MX Record Implementation
impl DnsRecordType for MXRecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_MX_RECORD;
    const RECORD_TYPE_STR: &'static str = "MX";

    fn hickory_record_type() -> RecordType {
        RecordType::MX
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// NS Record Implementation
impl DnsRecordType for NSRecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_NS_RECORD;
    const RECORD_TYPE_STR: &'static str = "NS";

    fn hickory_record_type() -> RecordType {
        RecordType::NS
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// SRV Record Implementation
impl DnsRecordType for SRVRecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_SRV_RECORD;
    const RECORD_TYPE_STR: &'static str = "SRV";

    fn hickory_record_type() -> RecordType {
        RecordType::SRV
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// CAA Record Implementation
impl DnsRecordType for CAARecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_CAA_RECORD;
    const RECORD_TYPE_STR: &'static str = "CAA";

    fn hickory_record_type() -> RecordType {
        RecordType::CAA
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}

// PTR Record Implementation
impl DnsRecordType for PTRRecord {
    const FINALIZER: &'static str = crate::labels::FINALIZER_PTR_RECORD;
    const RECORD_TYPE_STR: &'static str = "PTR";

    fn hickory_record_type() -> RecordType {
        RecordType::PTR
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}
