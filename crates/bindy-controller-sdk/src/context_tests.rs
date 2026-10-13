// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `context.rs`: the record-kind registry and the store
//! queries the zone controller's record discovery and watch mappers rely on.

#[cfg(test)]
mod tests {
    use super::super::{RecordStores, Stores, RECORD_KINDS};
    use crate::watch::MultiStore;
    use bindy_api::crd::{
        ARecord, Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSZone, PTRRecord,
    };
    use k8s_openapi::api::apps::v1::Deployment;
    use k8s_openapi::api::core::v1::{Endpoints, Pod};
    use kube::runtime::reflector::{self, Store};
    use kube::runtime::watcher;
    use serde::de::DeserializeOwned;
    use serde_json::json;
    use std::collections::BTreeMap;

    /// A store pre-filled with `objects`.
    fn store_of<K>(objects: Vec<K>) -> Store<K>
    where
        K: kube::Resource<DynamicType = ()> + Clone + 'static,
    {
        let (store, mut writer) = reflector::store::<K>();
        writer.apply_watcher_event(&watcher::Event::Init);
        for o in objects {
            writer.apply_watcher_event(&watcher::Event::InitApply(o));
        }
        writer.apply_watcher_event(&watcher::Event::InitDone);
        store
    }

    fn view<K>(objects: Vec<K>) -> MultiStore<K>
    where
        K: kube::Resource<DynamicType = ()> + Clone + 'static,
    {
        MultiStore::new(vec![store_of(objects)])
    }

