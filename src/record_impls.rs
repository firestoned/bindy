// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Implementations of `DnsRecordType` trait for all DNS record types.
//!
//! Note: We cannot use `async fn` syntax in trait implementations that return
//! `impl Future` until Rust stabilizes return-position impl Trait in traits (RPITIT).
#![allow(clippy::manual_async_fn)]

use crate::context::Context;
use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, RecordStatus, SRVRecord,
    TXTRecord,
};
use crate::reconcilers::{
    reconcile_a_record, reconcile_aaaa_record, reconcile_caa_record, reconcile_cname_record,
    reconcile_mx_record, reconcile_ns_record, reconcile_srv_record, reconcile_txt_record,
};
use crate::record_operator::{DnsRecordType, ReconcileError};
use anyhow::Result;
use hickory_client::rr::RecordType;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::sync::Arc;

// A Record Implementation
impl DnsRecordType for ARecord {
    const KIND: &'static str = "ARecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_A_RECORD;
    const RECORD_TYPE_STR: &'static str = "A";

    fn hickory_record_type() -> RecordType {
        RecordType::A
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_a_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
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
    const KIND: &'static str = "AAAARecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_AAAA_RECORD;
    const RECORD_TYPE_STR: &'static str = "AAAA";

    fn hickory_record_type() -> RecordType {
        RecordType::AAAA
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_aaaa_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
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
    const KIND: &'static str = "TXTRecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_TXT_RECORD;
    const RECORD_TYPE_STR: &'static str = "TXT";

    fn hickory_record_type() -> RecordType {
        RecordType::TXT
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_txt_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
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
    const KIND: &'static str = "CNAMERecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_CNAME_RECORD;
    const RECORD_TYPE_STR: &'static str = "CNAME";

    fn hickory_record_type() -> RecordType {
        RecordType::CNAME
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_cname_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
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
    const KIND: &'static str = "MXRecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_MX_RECORD;
    const RECORD_TYPE_STR: &'static str = "MX";

    fn hickory_record_type() -> RecordType {
        RecordType::MX
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_mx_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
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
    const KIND: &'static str = "NSRecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_NS_RECORD;
    const RECORD_TYPE_STR: &'static str = "NS";

    fn hickory_record_type() -> RecordType {
        RecordType::NS
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_ns_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
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
    const KIND: &'static str = "SRVRecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_SRV_RECORD;
    const RECORD_TYPE_STR: &'static str = "SRV";

    fn hickory_record_type() -> RecordType {
        RecordType::SRV
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_srv_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
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
    const KIND: &'static str = "CAARecord";
    const FINALIZER: &'static str = crate::labels::FINALIZER_CAA_RECORD;
    const RECORD_TYPE_STR: &'static str = "CAA";

    fn hickory_record_type() -> RecordType {
        RecordType::CAA
    }

    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            reconcile_caa_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn status(&self) -> &Option<RecordStatus> {
        &self.status
    }
}
