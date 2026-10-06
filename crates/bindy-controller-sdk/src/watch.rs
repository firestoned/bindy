// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The shared watch layer (ADR-0009 §3).
//!
//! A [`WatchSet`] holds one watcher per (kind, namespace target). Each watcher
//! keeps that target's reflector [`Store`] and fans every event out to the
//! controllers subscribed to it, so a kind is watched once however many
//! controllers react to it, and every controller sees the same cache.
//!
//! kube-runtime's own shared stores (`store_shared`) are not used: their
//! subscribers receive Apply events only, never Deletes, and bindy's
//! controllers react to deletes (an owned `Deployment` removed, a record
//! finally gone). This module broadcasts `InitApply`, `Apply` and `Delete`
//! alike, and controllers consume the streams through kube's
//! `Controller::for_stream` / `watches_stream` / `owns_stream`.

use crate::metrics::{record_watch_error, record_watch_event, record_watch_restart};
use crate::namespace_scope::{scoped_namespaced_api, NamespaceScope};
use async_broadcast::{InactiveReceiver, Sender};
use futures::{Stream, StreamExt};
use kube::runtime::reflector::{self, Store};
use kube::runtime::{predicates, watcher, Predicate, WatchStreamExt};
use kube::{Api, Client, Resource};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tracing::warn;

/// Events buffered per shard before the watcher waits for its slowest
/// subscriber. Backpressure, never loss: a dropped event is a missed reconcile.
pub const WATCH_BROADCAST_CAPACITY: usize = 1024;

/// First delay before restarting a watch stream that ended.
pub const WATCH_RESTART_BACKOFF_INITIAL: Duration = Duration::from_secs(1);

/// Longest delay between restarts of a watch stream that keeps ending.
pub const WATCH_RESTART_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Label used for the namespace of a cluster-wide shard.
const ALL_NAMESPACES_LABEL: &str = "<all>";

/// Delay schedule for restarting a watch stream that ended.
///
/// The watcher itself already retries failed requests with backoff; this only
/// applies when its stream ends altogether, which should not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestartBackoff {
    /// Delay before the first restart
    pub initial: Duration,
    /// Cap on the delay
    pub max: Duration,
}

impl RestartBackoff {
    /// The production schedule: 1s doubling to 60s.
    pub const DEFAULT: Self = Self {
        initial: WATCH_RESTART_BACKOFF_INITIAL,
        max: WATCH_RESTART_BACKOFF_MAX,
    };

    /// Delay before restart number `attempt` (0-based): `initial` doubled
    /// `attempt` times, capped at `max`.
    #[must_use]
    pub fn delay(&self, attempt: u32) -> Duration {
        self.initial
            .checked_mul(1_u32.checked_shl(attempt).unwrap_or(u32::MAX))
            .unwrap_or(self.max)
            .min(self.max)
    }
}

/// A reflector view over one or more namespace-scoped watches.
///
/// When the operator runs cluster-wide ([`NamespaceScope::All`]) this holds exactly
/// one shard built from `Api::all`, and every operation is a direct pass-through.
///
/// When the operator is scoped to a namespace set it holds **one shard per
/// namespace**. That sharding is load-bearing, not an implementation detail: a
/// single reflector `Store` cannot be fed by several namespace watches merged with
/// `select_all`, because `watcher::Event::InitDone` makes the store *replace* its
/// entire contents with the buffer of whichever watch just finished listing
/// (`kube_runtime::reflector::store` does `mem::swap(&mut *store, &mut self.buffer)`).
/// Merging N watches into one writer would leave the store holding only the last
/// namespace to sync, silently, and again on every watch reconnect. Sharding keeps
/// each watch's `Init`/`InitDone` cycle confined to its own store.
#[derive(Clone)]
pub struct MultiStore<K>
where
    K: Resource + Clone + 'static,
    K::DynamicType: Hash + Eq + Clone + Debug + Default,
{
    shards: Vec<Store<K>>,
}

