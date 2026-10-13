// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Shared context for all controllers, and the reflector stores behind it.
//!
//! Every controller receives an `Arc<Context>` holding the Kubernetes client,
//! the shared watch layer and the [`Stores`] it fills, so watch mappers and
//! reconcilers answer label-selection questions from memory instead of the
//! API server.
//!
//! The nine DNS record kinds are handled generically through [`RecordKind`]
//! and the [`RECORD_KINDS`] list, both generated from the record-kind table
//! in `bindy-api` (ADR-0020): adding a record kind there reaches the stores,
//! the watch wiring and the selector queries with no edit here.

use crate::watch::{MultiStore, WatchSet};
use bindy_api::crd::{
    Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSRecordKind, DNSZone,
    RecordReferenceWithTimestamp, RecordStatus,
};
use bindy_api::selector::matches_selector;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Endpoints, Pod};
use kube::core::object::HasSpec;
use kube::core::NamespaceResourceScope;
use kube::{Client, Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::sync::Arc;

/// Shared context passed to all controllers.
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client for API operations
    pub client: Client,

    /// Reflector stores for all cached kinds
    pub stores: Stores,

    /// The shared watch layer behind [`Self::stores`]: one watch and cache per
    /// kind and namespace target. Controllers subscribe to it instead of
    /// opening their own watches (ADR-0009 §3).
    pub watch: Arc<WatchSet>,

    /// HTTP client for bindcar zone synchronization API calls
    pub http_client: reqwest::Client,

    /// The set of namespaces this operator watches and manages.
    ///
    /// Drives how every controller builds its `Api` handles: [`NamespaceScope::All`]
    /// (the default) uses `Api::all` and needs cluster-wide RBAC, while a namespace
    /// list uses `Api::namespaced` per namespace and needs only RoleBindings.
    ///
    /// [`NamespaceScope::All`]: crate::namespace_scope::NamespaceScope::All
    pub namespace_scope: crate::namespace_scope::NamespaceScope,

    /// Fires on SIGTERM, SIGINT or loss of the leader lease. Every controller
    /// passes [`ShutdownSignal::wait`] to `graceful_shutdown_on`, so a shutdown
    /// drains in-flight reconciles (ADR-0009 §5).
    ///
    /// [`ShutdownSignal::wait`]: crate::shutdown::ShutdownSignal::wait
    pub shutdown: crate::shutdown::ShutdownSignal,
}

/// Timeout for one HTTP call to a bindcar sidecar.
const BINDCAR_HTTP_TIMEOUT_SECS: u64 = 10;

