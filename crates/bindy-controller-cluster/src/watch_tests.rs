// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `watch.rs`.

#[cfg(test)]
mod tests {
    use super::super::{clusters_for_instance, providers_for_instance};
    use crate::crd::{Bind9Instance, Bind9InstanceSpec, ServerRole};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
    use kube::api::ObjectMeta;

    #[allow(deprecated)]
    fn instance(cluster_ref: &str, owner: Option<&str>) -> Bind9Instance {
        Bind9Instance {
            metadata: ObjectMeta {
                name: Some("standalone".into()),
                namespace: Some("dns".into()),
                owner_references: owner.map(|name| {
                    vec![OwnerReference {
                        api_version: "bindy.firestoned.io/v1beta1".into(),
                        kind: "Bind9Cluster".into(),
                        name: name.into(),
                        uid: "uid".into(),
                        controller: Some(true),
                        ..Default::default()
                    }]
                }),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: cluster_ref.into(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: None,
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
                rndc_secret_ref: None,
                rndc_key: None,
                storage: None,
                placement: None,
                bindcar_config: None,
            },
            status: None,
        }
    }

    #[test]
    fn an_unowned_instance_wakes_the_cluster_it_references() {
        // Chaos suite: a Bind9Cluster counts every instance whose clusterRef
        // names it, but was woken only by instances it owns. A standalone
        // instance (clusterRef set, no ownerReference) turning Ready left the
        // cluster at "3/4 instances are ready" until some unrelated event.
        let refs = clusters_for_instance(&instance("my-dns", None));

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "my-dns");
        assert_eq!(refs[0].namespace.as_deref(), Some("dns"));
    }

    #[test]
    fn an_owned_instance_wakes_its_cluster_once() {
        let refs = clusters_for_instance(&instance("my-dns", Some("my-dns")));

        assert_eq!(
            refs.len(),
            1,
            "owner and clusterRef name the same cluster: {refs:?}"
        );
    }

    #[test]
    fn owner_and_reference_differing_wake_both() {
        let refs = clusters_for_instance(&instance("other", Some("my-dns")));

        let mut names: Vec<_> = refs.iter().map(|r| r.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["my-dns".to_string(), "other".to_string()]);
    }

    #[test]
    fn an_empty_cluster_ref_wakes_only_the_owner() {
        assert!(clusters_for_instance(&instance("", None)).is_empty());
    }

    #[test]
    fn an_instance_wakes_the_provider_it_references() {
        // The provider's status counts instances by clusterRef across
        // namespaces; it was woken only by the Bind9Clusters it owns.
        let refs = providers_for_instance(&instance("global-dns", None));

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "global-dns");
        assert_eq!(refs[0].namespace, None, "a provider is cluster-scoped");
    }

    #[test]
    fn an_empty_cluster_ref_wakes_no_provider() {
        assert!(providers_for_instance(&instance("", None)).is_empty());
    }
}
