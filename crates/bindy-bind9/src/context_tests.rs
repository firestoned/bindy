// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `context.rs` (the `StoresBind9Ext` helpers).

#[cfg(test)]
mod tests {
    use crate::context::{MultiStore, RecordStores, Stores, StoresBind9Ext};
    use crate::crd::{Bind9Cluster, Bind9Instance, ClusterBind9Provider, DNSZone};
    use k8s_openapi::api::apps::v1::Deployment;
    use k8s_openapi::api::core::v1::{Endpoints, Pod};
    use kube::runtime::{reflector, watcher};

    /// See `bind9::mod_tests`: reqwest is built with `rustls-no-provider`.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn empty<K>() -> MultiStore<K>
    where
        K: kube::Resource + Clone + 'static,
        K::DynamicType: Default + Eq + std::hash::Hash + Clone + std::fmt::Debug,
    {
        let (store, mut writer) = reflector::store();
        writer.apply_watcher_event(&watcher::Event::Init);
        writer.apply_watcher_event(&watcher::Event::InitDone);
        MultiStore::new(vec![store])
    }

    fn empty_stores() -> Stores {
        Stores {
            cluster_bind9_providers: empty::<ClusterBind9Provider>(),
            bind9_clusters: empty::<Bind9Cluster>(),
            bind9_instances: empty::<Bind9Instance>(),
            bind9_deployments: empty::<Deployment>(),
            endpoints: empty::<Endpoints>(),
            bind9_pods: empty::<Pod>(),
            dnszones: empty::<DNSZone>(),
            records: RecordStores::default(),
        }
    }

    /// Bug B15a: when the Deployment is not in the store yet, the fallback
    /// manager must still carry the instance namespace, or a TLS CA bundle
    /// is read from the wrong namespace.
    #[test]
    fn manager_without_a_deployment_keeps_the_instance_namespace() {
        ensure_crypto_provider();
        let stores = empty_stores();

        let manager = stores.create_bind9_manager_for_instance("inst", "dns");

        assert_eq!(manager.instance_namespace(), Some("dns"));
    }
}
