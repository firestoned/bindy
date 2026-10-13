// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Labels, owner references, the config-hash annotation and the names of an
//! instance's child resources.

use super::VOLUME_RNDC_KEY;
use crate::crd::Bind9Instance;
use crate::labels::{
    APP_NAME_BIND9, COMPONENT_DNS_CLUSTER, COMPONENT_DNS_SERVER, K8S_COMPONENT, K8S_INSTANCE,
    K8S_MANAGED_BY, K8S_NAME, K8S_PART_OF, MANAGED_BY_BIND9_CLUSTER, MANAGED_BY_BIND9_INSTANCE,
    PART_OF_BINDY,
};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::ConfigMap};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::ResourceExt;
use std::collections::BTreeMap;

/// Suffix of the RNDC `Secret` bindy generates for an instance
/// ([`default_rndc_secret_name`]).
const RNDC_SECRET_NAME_SUFFIX: &str = "-rndc-key";
/// Suffix of a rendered BIND9 configuration `ConfigMap`
/// ([`instance_configmap_name`]).
pub(super) const CONFIGMAP_NAME_SUFFIX: &str = "-config";

/// Pod-template annotation holding [`configmap_data_hash`] of the BIND
/// configuration the pod mounts. BIND reads `named.conf` only at start, so a
/// changed ConfigMap reaches it only through a new pod; changing this
/// annotation is what rolls the Deployment. It covers changes from a spec
/// edit and from an operator upgrade that renders the same spec differently.
pub const CONFIG_HASH_ANNOTATION: &str = "bindy.firestoned.io/config-hash";

/// Lowercase hex sha256 over a ConfigMap's `data`, key and value pairs in key
/// order, each NUL-terminated so a byte moving from one file to the next is a
/// different config.
///
/// # Arguments
///
/// * `configmap` - The ConfigMap whose content is hashed
///
/// # Returns
///
/// The digest, 64 hex characters.
#[must_use]
pub fn configmap_data_hash(configmap: &ConfigMap) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for (key, value) in configmap.data.iter().flatten() {
        hasher.update(key.as_bytes());
        hasher.update([0u8]);
        hasher.update(value.as_bytes());
        hasher.update([0u8]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Set [`CONFIG_HASH_ANNOTATION`] on a Deployment's pod template.
///
/// # Arguments
///
/// * `deployment` - The Deployment to stamp
/// * `hash` - The hash of the ConfigMap its pods mount
pub fn stamp_config_hash(deployment: &mut Deployment, hash: &str) {
    let Some(spec) = deployment.spec.as_mut() else {
        return;
    };
    spec.template
        .metadata
        .get_or_insert_with(Default::default)
        .annotations
        .get_or_insert_with(Default::default)
        .insert(CONFIG_HASH_ANNOTATION.to_string(), hash.to_string());
}

/// Builds standardized Kubernetes labels for BIND9 cluster resources.
///
/// Creates labels for resources managed by `Bind9Cluster` controller.
/// Use [`build_labels_from_instance`] for resources managed by `Bind9Instance`.
///
/// # Arguments
///
/// * `cluster_name` - Name of the `Bind9Cluster` resource
///
/// # Returns
///
/// A `BTreeMap` of label key-value pairs
#[must_use]
pub fn build_cluster_labels(cluster_name: &str) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert("app".into(), APP_NAME_BIND9.into());
    labels.insert("cluster".into(), cluster_name.into());
    labels.insert(K8S_NAME.into(), APP_NAME_BIND9.into());
    labels.insert(K8S_INSTANCE.into(), cluster_name.into());
    labels.insert(K8S_COMPONENT.into(), COMPONENT_DNS_CLUSTER.into());
    labels.insert(K8S_MANAGED_BY.into(), MANAGED_BY_BIND9_CLUSTER.into());
    labels.insert(K8S_PART_OF.into(), PART_OF_BINDY.into());
    labels
}

/// Builds standardized Kubernetes labels for BIND9 instance resources,
/// propagating the `managed-by` label from the `Bind9Instance` if it exists.
///
/// This function checks if the instance has a `bindy.firestoned.io/managed-by` label.
/// If it does (indicating the instance is managed by a `Bind9Cluster`), that label
/// value is propagated to the `app.kubernetes.io/managed-by` label. Otherwise,
/// it defaults to `Bind9Instance`.
///
/// This ensures that when a `Bind9Cluster` creates a `Bind9Instance` with
/// `managed-by: Bind9Cluster`, all child resources (Deployments, Services) also
/// get `managed-by: Bind9Cluster`.
///
/// # Arguments
///
/// * `instance_name` - Name of the `Bind9Instance` resource
/// * `instance` - The `Bind9Instance` resource to check for management labels
///
/// # Returns
///
/// A `BTreeMap` of label key-value pairs
#[must_use]
pub fn build_labels_from_instance(
    instance_name: &str,
    instance: &Bind9Instance,
) -> BTreeMap<String, String> {
    use crate::labels::{BINDY_MANAGED_BY_LABEL, BINDY_ROLE_LABEL};

    let mut labels = BTreeMap::new();
    labels.insert("app".into(), APP_NAME_BIND9.into());
    labels.insert("instance".into(), instance_name.into());
    labels.insert(K8S_NAME.into(), APP_NAME_BIND9.into());
    labels.insert(K8S_INSTANCE.into(), instance_name.into());
    labels.insert(K8S_COMPONENT.into(), COMPONENT_DNS_SERVER.into());
    labels.insert(K8S_PART_OF.into(), PART_OF_BINDY.into());

    // Check if instance has bindy.firestoned.io/managed-by label
    // If it does, propagate it to app.kubernetes.io/managed-by
    let managed_by = instance
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(BINDY_MANAGED_BY_LABEL))
        .map_or(MANAGED_BY_BIND9_INSTANCE, String::as_str);

    labels.insert(K8S_MANAGED_BY.into(), managed_by.into());

    // Propagate bindy.firestoned.io/role label if it exists on the instance
    // This allows selecting pods by role (e.g., all primaries)
    if let Some(instance_labels) = &instance.metadata.labels {
        if let Some(role) = instance_labels.get(BINDY_ROLE_LABEL) {
            labels.insert(BINDY_ROLE_LABEL.into(), role.clone());
        }
    }

    labels
}

