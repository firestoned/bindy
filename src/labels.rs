// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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

/// Component value for DNS server instances
pub const COMPONENT_DNS_SERVER: &str = "dns-server";

/// Component value for DNS clusters
pub const COMPONENT_DNS_CLUSTER: &str = "dns-cluster";

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

/// Label indicating which `Bind9Cluster` this resource belongs to
pub const BINDY_CLUSTER_LABEL: &str = "bindy.firestoned.io/cluster";

/// Label indicating the role of this instance (primary or secondary)
pub const BINDY_ROLE_LABEL: &str = "bindy.firestoned.io/role";

// ============================================================================
// Bindy-Specific Annotations
// ============================================================================

/// Annotation indicating which `Bind9Cluster` a DNS record belongs to
pub const BINDY_CLUSTER_ANNOTATION: &str = "bindy.firestoned.io/cluster";

/// Annotation indicating which `Bind9Instance` a DNS record is deployed to
pub const BINDY_INSTANCE_ANNOTATION: &str = "bindy.firestoned.io/instance";

/// Annotation indicating which `DNSZone` a DNS record belongs to
pub const BINDY_ZONE_ANNOTATION: &str = "bindy.firestoned.io/zone";

/// Annotation indicating which `Bind9Instance` selected this `DNSZone` via label selector
pub const BINDY_SELECTED_BY_INSTANCE_ANNOTATION: &str = "bindy.firestoned.io/selected-by-instance";

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

// ============================================================================
// Role Values
// ============================================================================

/// Role value for primary DNS instances
pub const ROLE_PRIMARY: &str = "primary";

/// Role value for secondary DNS instances
pub const ROLE_SECONDARY: &str = "secondary";
