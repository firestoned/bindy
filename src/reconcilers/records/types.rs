// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Shared types and imports for DNS record reconciliation.

#![allow(clippy::wildcard_imports)]

// Re-export commonly used types
pub use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, Condition, DNSZone, MXRecord, NSRecord,
    RecordStatus, SRVRecord, TXTRecord, ZoneReference,
};
pub use anyhow::{Context as AnyhowContext, Result};
pub use k8s_openapi::api::core::v1::{Event, ObjectReference};
pub use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
pub use k8s_openapi::chrono::Utc;
pub use kube::{
    api::{Patch, PatchParams, PostParams},
    client::Client,
    Api, Resource, ResourceExt,
};
pub use serde_json::json;
pub use tracing::{debug, info, warn};