impl Context {
    /// Build the shared context: the `WatchSet` with every kind the operator
    /// caches, the stores over it, and the bindcar HTTP client.
    ///
    /// Registering a kind starts its watch, so call this inside the Tokio
    /// runtime. Every controller subscribes to this one `WatchSet` instead of
    /// opening its own watches (ADR-0009 §3).
    ///
    /// # Arguments
    /// * `client` - The rate-limited Kubernetes client (ADR-0005)
    /// * `scope` - Cluster-wide, or the namespaces to watch
    /// * `shutdown` - The signal controllers drain on
    ///
    /// # Errors
    /// Returns an error if the bindcar HTTP client cannot be built.
    pub fn new(
        client: Client,
        scope: crate::namespace_scope::NamespaceScope,
        shutdown: crate::shutdown::ShutdownSignal,
    ) -> anyhow::Result<Self> {
        use crate::namespace_scope::NamespaceScope;

        match &scope {
            NamespaceScope::All => {
                tracing::info!("Namespace scope: ALL (cluster-wide), requires cluster-wide RBAC");
            }
            NamespaceScope::Namespaces(ns) => {
                tracing::info!(
                    namespaces = ?ns,
                    "Namespace scope: RESTRICTED, one watch per namespace, needs only per-namespace RoleBindings"
                );
            }
        }

        // One shared watch and cache per (kind, namespace target). See
        // `MultiStore` for why the namespaces are separate shards rather than
        // one merged store.
        let mut watch = WatchSet::new(client.clone(), scope.clone());
        // Cluster-scoped: always one cluster-wide watch, in every scope mode. Even
        // a fully namespace-scoped operator keeps a slim `ClusterRole` granting
        // `get/list/watch` on `clusterbind9providers`: the irreducible residue of
        // cluster-wide RBAC, and the reason M-22 cannot claim to eliminate
        // cluster-wide access entirely.
        let cluster_bind9_providers =
            watch.register_cluster::<ClusterBind9Provider>("ClusterBind9Provider");
        let bind9_clusters = watch.register::<Bind9Cluster>("Bind9Cluster");
        let bind9_instances = watch.register::<Bind9Instance>("Bind9Instance");
        // Deployments are filtered to those owned by a Bind9Instance: the operator
        // has no interest in every Deployment in every watched namespace.
        let bind9_deployments =
            watch.register_filtered::<Deployment, _>("Deployment", |deployment| {
                deployment
                    .metadata
                    .owner_references
                    .as_ref()
                    .is_some_and(|owners| owners.iter().any(|owner| owner.kind == "Bind9Instance"))
            });
        let dnszones = watch.register::<DNSZone>("DNSZone");
        // Every record kind, from the one list (RECORD_KINDS).
        let mut records = RecordStores::default();
        for ops in &RECORD_KINDS {
            (ops.register)(&mut watch, &mut records);
        }
        // Endpoints of bindy's own Services only (label-selected on the API
        // server): the zone controller's signal that a BIND9 pod was replaced,
        // and the cache the record and zone writes resolve a BIND9 instance's
        // pod endpoints from instead of a GET per write (ADR-0015).
        // One watch per namespace target, like every other kind, so namespace-
        // restricted mode needs only its per-namespace `endpoints` Role.
        let endpoints = watch
            .register_selected::<Endpoints>("Endpoints", bindy_api::labels::BINDY_PART_OF_SELECTOR);
        // bindy's BIND9 pods only (label-selected on the API server): the
        // zones-loaded gate controller's primary resource, and how a write
        // tells a pod whose containers are ready but whose readiness gate is
        // still closed from a pod that cannot take writes yet (ADR-0017).
        let bind9_pods =
            watch.register_selected::<Pod>("Pod", bindy_api::labels::BIND9_POD_SELECTOR);

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(BINDCAR_HTTP_TIMEOUT_SECS))
            .build()?;

        tracing::info!("Shared context initialized with one watch per cached kind");

        Ok(Self {
            client,
            stores: Stores {
                cluster_bind9_providers,
                bind9_clusters,
                bind9_instances,
                bind9_deployments,
                endpoints,
                bind9_pods,
                dnszones,
                records,
            },
            watch: Arc::new(watch),
            http_client,
            namespace_scope: scope,
            shutdown,
        })
    }
}

/// What a DNS record CRD kind is: the one trait every record kind implements
/// (ADR-0020).
///
/// Generated for every row of the record-kind table
/// ([`bindy_api::for_each_record_kind`]); [`RECORD_KINDS`] lists the kinds.
/// The kind string, finalizer and hickory `RecordType` all come from
/// [`Self::DNS_KIND`], so they cannot disagree. The spec is reached through
/// the kube-derived [`HasSpec`]; how a kind's spec becomes DNS data is the
/// `ReconcilableRecord` impl in `bindy-bind9`, the one hand-written part.
pub trait RecordKind:
    Resource<DynamicType = (), Scope = NamespaceResourceScope>
    + HasSpec<Spec: Serialize + Clone + Debug + Send + Sync + 'static>
    + Clone
    + Debug
    + Send
    + Sync
    + Serialize
    + DeserializeOwned
    + 'static
{
    /// This kind's row of the record-kind table.
    const DNS_KIND: DNSRecordKind;

    /// The kind name, e.g. `ARecord`.
    const KIND: &'static str = Self::DNS_KIND.as_str();

    /// The record's status, if it has one.
    fn record_status(&self) -> Option<&RecordStatus>;

    /// The record's DNS name from its spec (`spec.name`, e.g. `www` or `@`).
    fn spec_name(&self) -> &str;

    /// The record's TTL from its spec (`spec.ttl`), if set.
    fn ttl(&self) -> Option<i32>;
}