impl<K> MultiStore<K>
where
    K: Resource + Clone + 'static,
    K::DynamicType: Hash + Eq + Clone + Debug + Default,
{
    /// Build a view over the given shards.
    ///
    /// # Panics
    /// Panics if `shards` is empty. An empty view would make every lookup return
    /// nothing while the operator reported itself healthy, a far worse failure
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
        // Fast path: the cluster-wide default is a single shard.
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

/// One watcher for one (kind, namespace target): its store and its broadcast.
pub struct WatchShard<K>
where
    K: Resource + Clone + 'static,
    K::DynamicType: Hash + Eq + Clone,
{
    store: Store<K>,
    tx: Sender<Arc<K>>,
    // Keeps the channel open while no controller is subscribed, without
    // holding messages back (an inactive receiver gets nothing).
    _keep_open: InactiveReceiver<Arc<K>>,
    restarts: Arc<AtomicU64>,
    listing: Listing<K>,
}

/// Objects of the list in progress, broadcast but not yet visible in the
/// store.
///
/// kube's reflector store buffers `InitApply` objects and swaps them in only
/// at `InitDone`. A controller that subscribes half way through a list would
/// otherwise miss them: they were broadcast before its receiver existed, and
/// its store snapshot does not show them yet. The watcher records each one
/// here before broadcasting it and clears the list only after `InitDone` has
/// reached the store, so [`WatchShard::subscribe`] always finds it in one of
/// the three places.
type Listing<K> = Arc<Mutex<Vec<Arc<K>>>>;

/// Lock a [`Listing`], recovering it if a panicking thread poisoned it: it
/// holds plain values, so a poisoned one is still consistent.
fn lock_listing<K>(listing: &Listing<K>) -> std::sync::MutexGuard<'_, Vec<Arc<K>>> {
    listing.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<K> WatchShard<K>
where
    K: Resource + Clone + Debug + Send + Sync + 'static,
    K::DynamicType: Hash + Eq + Clone + Default + Send + Sync,
{
    /// Start the watcher for one shard.
    ///
    /// `source` is called to create the watcher event stream, and called again
    /// (after `backoff`) whenever that stream ends. `predicate` filters which
    /// objects reach the store and the subscribers; lifecycle events (`Init`,
    /// `InitDone`) always reach the store, since they drive its buffer swap.
    ///
    /// # Arguments
    /// * `kind` - Kind name, for logs and metrics
    /// * `target` - Namespace target, `None` when cluster-wide
    /// * `predicate` - Which objects to keep
    /// * `source` - Factory for the watcher event stream
    /// * `backoff` - Delay schedule for restarting an ended stream
    pub fn spawn<P, F, S>(
        kind: &'static str,
        target: Option<String>,
        predicate: P,
        source: F,
        backoff: RestartBackoff,
    ) -> Self
    where
        P: Fn(&K) -> bool + Send + Sync + 'static,
        F: Fn() -> S + Send + Sync + 'static,
        S: Stream<Item = Result<watcher::Event<K>, watcher::Error>> + Send + 'static,
    {
        let (store, writer) = reflector::store();
        let (mut tx, rx) = async_broadcast::broadcast(WATCH_BROADCAST_CAPACITY);
        // Broadcasting with no controller subscribed must not block the watcher.
        tx.set_await_active(false);
        let restarts = Arc::new(AtomicU64::new(0));
        let listing: Listing<K> = Arc::new(Mutex::new(Vec::new()));

        tokio::spawn(drive(
            kind,
            target.unwrap_or_else(|| ALL_NAMESPACES_LABEL.to_string()),
            predicate,
            source,
            backoff,
            writer,
            tx.clone(),
            restarts.clone(),
            listing.clone(),
        ));

        Self {
            store,
            tx,
            _keep_open: rx.deactivate(),
            restarts,
            listing,
        }
    }

    /// This shard's reflector store.
    #[must_use]
    pub fn store(&self) -> Store<K> {
        self.store.clone()
    }

    /// Times this shard's watch stream ended and was restarted.
    #[must_use]
    pub fn restarts(&self) -> u64 {
        self.restarts.load(Ordering::Relaxed)
    }

    /// A stream of every object this shard applies, for a controller.
    ///
    /// Yields the objects already known (those of a list in progress, then
    /// the store's contents), then every `InitApply`, `Apply` and `Delete`
    /// object from the moment of subscription. The live receiver is created
    /// first, then the list in progress is read, then the store: an object
    /// broadcast before the receiver existed is either still in the list in
    /// progress or, once that list is cleared after `InitDone`, already in
    /// the store, so nothing falls in between. An object can arrive twice,
    /// which a reconcile tolerates.
    pub fn subscribe(&self) -> impl Stream<Item = Result<K, watcher::Error>> + Send + 'static {
        let live = self.tx.new_receiver();
        // Order matters: the list in progress before the store (see Listing).
        let listing = lock_listing(&self.listing).clone();
        let snapshot = self.store.state();
        futures::stream::iter(listing)
            .chain(futures::stream::iter(snapshot))
            .chain(live)
            .map(|obj| Ok(K::clone(&obj)))
    }
}

