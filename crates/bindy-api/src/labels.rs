// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Common label and annotation constants used across all reconcilers.
//!
//! This module defines standard Kubernetes labels and Bindy-specific labels/annotations
//! to ensure consistency across all resources created by the controller.

// ============================================================================
// Kubernetes Standard Labels
// https://kubernetes.io/docs/concepts/overview/working-with-objects/common-labels/
// ============================================================================

/// Standard label for the component name within the architecture (e.g., "dns-server", "dns-cluster")
pub const K8S_COMPONENT: &str = "app.kubernetes.io/component";

/// Standard label for the tool being used to manage the operation of an application
pub const K8S_MANAGED_BY: &str = "app.kubernetes.io/managed-by";

/// Standard label for the name of the application (e.g., "bind9")
pub const K8S_NAME: &str = "app.kubernetes.io/name";

/// Standard label for a unique name identifying the instance of an application
pub const K8S_INSTANCE: &str = "app.kubernetes.io/instance";

/// Standard label for the name of a higher-level application this one is part of
pub const K8S_PART_OF: &str = "app.kubernetes.io/part-of";

// ============================================================================
// Kubernetes Standard Label Values
// ============================================================================

/// Value for `app.kubernetes.io/part-of` indicating this resource is part of Bindy
pub const PART_OF_BINDY: &str = "bindy";

/// Label selector for objects bindy creates (`app.kubernetes.io/part-of=bindy`).
///
/// Kubernetes copies a Service's labels onto its `Endpoints`, so this also
/// selects the `Endpoints` of the BIND9 Services bindy manages; the shared
/// watch layer uses it to watch only those.
pub const BINDY_PART_OF_SELECTOR: &str = "app.kubernetes.io/part-of=bindy";

/// Component value for DNS server instances
pub const COMPONENT_DNS_SERVER: &str = "dns-server";

/// Component value for DNS clusters
pub const COMPONENT_DNS_CLUSTER: &str = "dns-cluster";

/// Label selector for the BIND9 operand pods bindy creates
/// (`app.kubernetes.io/part-of=bindy`, `app.kubernetes.io/component=dns-server`).
///
/// The shared Pod watch uses it so its cache holds only bindy's BIND9 pods,
/// not every pod in a watched namespace (ADR-0017). Every pod template built
/// by `build_pod_labels_from_instance` carries both labels.
pub const BIND9_POD_SELECTOR: &str =
    "app.kubernetes.io/part-of=bindy,app.kubernetes.io/component=dns-server";

/// Application name for BIND9 instances
pub const APP_NAME_BIND9: &str = "bind9";

// ============================================================================
// Kubernetes Standard Label Values - Managed By
// ============================================================================

/// Value for `app.kubernetes.io/managed-by` when resource is managed by `Bind9Instance` controller
pub const MANAGED_BY_BIND9_INSTANCE: &str = "Bind9Instance";

/// Value for `app.kubernetes.io/managed-by` when resource is managed by `Bind9Cluster` controller
pub const MANAGED_BY_BIND9_CLUSTER: &str = "Bind9Cluster";

/// Value for `app.kubernetes.io/managed-by` when resource is managed by `ClusterBind9Provider` controller
pub const MANAGED_BY_CLUSTER_BIND9_PROVIDER: &str = "ClusterBind9Provider";

// ============================================================================
// Bindy-Specific Labels
// ============================================================================

/// Label indicating which controller manages this resource (`Bind9Instance` or `Bind9Cluster`)
pub const BINDY_MANAGED_BY_LABEL: &str = "bindy.firestoned.io/managed-by";

/// Value of [`BINDY_MANAGED_BY_LABEL`] on the `ARecord`s Scout writes
pub const MANAGED_BY_SCOUT: &str = "scout";

/// Label carrying the DNS zone a Scout-written record belongs to. A `DNSZone`
/// takes a Scout record only when this equals its `spec.zoneName` (ADR-0025)
pub const BINDY_ZONE_LABEL: &str = "bindy.firestoned.io/zone";

/// Label carrying the cluster name of the Scout that wrote a record
pub const BINDY_SOURCE_CLUSTER_LABEL: &str = "bindy.firestoned.io/source-cluster";

/// The Scout source registry: a ConfigMap in the records namespace mapping
/// each registered Scout `ServiceAccount` to the cluster names and zones it
/// may publish (ADR-0025). Written by `bindy bootstrap mc`; read by admission
/// policy 21 and, to tell a cluster rename from two clusters, by the records
/// controller (ADR-0027).
pub const SCOUT_SOURCES_CONFIGMAP_NAME: &str = "bindy-scout-sources";

/// Suffix of a registry key listing an account's cluster names
/// (`<sa>.clusters`, comma-separated).
pub const SCOUT_SOURCES_CLUSTERS_SUFFIX: &str = ".clusters";

/// Suffix of a registry key listing an account's zones (`<sa>.zones`).
pub const SCOUT_SOURCES_ZONES_SUFFIX: &str = ".zones";

/// Label indicating which `Bind9Cluster` this resource belongs to
pub const BINDY_CLUSTER_LABEL: &str = "bindy.firestoned.io/cluster";

/// Label indicating the role of this instance (primary or secondary)
pub const BINDY_ROLE_LABEL: &str = "bindy.firestoned.io/role";

// ============================================================================
// Bindy-Specific Annotations
// ============================================================================

/// Annotation indicating the instance index within a cluster (used for scale-down ordering)
pub const BINDY_INSTANCE_INDEX_ANNOTATION: &str = "bindy.firestoned.io/instance-index";

/// Annotation used to trigger reconciliation (value is timestamp)
pub const BINDY_RECONCILE_TRIGGER_ANNOTATION: &str = "bindy.firestoned.io/reconcile-trigger";

// ============================================================================
// Finalizers
// ============================================================================

/// Finalizer for `Bind9Cluster` resources
pub const FINALIZER_BIND9_CLUSTER: &str = "bindy.firestoned.io/bind9cluster-finalizer";

/// Finalizer for `Bind9Instance` resources
pub const FINALIZER_BIND9_INSTANCE: &str = "bindy.firestoned.io/bind9instance-finalizer";

/// Finalizer for `DNSZone` resources
pub const FINALIZER_DNS_ZONE: &str = "bindy.firestoned.io/dnszone-finalizer";

// The record kinds' finalizers come from the record-kind table:
// `DNSRecordKind::finalizer` (ADR-0020).

// ============================================================================
// Role Values
// ============================================================================

/// Role value for primary DNS instances
pub const ROLE_PRIMARY: &str = "primary";

/// Role value for secondary DNS instances
pub const ROLE_SECONDARY: &str = "secondary";

#[cfg(test)]
#[path = "labels_tests.rs"]
mod labels_tests;