/// Generates the [`RecordKind`] impls and [`RECORD_KINDS`] from the
/// record-kind table.
macro_rules! record_kinds {
    (@rows $( ($variant:ident, $record:ty, $($rest:tt)*) ),* $(,)?) => {
        $(
            impl RecordKind for $record {
                const DNS_KIND: DNSRecordKind = DNSRecordKind::$variant;
                fn record_status(&self) -> Option<&RecordStatus> {
                    self.status.as_ref()
                }
                fn spec_name(&self) -> &str {
                    &self.spec.name
                }
                fn ttl(&self) -> Option<i32> {
                    self.spec.ttl
                }
            }
        )*

        /// Every record kind, in the order record discovery reports them
        /// ([`DNSRecordKind::all`]).
        pub const RECORD_KINDS: [RecordKindOps; DNSRecordKind::COUNT] = [
            $( ops::<$record>(), )*
        ];
    };
}

bindy_api::for_each_record_kind!([record_kinds]);

/// The type-erased operations for one record kind.
#[derive(Clone, Copy)]
pub struct RecordKindOps {
    /// The kind name
    pub kind: &'static str,
    /// Register the kind in the [`WatchSet`] and keep its view in the
    /// [`RecordStores`].
    pub register: fn(&mut WatchSet, &mut RecordStores),
    /// Add an empty view of the kind to the [`RecordStores`], for tests that
    /// run without a `WatchSet`.
    #[cfg(test)]
    pub insert_empty: fn(&mut RecordStores),
    collect_tagged: fn(&RecordStores, &str, &str, &mut Vec<RecordReferenceWithTimestamp>),
    wait_ready: fn(&RecordStores) -> futures::future::BoxFuture<'_, anyhow::Result<()>>,
}

const fn ops<K: RecordKind>() -> RecordKindOps {
    RecordKindOps {
        kind: K::KIND,
        register: register_kind::<K>,
        #[cfg(test)]
        insert_empty: insert_empty_kind::<K>,
        collect_tagged: collect_tagged::<K>,
        wait_ready: wait_ready_kind::<K>,
    }
}

fn register_kind<K: RecordKind>(watch: &mut WatchSet, records: &mut RecordStores) {
    records.insert(watch.register::<K>(K::KIND));
}

fn wait_ready_kind<K: RecordKind>(
    records: &RecordStores,
) -> futures::future::BoxFuture<'_, anyhow::Result<()>> {
    Box::pin(records.get::<K>().wait_until_ready())
}

#[cfg(test)]
fn insert_empty_kind<K: RecordKind>(records: &mut RecordStores) {
    let (store, _writer) = kube::runtime::reflector::store::<K>();
    records.insert(MultiStore::new(vec![store]));
}

/// Push every record of kind `K` whose `status.zoneRef` names the zone
/// `zone_namespace`/`zone_name` and that is not being deleted.
fn collect_tagged<K: RecordKind>(
    records: &RecordStores,
    zone_namespace: &str,
    zone_name: &str,
    out: &mut Vec<RecordReferenceWithTimestamp>,
) {
    for record in records.get::<K>().state() {
        if record.meta().deletion_timestamp.is_some() {
            continue;
        }
        let Some(zone_ref) = record.record_status().and_then(|s| s.zone_ref.as_ref()) else {
            continue;
        };
        if zone_ref.namespace != zone_namespace || zone_ref.name != zone_name {
            continue;
        }
        out.push(RecordReferenceWithTimestamp {
            api_version: BINDY_API_VERSION.to_string(),
            kind: K::KIND.to_string(),
            name: record.name_any(),
            namespace: record.namespace().unwrap_or_default(),
            record_name: None,
            last_reconciled_at: None,
        });
    }
}

/// The API version of every bindy custom resource.
const BINDY_API_VERSION: &str = "bindy.firestoned.io/v1beta1";