/// The watcher loop for one shard: apply each kept event to the store,
/// broadcast its object, and restart the stream if it ever ends.
#[allow(clippy::too_many_arguments)]
async fn drive<K, P, F, S>(
    kind: &'static str,
    namespace: String,
    predicate: P,
    source: F,
    backoff: RestartBackoff,
    mut writer: reflector::store::Writer<K>,
    tx: Sender<Arc<K>>,
    restarts: Arc<AtomicU64>,
    listing: Listing<K>,
) where
    K: Resource + Clone + Debug + Send + Sync + 'static,
    K::DynamicType: Hash + Eq + Clone + Send + Sync,
    P: Fn(&K) -> bool + Send + Sync + 'static,
    F: Fn() -> S + Send + Sync + 'static,
    S: Stream<Item = Result<watcher::Event<K>, watcher::Error>> + Send + 'static,
{
    let mut attempt: u32 = 0;
    loop {
        let mut events = std::pin::pin!(source());
        while let Some(event) = events.next().await {
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    warn!(
                        kind,
                        namespace = %namespace,
                        diagnosis = %diagnose_watch_error(&error),
                        "Watch error"
                    );
                    record_watch_error(kind, &namespace);
                    continue;
                }
            };

            let object = match &event {
                watcher::Event::InitApply(o)
                | watcher::Event::Apply(o)
                | watcher::Event::Delete(o) => {
                    if !predicate(o) {
                        continue;
                    }
                    Some(Arc::new(o.clone()))
                }
                watcher::Event::Init | watcher::Event::InitDone => None,
            };

            // Store first: a controller woken by the broadcast looks the
            // object up in this store.
            writer.apply_watcher_event(&event);
            record_watch_event(kind, &namespace);
            attempt = 0;

            match &event {
                // A new list starts: forget what an interrupted one recorded.
                watcher::Event::Init => lock_listing(&listing).clear(),
                // Recorded before it is broadcast (see Listing).
                watcher::Event::InitApply(_) => {
                    if let Some(object) = &object {
                        lock_listing(&listing).push(object.clone());
                    }
                }
                // The store shows the list now; only then clear the record.
                watcher::Event::InitDone => lock_listing(&listing).clear(),
                watcher::Event::Apply(_) | watcher::Event::Delete(_) => {}
            }

            if let Some(object) = object {
                // An error here means no controller is subscribed yet (the
                // object is still in the store, or in the list in progress,
                // and a late subscriber gets it from there) or the channel is
                // closed.
                if tx.broadcast_direct(object).await.is_err() && tx.is_closed() {
                    // Every receiver is gone: the WatchSet was dropped.
                    return;
                }
            }
        }

        restarts.fetch_add(1, Ordering::Relaxed);
        record_watch_restart(kind, &namespace);
        let delay = backoff.delay(attempt);
        attempt = attempt.saturating_add(1);
        warn!(kind, namespace = %namespace, ?delay, "Watch stream ended; restarting");
        tokio::time::sleep(delay).await;
    }
}

