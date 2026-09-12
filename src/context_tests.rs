// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for context.rs

#[cfg(test)]
mod tests {
    use super::super::*;

    // --- MultiStore: the sharded reflector view (P1-2) ---

    use kube::runtime::reflector;
    use kube::runtime::watcher;

    /// Build a populated single-namespace shard.
    ///
    /// Objects are fed through a real `Init` -> `InitApply`* -> `InitDone` cycle,
    /// because that cycle is precisely what makes merging shards unsafe — see the
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
        // reported healthy — fail loudly at startup instead.
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
