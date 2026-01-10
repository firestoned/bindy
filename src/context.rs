// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Shared context for all controllers with reflector stores.
//!
//! This module provides the core infrastructure for the shared reflector store pattern.
//! All controllers receive an `Arc<Context>` that contains:
//! - Kubernetes client
//! - Reflector stores for all CRD types
//! - Metrics registry
//!
//! The stores enable O(1) in-memory lookups for label-based resource selection,
//! eliminating the need for API queries in watch mappers.

use crate::crd::{
    AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord, ClusterBind9Provider,
    DNSZone, LabelSelector, MXRecord, NSRecord, SRVRecord, TXTRecord,
};
use k8s_openapi::api::apps::v1::Deployment;
use kube::runtime::reflector::Store;
use kube::{Client, ResourceExt};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Shared context passed to all controllers.
///
/// This context provides access to:
/// - Kubernetes client for API operations
/// - Reflector stores for efficient label-based queries
/// - HTTP client for bindcar API calls
/// - Metrics for observability
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client for API operations
    pub client: Client,

    /// Reflector stores for all CRD types
    pub stores: Stores,

    /// HTTP client for bindcar zone synchronization API calls
    pub http_client: reqwest::Client,

    /// Metrics registry for observability
    pub metrics: Metrics,
}

/// Collection of all reflector stores for cross-controller queries.
///
/// Each store is populated by a dedicated reflector task and provides
/// in-memory access to resources without API calls.
#[derive(Clone)]
pub struct Stores {
    // Cluster-scoped resources
    pub cluster_bind9_providers: Store<ClusterBind9Provider>,

    // Namespace-scoped resources
    pub bind9_clusters: Store<Bind9Cluster>,
    pub bind9_instances: Store<Bind9Instance>,
    pub bind9_deployments: Store<Deployment>,
    pub dnszones: Store<DNSZone>,

    // DNS Record types
    pub a_records: Store<ARecord>,
    pub aaaa_records: Store<AAAARecord>,
    pub cname_records: Store<CNAMERecord>,
    pub txt_records: Store<TXTRecord>,
    pub mx_records: Store<MXRecord>,
    pub ns_records: Store<NSRecord>,
    pub srv_records: Store<SRVRecord>,
    pub caa_records: Store<CAARecord>,
}

impl Stores {
    /// Query all record stores and return matching records for a label selector.
    ///
    /// This method searches across all 8 record type stores to find records that:
    /// 1. Exist in the specified namespace
    /// 2. Match the provided label selector
    ///
    /// # Arguments
    /// * `selector` - The label selector to match against record labels
    /// * `namespace` - The namespace to search within (namespace-isolated)
    ///
    /// # Returns
    /// A vector of [`RecordRef`] enums containing references to all matching records
    #[must_use]
    pub fn records_matching_selector(
        &self,
        selector: &LabelSelector,
        namespace: &str,
    ) -> Vec<RecordRef> {
        let mut results = Vec::new();

        // Helper macro to reduce boilerplate
        macro_rules! collect_matching {
            ($store:expr, $variant:ident) => {
                for record in $store.state() {
                    if record.namespace().as_deref() == Some(namespace)
                        && crate::selector::matches_selector(selector, &record.labels())
                    {
                        results.push(RecordRef::$variant(
                            record.name_any(),
                            record.namespace().unwrap_or_default(),
                        ));
                    }
                }
            };
        }

        collect_matching!(self.a_records, A);
        collect_matching!(self.aaaa_records, AAAA);
        collect_matching!(self.cname_records, CNAME);
        collect_matching!(self.txt_records, TXT);
        collect_matching!(self.mx_records, MX);
        collect_matching!(self.ns_records, NS);
        collect_matching!(self.srv_records, SRV);
        collect_matching!(self.caa_records, CAA);

        results
    }

    /// Query dnszones matching a label selector.
    ///
    /// # Arguments
    /// * `selector` - The label selector to match against zone labels
    /// * `namespace` - The namespace to search within
    ///
    /// # Returns
    /// A vector of (name, namespace) tuples for matching zones
    #[must_use]
    pub fn dnszones_matching_selector(
        &self,
        selector: &LabelSelector,
        namespace: &str,
    ) -> Vec<(String, String)> {
        self.dnszones
            .state()
            .iter()
            .filter(|zone| {
                zone.namespace().as_deref() == Some(namespace)
                    && crate::selector::matches_selector(selector, zone.labels())
            })
            .map(|zone| (zone.name_any(), zone.namespace().unwrap_or_default()))
            .collect()
    }