/// Converts a [`watcher::Error`] into a short, human-readable diagnosis string.
///
/// The kube-runtime watcher wraps all errors in a thin enum. This function
/// peels back the layers to surface the actionable cause: connection refused,
/// unauthorized, RBAC-forbidden, or a generic API / transport error.
#[must_use]
pub fn diagnose_watch_error(e: &watcher::Error) -> String {
    // Extract the phase label and the inner kube client error, handling the
    // two variants that don't carry a kube::Error directly.
    let (phase, client_err) = match e {
        watcher::Error::InitialListFailed(e) => ("initial list", e),
        watcher::Error::WatchStartFailed(e) => ("watch start", e),
        watcher::Error::WatchFailed(e) => ("watch stream", e),
        watcher::Error::WatchError(status) => {
            return format!(
                "API server returned error during watch: {} (HTTP {})",
                status.message, status.code
            );
        }
        watcher::Error::NoResourceVersion => {
            return "resource does not support watch (no resourceVersion returned)".to_string();
        }
    };

    let detail = match client_err {
        kube::Error::Api(status) => match status.code {
            401 => format!(
                "unauthorized — check credentials/token ({})",
                status.message
            ),
            403 => format!("forbidden — check RBAC permissions ({})", status.message),
            code => format!("API error HTTP {code} — {}", status.message),
        },
        kube::Error::Auth(e) => format!("authentication error — {e}"),
        kube::Error::Service(e) => format!("cannot connect to API server — {e}"),
        kube::Error::HyperError(e) => format!("HTTP transport error — {e}"),
        other => format!("{other}"),
    };

    format!("{phase} failed: {detail}")
}

type Shards<K> = Vec<(Option<String>, WatchShard<K>)>;

/// Pass an object only when the part of it a mapper reads has changed.
///
/// `key` hashes what the subscriber depends on; an object is passed the first
/// time it is seen, whenever its key changes, and whenever `cached` says it is
/// no longer in the store (the `WatchSet` updates the store before it
/// broadcasts, so that is a Delete). Everything else, typically another
/// controller stamping a timestamp into the object's status, is dropped, so it
/// cannot fan out into reconciles of every object the mapper returns.
/// Watch errors pass through unchanged.
///
/// Unlike [`primary_predicate`] (kube's `predicate_filter`), this passes
/// deletes, which a mapper that rebuilds derived state must see.
///
/// # Arguments
/// * `stream` - A subscriber stream from the `WatchSet`
/// * `key` - Hash of the fields the mapper reads
/// * `cached` - Whether the object is still in the store
pub fn changed_only<K, S, F, C>(
    stream: S,
    key: F,
    cached: C,
) -> impl Stream<Item = Result<K, watcher::Error>> + Send + 'static
where
    K: Resource + Send + 'static,
    S: Stream<Item = Result<K, watcher::Error>> + Send + 'static,
    F: Fn(&K) -> u64 + Send + 'static,
    C: Fn(&K) -> bool + Send + 'static,
{
    let mut seen: HashMap<(Option<String>, String), u64> = HashMap::new();
    stream.filter_map(move |item| {
        let pass = match &item {
            Err(_) => true,
            Ok(object) => {
                let id = (
                    object.meta().namespace.clone(),
                    object.meta().name.clone().unwrap_or_default(),
                );
                if cached(object) {
                    let current = key(object);
                    seen.insert(id, current) != Some(current)
                } else {
                    seen.remove(&id);
                    true
                }
            }
        };
        futures::future::ready(pass.then_some(item))
    })
}

/// The predicate for a controller's primary stream: passes a change to the
/// object's generation, finalizers, labels or annotations, and drops the rest,
/// which in practice is the controller's own status writes (ADR-0009 §4,
/// amended 2026-10-05).
///
/// Finalizers are included because kube's `finalizer()` helper adds the
/// finalizer and then waits for that patch's event to come back. Setting
/// `deletionTimestamp` bumps a custom resource's generation, so deletion
/// passes too. Apply it to the controller's subscriber stream with
/// `predicate_filter`; the shared watch and its other subscribers still see
/// every event.
#[must_use]
pub fn primary_predicate<K: Resource + 'static>() -> impl Predicate<K> + Send + Sync + 'static {
    predicates::generation::<K>
        .combine(predicates::finalizers::<K>)
        .combine(predicates::labels::<K>)
        .combine(predicates::annotations::<K>)
}

/// Watcher configuration that filters on the API server by label selector, so
/// objects outside the selection are neither sent nor cached.
///
/// # Arguments
/// * `label_selector` - A Kubernetes label selector, e.g. `app=foo`
#[must_use]
pub fn label_selected_config(label_selector: &str) -> watcher::Config {
    watcher::Config::default().labels(label_selector)
}

