// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Shared context for all operators with reflector stores.
//!
//! The context itself (`Context`, `Stores`, `RecordRef`, the record-kind
//! registry) lives in `bindy-controller-sdk` (ADR-0009, roadmap 01 Phase B
//! step B3) and is re-exported here under its old paths. What stays is the
//! BIND9-domain half: building a [`crate::bind9::Bind9Manager`] for an
//! instance and resolving its sidecar TLS, as the [`StoresBind9Ext`] trait.
//! It moves with the BIND9 code to `bindy-bind9` (Phase C).

pub use bindy_controller_sdk::context::{
    Context, Metrics, RecordKind, RecordKindOps, RecordRef, RecordStores, Stores, RECORD_KINDS,
};
pub use bindy_controller_sdk::watch::MultiStore;

use kube::ResourceExt;

/// BIND9-domain helpers over the shared [`Stores`].
pub trait StoresBind9Ext {
    /// Resolve the sidecar TLS configuration for an instance.
    ///
    /// Merges `bindcarConfig` across instance, cluster and provider using the
    /// same precedence as the reconciler, then returns its `tls` block.
    /// Returns `None` when TLS is not configured, which is the default.
    fn resolve_bindcar_tls(
        &self,
        instance_name: &str,
        instance_namespace: &str,
    ) -> Option<crate::crd::BindcarTlsConfig>;

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
    /// # use bindy::context::{Stores, StoresBind9Ext};
    /// # fn example(stores: &Stores) {
    /// let manager = stores.create_bind9_manager_for_instance(
    ///     "my-instance",
    ///     "bindy-system"
    /// );
    /// # }
    /// ```
    fn create_bind9_manager_for_instance(
        &self,
        instance_name: &str,
        instance_namespace: &str,
    ) -> crate::bind9::Bind9Manager;

    /// As [`Self::create_bind9_manager_for_instance`], but supplying the
    /// Kubernetes client needed to read a TLS CA bundle.
    ///
    /// Callers that may talk to a TLS-enabled sidecar must use this form:
    /// without a client the manager cannot read the configured CA bundle and
    /// will refuse to connect rather than fall back to plaintext.
    fn create_bind9_manager_for_instance_with_client(
        &self,
        instance_name: &str,
        instance_namespace: &str,
        kube_client: Option<kube::Client>,
    ) -> crate::bind9::Bind9Manager;
}

impl StoresBind9Ext for Stores {
    fn resolve_bindcar_tls(
        &self,
        instance_name: &str,
        instance_namespace: &str,
    ) -> Option<crate::crd::BindcarTlsConfig> {
        let instance = self.get_bind9instance(instance_name, instance_namespace)?;

        // Resolve the owning cluster from the store. `cluster_ref` is the
        // declared link; ownerReferences are the authoritative one for
        // cluster-generated instances, so try both (mirrors fetch_cluster_info,
        // without the API round trip).
        let cluster = self
            .bind9_clusters
            .state()
            .iter()
            .find(|c| {
                c.namespace().as_deref() == Some(instance_namespace)
                    && (c.name_any() == instance.spec.cluster_ref
                        || instance
                            .metadata
                            .owner_references
                            .as_ref()
                            .is_some_and(|refs| {
                                refs.iter()
                                    .any(|r| r.kind == "Bind9Cluster" && r.name == c.name_any())
                            }))
            })
            .cloned();

        let provider = cluster.as_ref().and_then(|c| {
            let owners = c.metadata.owner_references.as_ref()?;
            self.cluster_bind9_providers
                .state()
                .iter()
                .find(|p| {
                    owners
                        .iter()
                        .any(|r| r.kind == "ClusterBind9Provider" && r.name == p.name_any())
                })
                .cloned()
        });

        crate::bind9_resources::resolve_bindcar_config(
            &instance,
            cluster.as_deref(),
            provider.as_deref(),
        )
        .and_then(|c| c.tls)
    }

    fn create_bind9_manager_for_instance(
        &self,
        instance_name: &str,
        instance_namespace: &str,
    ) -> crate::bind9::Bind9Manager {
        self.create_bind9_manager_for_instance_with_client(instance_name, instance_namespace, None)
    }

    fn create_bind9_manager_for_instance_with_client(
        &self,
        instance_name: &str,
        instance_namespace: &str,
        kube_client: Option<kube::Client>,
    ) -> crate::bind9::Bind9Manager {
        let tls = self.resolve_bindcar_tls(instance_name, instance_namespace);
        let apply = |m: crate::bind9::Bind9Manager| {
            let m = m.with_tls(tls.clone());
            match kube_client.clone() {
                Some(c) => m.with_kube_client(c),
                None => m,
            }
        };

        // Try to get the deployment for this instance
        if let Some(deployment) = self.get_deployment(instance_name, instance_namespace) {
            // Found deployment - create manager with auth detection
            apply(crate::bind9::Bind9Manager::new_with_deployment(
                deployment,
                instance_name.to_string(),
                instance_namespace.to_string(),
            ))
        } else {
            // No deployment found - fall back to basic manager (auth assumed enabled)
            tracing::debug!(
                instance = instance_name,
                namespace = instance_namespace,
                "Deployment not found in store, using basic Bind9Manager (auth enabled)"
            );
            apply(crate::bind9::Bind9Manager::new())
        }
    }
}
