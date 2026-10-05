// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `context.rs`: the record-kind registry and the store
//! queries the zone controller's record discovery and watch mappers rely on.

#[cfg(test)]
mod tests {
    use super::super::{RecordKind, RecordRef, RecordStores, Stores, RECORD_KINDS};
    use crate::watch::MultiStore;
    use bindy_api::crd::{
        ARecord, Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSZone, LabelSelector,
        PTRRecord,
    };
    use k8s_openapi::api::apps::v1::Deployment;
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

    fn selector(labels: &[(&str, &str)]) -> LabelSelector {
        LabelSelector {
            match_labels: Some(
                labels
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            ),
            match_expressions: None,
        }
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

    #[test]
    fn record_kind_builds_its_record_ref_variant() {
        assert_eq!(
            ARecord::record_ref("a".into(), "ns".into()),
            RecordRef::A("a".into(), "ns".into())
        );
        assert_eq!(
            PTRRecord::record_ref("p".into(), "ns".into()),
            RecordRef::PTR("p".into(), "ns".into())
        );
    }

    #[test]
    fn records_matching_selector_finds_matching_records_of_every_kind() {
        let s = stores(
            vec![],
            vec![
                a_record("a1", "ns1", &[("app", "web")]),
                a_record("a2", "ns1", &[("app", "db")]),
            ],
            vec![ptr_record("p1", "ns1", &[("app", "web")])],
        );

        let found = s.records_matching_selector(&selector(&[("app", "web")]), "ns1");

        // Kinds come back in RECORD_KINDS order: A before PTR.
        assert_eq!(
            found,
            [
                RecordRef::A("a1".into(), "ns1".into()),
                RecordRef::PTR("p1".into(), "ns1".into())
            ]
        );
    }

    #[test]
    fn records_matching_selector_is_namespace_isolated() {
        let s = stores(
            vec![],
            vec![a_record("a1", "other", &[("app", "web")])],
            vec![],
        );
        assert!(s
            .records_matching_selector(&selector(&[("app", "web")]), "ns1")
            .is_empty());
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

    #[test]
    fn record_ref_accessors() {
        let r = RecordRef::SRV("svc".into(), "ns".into());
        assert_eq!(r.name(), "svc");
        assert_eq!(r.namespace(), "ns");
        assert_eq!(r.record_type(), "SRV");
    }
}

/// Carried over from `bindy`'s `context_tests.rs` when `context` moved here
/// (roadmap 01 Phase B step B3).
#[cfg(test)]
mod multistore_and_record_ref {
    use super::super::RecordRef;
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

    #[test]
    fn test_record_ref_name() {
        let record = RecordRef::A("test-record".to_string(), "default".to_string());
        assert_eq!(record.name(), "test-record");

        let record = RecordRef::AAAA("ipv6-record".to_string(), "default".to_string());
        assert_eq!(record.name(), "ipv6-record");
    }

    #[test]
    fn test_record_ref_namespace() {
        let record = RecordRef::A("test-record".to_string(), "bindy-system".to_string());
        assert_eq!(record.namespace(), "bindy-system");

        let record = RecordRef::TXT("txt-record".to_string(), "other-ns".to_string());
        assert_eq!(record.namespace(), "other-ns");
    }

    #[test]
    fn test_record_ref_record_type() {
        assert_eq!(
            RecordRef::A("test".to_string(), "default".to_string()).record_type(),
            "A"
        );
        assert_eq!(
            RecordRef::AAAA("test".to_string(), "default".to_string()).record_type(),
            "AAAA"
        );
        assert_eq!(
            RecordRef::CNAME("test".to_string(), "default".to_string()).record_type(),
            "CNAME"
        );
        assert_eq!(
            RecordRef::TXT("test".to_string(), "default".to_string()).record_type(),
            "TXT"
        );
        assert_eq!(
            RecordRef::MX("test".to_string(), "default".to_string()).record_type(),
            "MX"
        );
        assert_eq!(
            RecordRef::NS("test".to_string(), "default".to_string()).record_type(),
            "NS"
        );
        assert_eq!(
            RecordRef::SRV("test".to_string(), "default".to_string()).record_type(),
            "SRV"
        );
        assert_eq!(
            RecordRef::CAA("test".to_string(), "default".to_string()).record_type(),
            "CAA"
        );
    }

    #[test]
    fn test_record_ref_equality() {
        let record1 = RecordRef::A("test".to_string(), "default".to_string());
        let record2 = RecordRef::A("test".to_string(), "default".to_string());
        let record3 = RecordRef::A("other".to_string(), "default".to_string());
        let record4 = RecordRef::AAAA("test".to_string(), "default".to_string());

        assert_eq!(record1, record2);
        assert_ne!(record1, record3);
        assert_ne!(record1, record4);
    }

    #[test]
    fn test_record_ref_clone() {
        let record = RecordRef::A("test".to_string(), "default".to_string());
        let cloned = record.clone();

        assert_eq!(record, cloned);
        assert_eq!(record.name(), cloned.name());
        assert_eq!(record.namespace(), cloned.namespace());
        assert_eq!(record.record_type(), cloned.record_type());
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
    use k8s_openapi::api::core::v1::Endpoints;

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
