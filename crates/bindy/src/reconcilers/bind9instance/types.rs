// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Shared types and imports for `Bind9Instance` reconciliation.
//!
//! This module provides common type re-exports and shared utilities
//! used across the bind9instance reconciliation modules.

#![allow(clippy::wildcard_imports)]

// Re-export commonly used types from parent modules
pub use crate::context::Context;
pub use crate::crd::{
    Bind9Cluster, Bind9Instance, Bind9InstanceStatus, ClusterReference, Condition, ZoneReference,
};
pub use crate::labels::{BINDY_MANAGED_BY_LABEL, FINALIZER_BIND9_INSTANCE};
pub use crate::status_reasons::{
    pod_condition_type, CONDITION_TYPE_READY, REASON_ALL_READY, REASON_NOT_READY,
    REASON_PARTIALLY_READY, REASON_READY,
};

// Re-export commonly used Kubernetes types
pub use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{ConfigMap, Pod, Secret, Service, ServiceAccount},
};
pub use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

// Re-export kube-rs types
pub use kube::{
    api::{ListParams, Patch, PatchParams, PostParams},
    client::Client,
    Api, ResourceExt,
};

// Re-export common utilities
pub use anyhow::Result;
pub use chrono::Utc;
pub use serde_json::json;
pub use std::sync::Arc;
pub use tracing::{debug, error, info, warn};