    fn object<K: DeserializeOwned>(
        kind: &str,
        name: &str,
        ns: &str,
        labels: &[(&str, &str)],
        spec: serde_json::Value,
    ) -> K {
        let labels: BTreeMap<String, String> = labels
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": kind,
            "metadata": {"name": name, "namespace": ns, "labels": labels},
            "spec": spec,
        }))
        .expect("valid fixture")
    }

    fn a_record(name: &str, ns: &str, labels: &[(&str, &str)]) -> ARecord {
        object(
            "ARecord",
            name,
            ns,
            labels,
            json!({"name": "www", "ipv4Addresses": ["192.0.2.1"]}),
        )
    }

    fn ptr_record(name: &str, ns: &str, labels: &[(&str, &str)]) -> PTRRecord {
        object(
            "PTRRecord",
            name,
            ns,
            labels,
            json!({"name": "10", "target": "www.example.com."}),
        )
    }

    fn zone(name: &str, ns: &str, records_from: Option<&[(&str, &str)]>) -> DNSZone {
        let mut spec = json!({
            "zoneName": format!("{name}.example"),
            "soaRecord": {
                "primaryNs": "ns1.example.", "adminEmail": "admin.example.",
                "serial": 1, "refresh": 3600, "retry": 600, "expire": 604800, "negativeTtl": 300
            }
        });
        if let Some(labels) = records_from {
            let m: BTreeMap<&str, &str> = labels.iter().copied().collect();
            spec["recordsFrom"] = json!([{"selector": {"matchLabels": m}}]);
        }
        object("DNSZone", name, ns, &[], spec)
    }

    /// `spec_name` is the record's DNS name from its spec, the name record
    /// discovery publishes in `status.records[].recordName`.
    #[test]
    fn spec_name_reads_the_record_spec_name() {
        use super::super::RecordKind;

        assert_eq!(a_record("web", "team", &[]).spec_name(), "www");
        assert_eq!(ptr_record("rev", "team", &[]).spec_name(), "10");
    }

    /// Stores with the given zones and A/PTR records; every other record kind
    /// registered but empty.
    fn stores(zones: Vec<DNSZone>, a: Vec<ARecord>, ptr: Vec<PTRRecord>) -> Stores {
        let mut records = RecordStores::default();
        for ops in RECORD_KINDS {
            (ops.insert_empty)(&mut records);
        }
        records.insert(view(a));
        records.insert(view(ptr));
        Stores {
            cluster_bind9_providers: view::<ClusterBind9Provider>(vec![]),
            bind9_clusters: view::<Bind9Cluster>(vec![]),
            bind9_instances: view::<Bind9Instance>(vec![]),
            bind9_deployments: view::<Deployment>(vec![]),
            endpoints: view::<Endpoints>(vec![]),
            bind9_pods: view::<Pod>(vec![]),
            dnszones: view(zones),
            records,
        }
    }

    #[test]
    fn record_kinds_cover_all_nine_kinds_in_discovery_order() {
        let kinds: Vec<&str> = RECORD_KINDS.iter().map(|ops| ops.kind).collect();
        assert_eq!(
            kinds,
            [
                "ARecord",
                "AAAARecord",
                "CNAMERecord",
                "TXTRecord",
                "MXRecord",
                "NSRecord",
                "SRVRecord",
                "CAARecord",
                "PTRRecord"
            ]
        );
    }

    /// Tag a record fixture with `status.zoneRef` pointing at `zone`.
    fn tagged<K: serde::Serialize + DeserializeOwned>(record: K, zone: &str, ns: &str) -> K {
        let mut value = serde_json::to_value(record).expect("fixture serializes");
        value["status"] = json!({
            "conditions": [],
            "zoneRef": {
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "DNSZone",
                "name": zone,
                "namespace": ns,
                "zoneName": format!("{zone}.example"),
            }
        });
        serde_json::from_value(value).expect("tagged fixture")
    }

    /// The zones-loaded gate replays the records the record controller would
    /// write: every record tagged with the zone (ADR-0017).
    #[test]
    fn records_tagged_with_zone_returns_tagged_records_of_every_kind() {
        let s = stores(
            vec![],
            vec![
                tagged(a_record("a1", "ns1", &[]), "z1", "ns1"),
                tagged(a_record("a2", "ns1", &[]), "other", "ns1"),
                a_record("a3", "ns1", &[]),
            ],
            vec![tagged(ptr_record("p1", "ns1", &[]), "z1", "ns1")],
        );

        let found = s.records_tagged_with_zone("ns1", "z1");

        let refs: Vec<(String, String, String)> = found
            .iter()
            .map(|r| (r.kind.clone(), r.namespace.clone(), r.name.clone()))
            .collect();
        assert_eq!(
            refs,
            [
                ("ARecord".into(), "ns1".into(), "a1".into()),
                ("PTRRecord".into(), "ns1".into(), "p1".into()),
            ]
        );
    }

    /// A record being deleted must not be replayed: its finalizer is removing
    /// it from DNS.
    #[test]
    fn records_tagged_with_zone_skips_terminating_records() {
        let mut deleting = tagged(a_record("a1", "ns1", &[]), "z1", "ns1");
        deleting.metadata.deletion_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                k8s_openapi::jiff::Timestamp::now(),
            ));
        let s = stores(vec![], vec![deleting], vec![]);

        assert!(s.records_tagged_with_zone("ns1", "z1").is_empty());
    }

    #[test]
    fn records_tagged_with_zone_matches_the_zone_namespace() {
        let s = stores(
            vec![],
            vec![tagged(a_record("a1", "ns1", &[]), "z1", "ns2")],
            vec![],
        );

        assert!(s.records_tagged_with_zone("ns1", "z1").is_empty());
        assert_eq!(s.records_tagged_with_zone("ns2", "z1").len(), 1);
    }

    #[test]
    fn dnszones_selecting_record_matches_records_from_in_the_same_namespace() {
        let s = stores(
            vec![
                zone("z1", "ns1", Some(&[("app", "web")])),
                zone("z2", "ns1", Some(&[("app", "db")])),
                zone("z3", "other", Some(&[("app", "web")])),
                zone("z4", "ns1", None),
            ],
            vec![],
            vec![],
        );
        let labels: BTreeMap<String, String> = [("app".to_string(), "web".to_string())].into();

        assert_eq!(
            s.dnszones_selecting_record(&labels, "ns1"),
            [("z1".to_string(), "ns1".to_string())]
        );
    }

    #[test]
    fn get_dnszone_finds_by_name_and_namespace() {
        let s = stores(vec![zone("z1", "ns1", None)], vec![], vec![]);
        assert!(s.get_dnszone("z1", "ns1").is_some());
        assert!(s.get_dnszone("z1", "other").is_none());
        assert!(s.get_dnszone("missing", "ns1").is_none());
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn record_stores_panic_on_an_unregistered_kind() {
        let records = RecordStores::default();
        let _ = records.get::<ARecord>();
    }
}