/// Builds the label set stamped onto the **Pods** of a `Bind9Instance`.
///
/// This is deliberately a superset of [`build_labels_from_instance`], which
/// remains the Deployment's `spec.selector` and the Service's selector.
///
/// # Why the two are separate
///
/// `spec.selector` on a Deployment is **immutable** — Kubernetes rejects any
/// change to it, because changing which Pods a Deployment claims would orphan
/// the ones it used to own. When one label map fed both the selector and the
/// Pod template (as it did before topology spreading landed), adding any new
/// label to Pods would have changed the selector too, wedging the reconciler
/// on every Deployment that already existed.
///
/// So: [`build_labels_from_instance`] is frozen and owns the selector, and
/// everything added afterwards goes here. Kubernetes only requires that the
/// selector *matches* the template labels, so the template may carry extras.
/// Service selectors are subset matches and are unaffected.
///
/// # Extra labels
///
/// * `bindy.firestoned.io/cluster` — the owning cluster, taken from
///   `spec.clusterRef`. Without it there is no label shared by the sibling
///   single-Pod Deployments of a cluster, and so no way to write a topology
///   spread selector that balances all primaries against each other.
/// * `bindy.firestoned.io/role` — derived from `spec.role` rather than from
///   the CR's metadata, so it is present even on a hand-written
///   `Bind9Instance` that carries no role label. Only inserted when the
///   selector does not already carry the key, so the Pod can never stop
///   matching its own Deployment's selector.
#[must_use]
pub fn build_pod_labels_from_instance(
    instance_name: &str,
    instance: &Bind9Instance,
) -> BTreeMap<String, String> {
    use crate::labels::{BINDY_CLUSTER_LABEL, BINDY_ROLE_LABEL};

    let mut labels = build_labels_from_instance(instance_name, instance);

    if !instance.spec.cluster_ref.is_empty() {
        labels.insert(
            BINDY_CLUSTER_LABEL.to_string(),
            instance.spec.cluster_ref.clone(),
        );
    }

    labels
        .entry(BINDY_ROLE_LABEL.to_string())
        .or_insert_with(|| instance.spec.role.as_str().to_string());

    labels
}

/// The controller owner reference every bindy child carries
/// (`controller: true`, `blockOwnerDeletion: true`; errors on a missing or
/// empty UID). Defined in the controller SDK and re-exported here so the
/// builders below and existing callers keep their path.
pub use bindy_controller_sdk::resources::controller_owner_reference;