    /// Query `Bind9Instance`s matching a label selector.
    ///
    /// # Arguments
    /// * `selector` - The label selector to match against instance labels
    /// * `namespace` - The namespace to search within
    ///
    /// # Returns
    /// A vector of (name, namespace) tuples for matching instances
    #[must_use]
    pub fn bind9instances_matching_selector(
        &self,
        selector: &LabelSelector,
        namespace: &str,
    ) -> Vec<(String, String)> {
        self.bind9_instances
            .state()
            .iter()
            .filter(|inst| {
                inst.namespace().as_deref() == Some(namespace)
                    && crate::selector::matches_selector(selector, inst.labels())
            })
            .map(|inst| (inst.name_any(), inst.namespace().unwrap_or_default()))
            .collect()
    }

    /// Find all `DNSZone`s whose `recordsFrom` selector matches given record labels.
    ///
    /// This is a "reverse lookup" - given a record's labels, find which zones select it.
    /// Used by record watch mappers to determine which zones need reconciliation
    /// when a record changes.
    ///
    /// # Arguments
    /// * `record_labels` - The labels of the record to match
    /// * `record_namespace` - The namespace of the record
    ///
    /// # Returns
    /// A vector of (name, namespace) tuples for zones that select this record
    #[must_use]
    pub fn dnszones_selecting_record(
        &self,
        record_labels: &BTreeMap<String, String>,
        record_namespace: &str,
    ) -> Vec<(String, String)> {
        self.dnszones
            .state()
            .iter()
            .filter(|zone| {
                zone.namespace().as_deref() == Some(record_namespace)
                    && zone.spec.records_from.as_ref().is_some_and(|sources| {
                        sources.iter().any(|source| {
                            crate::selector::matches_selector(&source.selector, record_labels)
                        })
                    })
            })
            .map(|zone| (zone.name_any(), zone.namespace().unwrap_or_default()))
            .collect()
    }

    /// Get a specific `DNSZone` by name and namespace from the store.
    ///
    /// # Arguments
    /// * `name` - The name of the zone
    /// * `namespace` - The namespace of the zone
    ///
    /// # Returns
    /// An [`Arc<DNSZone>`] if found, `None` otherwise
    #[must_use]
    pub fn get_dnszone(&self, name: &str, namespace: &str) -> Option<Arc<DNSZone>> {
        self.dnszones
            .state()
            .iter()
            .find(|zone| zone.name_any() == name && zone.namespace().as_deref() == Some(namespace))
            .cloned()
    }

    /// Get a specific `Bind9Instance` by name and namespace from the store.
    ///
    /// # Arguments
    /// * `name` - The name of the instance
    /// * `namespace` - The namespace of the instance
    ///
    /// # Returns
    /// An [`Arc<Bind9Instance>`] if found, `None` otherwise
    #[must_use]
    pub fn get_bind9instance(&self, name: &str, namespace: &str) -> Option<Arc<Bind9Instance>> {
        self.bind9_instances
            .state()
            .iter()
            .find(|inst| inst.name_any() == name && inst.namespace().as_deref() == Some(namespace))
            .cloned()
    }

    /// Get a specific `Deployment` by name and namespace from the store.
    ///
    /// # Arguments
    /// * `name` - The name of the deployment
    /// * `namespace` - The namespace of the deployment
    ///
    /// # Returns
    /// An [`Arc<Deployment>`] if found, `None` otherwise
    #[must_use]
    pub fn get_deployment(&self, name: &str, namespace: &str) -> Option<Arc<Deployment>> {
        self.bind9_deployments
            .state()
            .iter()
            .find(|dep| {
                dep.metadata.name.as_deref() == Some(name)
                    && dep.metadata.namespace.as_deref() == Some(namespace)
            })
            .cloned()
    }