/// One shared watch per (kind, namespace target), built once at startup.
///
/// Register each cached kind once; controllers then take that kind's
/// [`store`](Self::store) and [`subscribe`](Self::subscribe) to its events
/// for the namespace target they run in.
pub struct WatchSet {
    client: Client,
    scope: NamespaceScope,
    kinds: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl WatchSet {
    /// An empty set for `client`, sharded per `scope`.
    #[must_use]
    pub fn new(client: Client, scope: NamespaceScope) -> Self {
        Self {
            client,
            scope,
            kinds: HashMap::new(),
        }
    }

    /// The namespace scope the set shards by.
    #[must_use]
    pub fn scope(&self) -> &NamespaceScope {
        &self.scope
    }

    /// Start one watch per namespace target for a namespaced kind.
    ///
    /// # Panics
    /// Panics if `K` is already registered.
    pub fn register<K>(&mut self, kind: &'static str) -> MultiStore<K>
    where
        K: Resource<Scope = kube::core::NamespaceResourceScope>
            + Clone
            + Debug
            + Send
            + Sync
            + serde::de::DeserializeOwned
            + 'static,
        K::DynamicType: Hash + Eq + Clone + Debug + Default + Send + Sync + Unpin,
    {
        self.register_filtered::<K, _>(kind, |_| true)
    }

    /// As [`register`](Self::register), keeping only objects that match
    /// `predicate` (in the store and in the broadcast).
    ///
    /// # Panics
    /// Panics if `K` is already registered.
    pub fn register_filtered<K, P>(&mut self, kind: &'static str, predicate: P) -> MultiStore<K>
    where
        K: Resource<Scope = kube::core::NamespaceResourceScope>
            + Clone
            + Debug
            + Send
            + Sync
            + serde::de::DeserializeOwned
            + 'static,
        K::DynamicType: Hash + Eq + Clone + Debug + Default + Send + Sync + Unpin,
        P: Fn(&K) -> bool + Clone + Send + Sync + 'static,
    {
        self.register_namespaced::<K, P>(kind, watcher::Config::default(), predicate)
    }

    /// As [`register`](Self::register), watching only objects that match
    /// `label_selector`, filtered by the API server (for kinds the operator
    /// only needs a labelled subset of, such as the `Endpoints` of its own
    /// Services).
    ///
    /// # Panics
    /// Panics if `K` is already registered.
    pub fn register_selected<K>(
        &mut self,
        kind: &'static str,
        label_selector: &str,
    ) -> MultiStore<K>
    where
        K: Resource<Scope = kube::core::NamespaceResourceScope>
            + Clone
            + Debug
            + Send
            + Sync
            + serde::de::DeserializeOwned
            + 'static,
        K::DynamicType: Hash + Eq + Clone + Debug + Default + Send + Sync + Unpin,
    {
        self.register_namespaced::<K, _>(kind, label_selected_config(label_selector), |_| true)
    }

    fn register_namespaced<K, P>(
        &mut self,
        kind: &'static str,
        config: watcher::Config,
        predicate: P,
    ) -> MultiStore<K>
    where
        K: Resource<Scope = kube::core::NamespaceResourceScope>
            + Clone
            + Debug
            + Send
            + Sync
            + serde::de::DeserializeOwned
            + 'static,
        K::DynamicType: Hash + Eq + Clone + Debug + Default + Send + Sync + Unpin,
        P: Fn(&K) -> bool + Clone + Send + Sync + 'static,
    {
        let mut shards: Shards<K> = Vec::new();
        for target in self.scope.api_targets() {
            let api: Api<K> = scoped_namespaced_api::<K>(&self.client, target);
            let config = config.clone();
            let source = move || watcher(api.clone(), config.clone()).default_backoff();
            let shard = WatchShard::spawn(
                kind,
                target.map(ToString::to_string),
                predicate.clone(),
                source,
                RestartBackoff::DEFAULT,
            );
            shards.push((target.map(ToString::to_string), shard));
        }
        self.insert(kind, shards)
    }

    /// Start the single cluster-wide watch for a cluster-scoped kind. Store
    /// and subscribe to it with `target = None`, in every scope mode.
    ///
    /// # Panics
    /// Panics if `K` is already registered.
    pub fn register_cluster<K>(&mut self, kind: &'static str) -> MultiStore<K>
    where
        K: Resource<Scope = kube::core::ClusterResourceScope>
            + Clone
            + Debug
            + Send
            + Sync
            + serde::de::DeserializeOwned
            + 'static,
        K::DynamicType: Hash + Eq + Clone + Debug + Default + Send + Sync + Unpin,
    {
        let api: Api<K> = Api::all(self.client.clone());
        let source = move || watcher(api.clone(), watcher::Config::default()).default_backoff();
        let shard = WatchShard::spawn(kind, None, |_: &K| true, source, RestartBackoff::DEFAULT);
        self.insert(kind, vec![(None, shard)])
    }

    /// The store of `K` for one namespace target (`None` when cluster-wide or
    /// for a cluster-scoped kind).
    ///
    /// # Panics
    /// Panics if `K` is not registered or has no shard for `target`: both are
    /// wiring bugs, caught at startup.
    #[must_use]
    pub fn store<K>(&self, target: Option<&str>) -> Store<K>
    where
        K: Resource + Clone + Debug + Send + Sync + 'static,
        K::DynamicType: Hash + Eq + Clone + Default + Send + Sync,
    {
        self.shard::<K>(target).store()
    }

    /// Subscribe to every object `K`'s watch applies for one namespace target.
    /// See [`WatchShard::subscribe`].
    ///
    /// # Panics
    /// Panics if `K` is not registered or has no shard for `target`.
    pub fn subscribe<K>(
        &self,
        target: Option<&str>,
    ) -> impl Stream<Item = Result<K, watcher::Error>> + Send + 'static
    where
        K: Resource + Clone + Debug + Send + Sync + 'static,
        K::DynamicType: Hash + Eq + Clone + Default + Send + Sync,
    {
        self.shard::<K>(target).subscribe()
    }

    /// Subscribe to `K`'s events in **every** namespace target, merged.
    ///
    /// For a controller whose trigger can sit in another namespace than the
    /// objects it reconciles: a zone in namespace A can be served by a
    /// `Bind9Instance` in namespace B, so A's zone controller must see B's
    /// instance and `Endpoints` events. Objects the mapper resolves outside the
    /// controller's own namespace are dropped by the controller, which only
    /// reconciles what is in its store. Costs no extra API watch.
    ///
    /// # Panics
    /// Panics if `K` is not registered.
    pub fn subscribe_all<K>(&self) -> impl Stream<Item = Result<K, watcher::Error>> + Send + 'static
    where
        K: Resource + Clone + Debug + Send + Sync + 'static,
        K::DynamicType: Hash + Eq + Clone + Default + Send + Sync,
    {
        let streams: Vec<_> = self
            .shards::<K>()
            .iter()
            .map(|(_, shard)| Box::pin(shard.subscribe()))
            .collect();
        futures::stream::select_all(streams)
    }

    fn insert<K>(&mut self, kind: &'static str, shards: Shards<K>) -> MultiStore<K>
    where
        K: Resource + Clone + Debug + Send + Sync + 'static,
        K::DynamicType: Hash + Eq + Clone + Debug + Default + Send + Sync,
    {
        let view = MultiStore::new(shards.iter().map(|(_, shard)| shard.store()).collect());
        let previous = self.kinds.insert(TypeId::of::<K>(), Box::new(shards));
        assert!(
            previous.is_none(),
            "{kind} registered twice in the WatchSet"
        );
        view
    }

    fn shards<K>(&self) -> &Shards<K>
    where
        K: Resource + Clone + 'static,
        K::DynamicType: Hash + Eq + Clone,
    {
        self.kinds
            .get(&TypeId::of::<K>())
            .and_then(|any| any.downcast_ref::<Shards<K>>())
            .unwrap_or_else(|| {
                panic!(
                    "{} is not registered in the WatchSet",
                    std::any::type_name::<K>()
                )
            })
    }

    fn shard<K>(&self, target: Option<&str>) -> &WatchShard<K>
    where
        K: Resource + Clone + 'static,
        K::DynamicType: Hash + Eq + Clone,
    {
        self.shards::<K>()
            .iter()
            .find(|(t, _)| t.as_deref() == target)
            .map(|(_, shard)| shard)
            .unwrap_or_else(|| {
                panic!(
                    "WatchSet has no shard of {} for namespace target {target:?}",
                    std::any::type_name::<K>()
                )
            })
    }
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod watch_tests;