/// Builds owner references for a resource owned by a `Bind9Instance`
///
/// Sets up cascade deletion so that when the `Bind9Instance` is deleted,
/// all its child resources (`Deployment`, `Service`, `ConfigMap`) are automatically deleted.
///
/// # Arguments
///
/// * `instance` - The `Bind9Instance` that owns this resource
///
/// # Returns
///
/// A vector containing a single `OwnerReference` pointing to the instance
///
/// # Errors
///
/// Returns an error when the instance has no UID
/// ([`controller_owner_reference`]).
pub fn build_owner_references(instance: &Bind9Instance) -> anyhow::Result<Vec<OwnerReference>> {
    Ok(vec![controller_owner_reference(instance)?])
}

/// The name of the RNDC `Secret` bindy creates for an instance when no
/// `rndcKey.secretRef` or inline `rndcKey.secret` names another one:
/// `{instance}-rndc-key`.
///
/// # Arguments
///
/// * `instance_name` - Name of the `Bind9Instance`
///
/// # Returns
///
/// The default RNDC `Secret` name.
#[must_use]
pub fn default_rndc_secret_name(instance_name: &str) -> String {
    format!("{instance_name}{RNDC_SECRET_NAME_SUFFIX}")
}

/// The RNDC `Secret` an instance uses, given its resolved RNDC configuration.
///
/// The single source of truth for the name: the instance reconciler creates
/// (or adopts) this Secret, the Deployment mounts it, and the drift check
/// looks for it.
///
/// * `rndcKey.secretRef` names an existing Secret: that one, never created
///   by bindy.
/// * An inline `rndcKey.secret` is created under its `metadata.name`.
/// * Otherwise bindy generates `{instance}-rndc-key`
///   ([`default_rndc_secret_name`]).
///
/// # Arguments
///
/// * `instance` - The `Bind9Instance`
/// * `rndc_config` - Its RNDC configuration, resolved through the instance >
///   role precedence
///
/// # Returns
///
/// The name of the `Secret` holding the instance's RNDC key.
#[must_use]
pub fn rndc_secret_name(
    instance: &Bind9Instance,
    rndc_config: &crate::crd::RndcKeyConfig,
) -> String {
    if let Some(secret_ref) = rndc_config.secret_ref.as_ref() {
        return secret_ref.name.clone();
    }
    if let Some(secret) = rndc_config.secret.as_ref() {
        return secret.metadata.name.clone();
    }
    default_rndc_secret_name(&instance.name_any())
}

/// The RNDC `Secret` a Deployment's pods mount, read from its `rndc-key`
/// volume.
///
/// This is the effective truth for the key BIND9 in those pods has loaded,
/// whichever configuration level chose it.
///
/// # Arguments
///
/// * `deployment` - A BIND9 Deployment built by [`build_deployment`](super::build_deployment)
///
/// # Returns
///
/// The mounted Secret's name, or `None` when the Deployment has no
/// Secret-backed `rndc-key` volume.
#[must_use]
pub fn rndc_secret_name_from_deployment(deployment: &Deployment) -> Option<String> {
    deployment
        .spec
        .as_ref()?
        .template
        .spec
        .as_ref()?
        .volumes
        .as_ref()?
        .iter()
        .find(|volume| volume.name == VOLUME_RNDC_KEY)?
        .secret
        .as_ref()?
        .secret_name
        .clone()
}

/// The `ConfigMap` an instance's pods mount for `named.conf` and friends.
///
/// An instance with a `clusterRef` (managed by its cluster or created by
/// hand) mounts the cluster's shared `{cluster}-config`; a standalone one
/// mounts its own `{instance}-config`. The Deployment builder, the instance
/// reconciler and its drift check all take the name from here.
///
/// # Arguments
///
/// * `instance_name` - Name of the `Bind9Instance`
/// * `instance` - The `Bind9Instance`
///
/// # Returns
///
/// The mounted `ConfigMap` name.
#[must_use]
pub fn instance_configmap_name(instance_name: &str, instance: &Bind9Instance) -> String {
    if instance.spec.cluster_ref.is_empty() {
        return format!("{instance_name}{CONFIGMAP_NAME_SUFFIX}");
    }
    format!("{}{CONFIGMAP_NAME_SUFFIX}", instance.spec.cluster_ref)
}