/// Carried over from `bindy`'s `context_tests.rs` when `context` moved here
/// (roadmap 01 Phase B step B3).
#[cfg(test)]
mod multistore {
    use crate::watch::MultiStore;
    use bindy_api::crd::Bind9Instance;

    // --- MultiStore: the sharded reflector view (P1-2) ---

    use kube::runtime::reflector;
    use kube::runtime::watcher;

    /// Build a populated single-namespace shard.
    ///
    /// Objects are fed through a real `Init` -> `InitApply`* -> `InitDone` cycle,
    /// because that cycle is precisely what makes merging shards unsafe; see the
    /// `MultiStore` docs.
    fn shard_with(names: &[(&str, &str)]) -> reflector::Store<Bind9Instance> {
        let (store, mut writer) = reflector::store::<Bind9Instance>();
        writer.apply_watcher_event(&watcher::Event::Init);
        for (name, namespace) in names {
            let instance: Bind9Instance = serde_json::from_value(serde_json::json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "Bind9Instance",
                "metadata": { "name": name, "namespace": namespace },
                "spec": { "clusterRef": "c", "role": "primary" }
            }))
            .expect("valid Bind9Instance fixture");
            writer.apply_watcher_event(&watcher::Event::InitApply(instance));
        }
        writer.apply_watcher_event(&watcher::Event::InitDone);
        store
    }

    #[test]
    fn test_multistore_single_shard_passes_through() {
        let store = MultiStore::new(vec![shard_with(&[("a", "ns1"), ("b", "ns1")])]);
        assert_eq!(store.shard_count(), 1);
        assert_eq!(store.state().len(), 2);
    }

    #[test]
    fn test_multistore_concatenates_shards() {
        // The whole point: two namespace shards must both be visible. A single
        // reflector fed by two merged watches would show only one namespace here.
        let store = MultiStore::new(vec![
            shard_with(&[("a", "ns1")]),
            shard_with(&[("b", "ns2"), ("c", "ns2")]),
        ]);
        assert_eq!(store.shard_count(), 2);

        let names: Vec<String> = store
            .state()
            .iter()
            .map(|i| i.metadata.name.clone().unwrap_or_default())
            .collect();
        assert_eq!(names.len(), 3, "both shards must contribute: {names:?}");
        for expected in ["a", "b", "c"] {
            assert!(names.iter().any(|n| n == expected), "missing {expected}");
        }
    }

    #[test]
    fn test_multistore_resync_of_one_shard_does_not_clear_others() {
        // This is the regression the whole sharding design exists for. Re-running a
        // shard's Init/InitDone cycle (what happens on every watch reconnect) resets
        // THAT shard only; a merged single-store design would wipe every namespace.
        let shard_a = shard_with(&[("a", "ns1")]);
        let (shard_b, mut writer_b) = reflector::store::<Bind9Instance>();
        let store = MultiStore::new(vec![shard_a, shard_b]);

        // ns2 syncs for the first time.
        writer_b.apply_watcher_event(&watcher::Event::Init);
        let instance: Bind9Instance = serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": { "name": "b", "namespace": "ns2" },
            "spec": { "clusterRef": "c", "role": "primary" }
        }))
        .expect("valid Bind9Instance fixture");
        writer_b.apply_watcher_event(&watcher::Event::InitApply(instance));
        writer_b.apply_watcher_event(&watcher::Event::InitDone);

        assert_eq!(
            store.state().len(),
            2,
            "both namespaces present after ns2 sync"
        );

        // ns2 reconnects and re-lists as empty; ns1 must be untouched.
        writer_b.apply_watcher_event(&watcher::Event::Init);
        writer_b.apply_watcher_event(&watcher::Event::InitDone);

        let remaining: Vec<String> = store
            .state()
            .iter()
            .map(|i| i.metadata.name.clone().unwrap_or_default())
            .collect();
        assert_eq!(
            remaining,
            vec!["a".to_string()],
            "ns2's resync must not clear ns1's shard"
        );
    }

    #[test]
    #[should_panic(expected = "at least one shard")]
    fn test_multistore_rejects_empty_shard_list() {
        // An empty view would make every lookup return nothing while the operator
        // reported healthy, so fail loudly at startup instead.
        let _ = MultiStore::<Bind9Instance>::new(vec![]);
    }
}