/// The reflector views of every record kind, keyed by type.
#[derive(Clone, Default)]
pub struct RecordStores {
    stores: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl RecordStores {
    /// Keep `view` as the store of `K`, replacing any earlier one.
    pub fn insert<K: RecordKind>(&mut self, view: MultiStore<K>) {
        self.stores.insert(TypeId::of::<K>(), Arc::new(view));
    }

    /// Wait until the store of every kind in [`RECORD_KINDS`] has finished
    /// its initial list.
    ///
    /// # Errors
    /// Returns an error if a kind's watch ended before it ever listed.
    ///
    /// # Panics
    /// Panics if a kind was never inserted (see [`RecordStores::get`]).
    pub async fn wait_until_ready(&self) -> anyhow::Result<()> {
        for ops in &RECORD_KINDS {
            (ops.wait_ready)(self).await?;
        }
        Ok(())
    }

    /// The store of `K`.
    ///
    /// # Panics
    /// Panics if `K` was never inserted: a wiring bug, caught at startup.
    #[must_use]
    pub fn get<K: RecordKind>(&self) -> &MultiStore<K> {
        self.stores
            .get(&TypeId::of::<K>())
            .and_then(|any| any.downcast_ref::<MultiStore<K>>())
            .unwrap_or_else(|| panic!("{} is not registered in RecordStores", K::KIND))
    }
}

/// Collection of all reflector stores for cross-controller queries.
#[derive(Clone)]
pub struct Stores {
    /// Cluster-scoped providers
    pub cluster_bind9_providers: MultiStore<ClusterBind9Provider>,
    /// Namespaced clusters
    pub bind9_clusters: MultiStore<Bind9Cluster>,
    /// BIND9 instances
    pub bind9_instances: MultiStore<Bind9Instance>,
    /// Deployments owned by a `Bind9Instance`
    pub bind9_deployments: MultiStore<Deployment>,
    /// `Endpoints` of bindy's own Services (label-selected), one per BIND9
    /// instance: where a record or zone write finds the instance's ready pods
    /// without a GET per write (ADR-0015)
    pub endpoints: MultiStore<Endpoints>,
    /// bindy's BIND9 pods (label-selected, [`bindy_api::labels::BIND9_POD_SELECTOR`]):
    /// the zones-loaded gate's primary resource, and the container readiness
    /// of pods a readiness gate still holds out of their Service (ADR-0017)
    pub bind9_pods: MultiStore<Pod>,
    /// DNS zones
    pub dnszones: MultiStore<DNSZone>,
    /// Every record kind (see [`RECORD_KINDS`])
    pub records: RecordStores,
}

impl Stores {
    /// Wait until every store, the record stores included, has finished its
    /// initial list, so no controller reconciles against a store that still
    /// reads empty (the zones-loaded gate replays records from the record
    /// stores onto a new pod).
    ///
    /// # Errors
    /// Returns an error if a store's watch ended before it ever listed.
    pub async fn wait_until_ready(&self) -> anyhow::Result<()> {
        futures::try_join!(
            self.cluster_bind9_providers.wait_until_ready(),
            self.bind9_clusters.wait_until_ready(),
            self.bind9_instances.wait_until_ready(),
            self.bind9_deployments.wait_until_ready(),
            self.endpoints.wait_until_ready(),
            self.bind9_pods.wait_until_ready(),
            self.dnszones.wait_until_ready(),
            self.records.wait_until_ready(),
        )?;
        Ok(())
    }

    /// Every record, of every kind, tagged with a zone through its
    /// `status.zoneRef` and not being deleted, in [`RECORD_KINDS`] order.
    ///
    /// These are the records the record controller writes into the zone, so
    /// they are what a pod that is being given the zone must receive
    /// (ADR-0017).
    ///
    /// # Arguments
    /// * `zone_namespace` - Namespace of the `DNSZone`
    /// * `zone_name` - Name of the `DNSZone` resource
    ///
    /// # Returns
    /// One reference per tagged record, without timestamps
    #[must_use]
    pub fn records_tagged_with_zone(
        &self,
        zone_namespace: &str,
        zone_name: &str,
    ) -> Vec<RecordReferenceWithTimestamp> {
        let mut results = Vec::new();
        for ops in &RECORD_KINDS {
            (ops.collect_tagged)(&self.records, zone_namespace, zone_name, &mut results);
        }
        results
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
                        sources
                            .iter()
                            .any(|source| matches_selector(&source.selector, record_labels))
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
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