    /// Create a `Bind9Manager` for a specific instance with deployment-aware auth.
    ///
    /// This helper function looks up the deployment for the given instance and creates
    /// a `Bind9Manager` with proper authentication detection. If the deployment is found,
    /// it creates a manager that can determine auth status by inspecting the bindcar
    /// container's environment variables. If not found, it falls back to a basic manager
    /// that assumes auth is enabled.
    ///
    /// # Arguments
    /// * `instance_name` - Name of the `Bind9Instance`
    /// * `instance_namespace` - Namespace of the instance
    ///
    /// # Returns
    /// A `Bind9Manager` configured for the instance
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use bindy::context::Stores;
    /// # fn example(stores: &Stores) {
    /// let manager = stores.create_bind9_manager_for_instance(
    ///     "my-instance",
    ///     "dns-system"
    /// );
    /// # }
    /// ```
    #[must_use]
    pub fn create_bind9_manager_for_instance(
        &self,
        instance_name: &str,
        instance_namespace: &str,
    ) -> crate::bind9::Bind9Manager {
        // Try to get the deployment for this instance
        if let Some(deployment) = self.get_deployment(instance_name, instance_namespace) {
            // Found deployment - create manager with auth detection
            crate::bind9::Bind9Manager::new_with_deployment(
                deployment,
                instance_name.to_string(),
                instance_namespace.to_string(),
            )
        } else {
            // No deployment found - fall back to basic manager (auth assumed enabled)
            tracing::debug!(
                instance = instance_name,
                namespace = instance_namespace,
                "Deployment not found in store, using basic Bind9Manager (auth enabled)"
            );
            crate::bind9::Bind9Manager::new()
        }
    }
}

/// Enum representing a reference to any DNS record type.
///
/// This enum provides a type-safe way to reference records of different types
/// in a unified collection. Each variant contains the name and namespace of the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordRef {
    /// A record (IPv4 address)
    A(String, String),
    /// AAAA record (IPv6 address)
    AAAA(String, String),
    /// CNAME record (canonical name)
    CNAME(String, String),
    /// TXT record (text data)
    TXT(String, String),
    /// MX record (mail exchange)
    MX(String, String),
    /// NS record (name server)
    NS(String, String),
    /// SRV record (service locator)
    SRV(String, String),
    /// CAA record (certificate authority authorization)
    CAA(String, String),
}

impl RecordRef {
    /// Get the name of the record.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            RecordRef::A(name, _)
            | RecordRef::AAAA(name, _)
            | RecordRef::CNAME(name, _)
            | RecordRef::TXT(name, _)
            | RecordRef::MX(name, _)
            | RecordRef::NS(name, _)
            | RecordRef::SRV(name, _)
            | RecordRef::CAA(name, _) => name,
        }
    }

    /// Get the namespace of the record.
    #[must_use]
    pub fn namespace(&self) -> &str {
        match self {
            RecordRef::A(_, ns)
            | RecordRef::AAAA(_, ns)
            | RecordRef::CNAME(_, ns)
            | RecordRef::TXT(_, ns)
            | RecordRef::MX(_, ns)
            | RecordRef::NS(_, ns)
            | RecordRef::SRV(_, ns)
            | RecordRef::CAA(_, ns) => ns,
        }
    }

    /// Get the record type as a string.
    #[must_use]
    pub fn record_type(&self) -> &str {
        match self {
            RecordRef::A(_, _) => "A",
            RecordRef::AAAA(_, _) => "AAAA",
            RecordRef::CNAME(_, _) => "CNAME",
            RecordRef::TXT(_, _) => "TXT",
            RecordRef::MX(_, _) => "MX",
            RecordRef::NS(_, _) => "NS",
            RecordRef::SRV(_, _) => "SRV",
            RecordRef::CAA(_, _) => "CAA",
        }
    }
}

/// Metrics for observability.
///
/// This struct will hold Prometheus metrics for monitoring controller behavior.
/// For now, it's a placeholder that can be extended with actual metrics.
#[derive(Clone, Default)]
pub struct Metrics {
    // Future: Add prometheus metrics here
    // pub reconciliations_total: IntCounter,
    // pub reconciliation_errors_total: IntCounter,
    // pub reconciliation_duration: Histogram,
    // pub store_size_dnszones: IntGauge,
    // pub store_size_records: IntGauge,
    // pub store_size_instances: IntGauge,
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
