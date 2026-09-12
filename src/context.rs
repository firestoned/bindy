// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Shared context for all operators with reflector stores.
//!
//! This module provides the core infrastructure for the shared reflector store pattern.
//! All operators receive an `Arc<Context>` that contains:
//! - Kubernetes client
//! - Reflector stores for all CRD types
//! - Metrics registry
//!
//! The stores enable O(1) in-memory lookups for label-based resource selection,
//! eliminating the need for API queries in watch mappers.

use crate::crd::{
    AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord, ClusterBind9Provider,
    DNSZone, LabelSelector, MXRecord, NSRecord, PTRRecord, SRVRecord, TXTRecord,
};
use k8s_openapi::api::apps::v1::Deployment;
use kube::runtime::reflector::Store;
use kube::{Client, ResourceExt};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A reflector view over one or more namespace-scoped watches.
///
/// When the operator runs cluster-wide ([`NamespaceScope::All`]) this holds exactly
/// one shard built from `Api::all`, and every operation is a direct pass-through —
/// the cluster-wide deployment behaves exactly as it did before namespace scoping
/// existed.
///
/// When the operator is scoped to a namespace set it holds **one shard per
/// namespace**. That sharding is load-bearing, not an implementation detail: a
/// single reflector `Store` cannot be fed by several namespace watches merged with
/// `select_all`, because `watcher::Event::InitDone` makes the store *replace* its
/// entire contents with the buffer of whichever watch just finished listing
/// (`kube_runtime::reflector::store` does `mem::swap(&mut *store, &mut self.buffer)`).
/// Merging N watches into one writer would leave the store holding only the last
/// namespace to sync — silently, and again on every watch reconnect. Sharding keeps
/// each watch's `Init`/`InitDone` cycle confined to its own store.
///
/// [`NamespaceScope::All`]: crate::namespace_scope::NamespaceScope::All
#[derive(Clone)]
pub struct MultiStore<K>
where
    K: kube::Resource + Clone + 'static,
    K::DynamicType: std::hash::Hash + Eq + Clone + std::fmt::Debug + Default,
{
    shards: Vec<Store<K>>,
}

impl<K> MultiStore<K>
where
    K: kube::Resource + Clone + 'static,
    K::DynamicType: std::hash::Hash + Eq + Clone + std::fmt::Debug + Default,
{
    /// Build a view over the given shards.
    ///
    /// # Panics
    /// Panics if `shards` is empty. An empty view would make every lookup return
    /// nothing while the operator reported itself healthy — a far worse failure
    /// than a loud one at startup.
    #[must_use]
    pub fn new(shards: Vec<Store<K>>) -> Self {
        assert!(
            !shards.is_empty(),
            "MultiStore requires at least one shard; an empty view would silently \
             make every reflector lookup return nothing"
        );
        Self { shards }
    }

    /// All objects across every shard.
    ///
    /// Shards are disjoint by construction (one namespace each, or a single
    /// cluster-wide shard), so no de-duplication is needed.
    #[must_use]
    pub fn state(&self) -> Vec<Arc<K>> {
        // Fast path: the cluster-wide default is a single shard. Return its state
        // directly so the default deployment allocates exactly as it did before.
        if let [only] = self.shards.as_slice() {
            return only.state();
        }
        self.shards.iter().flat_map(Store::state).collect()
    }

    /// Number of shards backing this view (1 when cluster-wide).
    #[must_use]
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }
}

/// Shared context passed to all operators.
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

    /// The set of namespaces this operator watches and manages.
    ///
    /// Drives how every controller builds its `Api` handles: [`NamespaceScope::All`]
    /// (the default) uses `Api::all` and needs cluster-wide RBAC, while a namespace
    /// list uses `Api::namespaced` per namespace and needs only RoleBindings.
    ///
    /// [`NamespaceScope::All`]: crate::namespace_scope::NamespaceScope::All
    pub namespace_scope: crate::namespace_scope::NamespaceScope,
}

/// Collection of all reflector stores for cross-operator queries.
///
/// Each store is populated by a dedicated reflector task and provides
/// in-memory access to resources without API calls.
#[derive(Clone)]
pub struct Stores {
    // Cluster-scoped resources
    pub cluster_bind9_providers: MultiStore<ClusterBind9Provider>,

    // Namespace-scoped resources
    pub bind9_clusters: MultiStore<Bind9Cluster>,
    pub bind9_instances: MultiStore<Bind9Instance>,
    pub bind9_deployments: MultiStore<Deployment>,
    pub dnszones: MultiStore<DNSZone>,

    // DNS Record types
    pub a_records: MultiStore<ARecord>,
    pub aaaa_records: MultiStore<AAAARecord>,
    pub cname_records: MultiStore<CNAMERecord>,
    pub txt_records: MultiStore<TXTRecord>,
    pub mx_records: MultiStore<MXRecord>,
    pub ns_records: MultiStore<NSRecord>,
    pub srv_records: MultiStore<SRVRecord>,
    pub caa_records: MultiStore<CAARecord>,
    pub ptr_records: MultiStore<PTRRecord>,
}

impl Stores {
    /// Query all record stores and return matching records for a label selector.
    ///
    /// This method searches across all 9 record type stores to find records that:
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
        collect_matching!(self.ptr_records, PTR);

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
    ///     "bindy-system"
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
    /// PTR record (reverse DNS pointer)
    PTR(String, String),
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
            | RecordRef::CAA(name, _)
            | RecordRef::PTR(name, _) => name,
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
            | RecordRef::CAA(_, ns)
            | RecordRef::PTR(_, ns) => ns,
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
            RecordRef::PTR(_, _) => "PTR",
        }
    }
}

/// Metrics for observability.
///
/// This struct will hold Prometheus metrics for monitoring operator behavior.
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
