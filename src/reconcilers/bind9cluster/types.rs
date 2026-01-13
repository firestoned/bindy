// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Shared types and imports for `Bind9Cluster` reconciliation.
//!
//! This module provides common type re-exports and shared utilities
//! used across the bind9cluster reconciliation modules.

#![allow(clippy::wildcard_imports)]

// Re-export commonly used types from parent modules
pub use crate::context::Context;
pub use crate::crd::{
    Bind9Cluster, Bind9ClusterStatus, Bind9Instance, Bind9InstanceSpec, Condition, ServerRole,
};
pub use crate::labels::{
    BINDY_CLUSTER_LABEL, BINDY_INSTANCE_INDEX_ANNOTATION, BINDY_MANAGED_BY_LABEL,
    BINDY_RECONCILE_TRIGGER_ANNOTATION, BINDY_ROLE_LABEL, K8S_PART_OF, MANAGED_BY_BIND9_CLUSTER,
    PART_OF_BINDY, ROLE_PRIMARY, ROLE_SECONDARY,
};
pub use crate::status_reasons::{
    bind9_instance_condition_type, CONDITION_TYPE_READY, REASON_ALL_READY, REASON_NOT_READY,
    REASON_NO_CHILDREN, REASON_PARTIALLY_READY, REASON_READY,
};

// Re-export commonly used Kubernetes types
pub use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{ConfigMap, Secret, Service},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};

// Re-export kube-rs types
pub use kube::{
    api::{DeleteParams, ListParams, Patch, PatchParams, PostParams},
    client::Client,
    Api, ResourceExt,
};

// Re-export common utilities
pub use anyhow::Result;
pub use chrono::Utc;
pub use serde_json::json;
pub use std::collections::BTreeMap;
pub use std::sync::Arc;
pub use tracing::{debug, error, info, warn};