#[cfg(test)]
mod context_new {
    use super::super::Context;
    use crate::namespace_scope::NamespaceScope;
    use crate::shutdown;
    use bindy_api::crd::{
        AAAARecord, ARecord, Bind9Cluster, Bind9Instance, CAARecord, CNAMERecord,
        ClusterBind9Provider, DNSZone, MXRecord, NSRecord, PTRRecord, SRVRecord, TXTRecord,
    };
    use k8s_openapi::api::apps::v1::Deployment;
    use k8s_openapi::api::core::v1::{Endpoints, Pod};

    fn unreachable_client() -> kube::Client {
        // Never dialled successfully: these tests only check what is registered.
        let config = kube::Config::new("http://127.0.0.1:1".parse().unwrap());
        kube::Client::try_from(config).unwrap()
    }

    #[tokio::test]
    async fn context_new_registers_every_cached_kind_per_namespace_target() {
        let scope = NamespaceScope::Namespaces(vec!["a".to_string(), "b".to_string()]);
        let (_trigger, signal) = shutdown::channel();

        let ctx = Context::new(unreachable_client(), scope, signal).expect("builds");

        // Cluster-scoped: one cluster-wide shard in every mode.
        let _ = ctx.watch.store::<ClusterBind9Provider>(None);
        for target in ["a", "b"] {
            let t = Some(target);
            let _ = ctx.watch.store::<Bind9Cluster>(t);
            let _ = ctx.watch.store::<Bind9Instance>(t);
            let _ = ctx.watch.store::<Deployment>(t);
            let _ = ctx.watch.store::<DNSZone>(t);
            let _ = ctx.watch.store::<Endpoints>(t);
            // The zones-loaded gate's label-selected BIND9 pod watch (ADR-0017).
            let _ = ctx.watch.store::<Pod>(t);
            let _ = ctx.watch.store::<ARecord>(t);
            let _ = ctx.watch.store::<AAAARecord>(t);
            let _ = ctx.watch.store::<TXTRecord>(t);
            let _ = ctx.watch.store::<CNAMERecord>(t);
            let _ = ctx.watch.store::<MXRecord>(t);
            let _ = ctx.watch.store::<NSRecord>(t);
            let _ = ctx.watch.store::<SRVRecord>(t);
            let _ = ctx.watch.store::<CAARecord>(t);
            let _ = ctx.watch.store::<PTRRecord>(t);
        }
        assert_eq!(ctx.stores.dnszones.shard_count(), 2);
        assert_eq!(ctx.stores.cluster_bind9_providers.shard_count(), 1);
        assert_eq!(ctx.stores.bind9_pods.shard_count(), 2);
    }

    #[tokio::test]
    async fn context_new_carries_the_shutdown_signal() {
        let (trigger, signal) = shutdown::channel();
        let ctx = Context::new(unreachable_client(), NamespaceScope::All, signal).expect("builds");
        assert!(!ctx.shutdown.is_triggered());
        trigger.fire();
        assert!(ctx.shutdown.is_triggered());
    }
}

/// The zones-loaded gate reads the record stores (`records_tagged_with_zone`),
/// so the startup gate must wait for them too: an unlisted record store would
/// replay an empty record list onto a new pod and admit it (B16).
#[cfg(test)]
mod record_stores_ready_tests {
    use crate::context::{RecordStores, RECORD_KINDS};
    use std::time::Duration;

    /// Long enough for a ready store to answer, short enough to keep the suite fast.
    const NOT_READY_PROBE: Duration = Duration::from_millis(50);

    #[tokio::test]
    async fn unlisted_record_stores_are_not_ready() {
        let mut records = RecordStores::default();
        for ops in &RECORD_KINDS {
            (ops.insert_empty)(&mut records);
        }

        let waited = tokio::time::timeout(NOT_READY_PROBE, records.wait_until_ready()).await;

        // Either still waiting or an error (these test views have no
        // writer): never "ready".
        assert!(
            !matches!(waited, Ok(Ok(()))),
            "record stores that never listed must not report ready"
        );
    }
}
