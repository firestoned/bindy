// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The rendered BIND9 configuration `ConfigMap`s: one per standalone
//! instance, one shared by every instance of a cluster.

use super::meta::{
    build_cluster_labels, build_labels_from_instance, build_owner_references, CONFIGMAP_NAME_SUFFIX,
};
use super::named_conf::{build_named_conf, build_options_conf};
use super::{NAMED_CONF_FILENAME, NAMED_CONF_OPTIONS_FILENAME, RNDC_CONF_FILENAME};
use crate::crd::{Bind9Cluster, Bind9Instance};
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use std::collections::BTreeMap;
use tracing::debug;

// rndc.conf is the rndc client's file, not named.conf grammar, so it stays a
// template; named.conf and named.conf.options are written by hornet (ADR-0013).
const RNDC_CONF_TEMPLATE: &str = include_str!("../../../../templates/rndc.conf.tmpl");

/// Builds a Kubernetes `ConfigMap` containing BIND9 configuration files.
///
/// Creates a `ConfigMap` with the files NOT overridden by custom
/// `configMapRefs` (instance overrides cluster):
/// - `named.conf` - Main BIND9 configuration (omitted when `namedConf` ref is set)
/// - `named.conf.options` - BIND9 options (omitted when `namedConfOptions` ref is set)
/// - `rndc.conf` - RNDC client configuration (ALWAYS generated; it is not
///   overridable and is mounted from this `ConfigMap` unconditionally)
///
/// Because `rndc.conf` is always present, this `ConfigMap` must always be
/// created, even when both `namedConf` and `namedConfOptions` refs are set,
/// so the pod's `config` volume always has a backing `ConfigMap`.
///
/// # Arguments
///
/// * `name` - Name of the instance; the `ConfigMap` is `{name}-config`
/// * `namespace` - Kubernetes namespace
/// * `instance` - `Bind9Instance` spec containing configuration options
/// * `cluster` - Optional `Bind9Cluster` containing shared configuration
/// * `role_allow_transfer` - Role-specific allow-transfer override from cluster spec
///
/// # Returns
///
/// A Kubernetes `ConfigMap` resource ready for creation/update
///
/// # Errors
/// Returns an error if any ACL, forwarder, or listen-address entry in the
/// instance or cluster spec fails validation (see [`crate::bind9_acl`] for
/// the accepted ACL syntax), or [`crate::config_check::InvalidBind9Config`]
/// if the rendered configuration does not parse (ADR-0013), or an error when
/// the instance has no UID for the owner reference.
pub fn build_configmap(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    role_allow_transfer: Option<&Vec<String>>,
) -> anyhow::Result<ConfigMap> {
    debug!(
        name = %name,
        namespace = %namespace,
        "Building ConfigMap for Bind9Instance"
    );

    // Check if custom ConfigMaps are referenced (instance overrides cluster)
    let config_map_refs = instance
        .spec
        .config_map_refs
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.common.config_map_refs.as_ref()));

    let named_conf_overridden = config_map_refs.is_some_and(|refs| refs.named_conf.is_some());
    let options_overridden = config_map_refs.is_some_and(|refs| refs.named_conf_options.is_some());

    // named.conf, unless the user supplies it via the namedConf ref.
    let named_conf = (!named_conf_overridden).then(|| build_named_conf(config_map_refs));

    // named.conf.options, unless the user supplies it via the namedConfOptions
    // ref; validates ACL entries before rendering.
    let options_conf = if options_overridden {
        None
    } else {
        Some(build_options_conf(
            instance.spec.config.as_ref(),
            cluster.and_then(|c| c.spec.common.global.as_ref()),
            role_allow_transfer,
        )?)
    };

    let owner_refs = build_owner_references(instance)?;

    configuration_configmap(
        name,
        namespace,
        build_labels_from_instance(name, instance),
        Some(owner_refs),
        named_conf,
        options_conf,
    )
}

/// Builds a cluster-level shared `ConfigMap` containing BIND9 configuration files.
///
/// This `ConfigMap` is shared across all instances in a cluster, containing configuration
/// from `spec.global`. This eliminates the need for per-instance `ConfigMaps` when all
/// instances share the same configuration. It always carries `named.conf` and
/// `named.conf.options`: an instance's `configMapRefs` decide only which file
/// its pods mount.
///
/// # Arguments
///
/// * `cluster_name` - Name of the cluster; the `ConfigMap` is `{cluster_name}-config`
/// * `namespace` - Kubernetes namespace
/// * `cluster` - `Bind9Cluster` containing shared configuration
///
/// # Returns
///
/// A Kubernetes `ConfigMap` resource ready for creation/update
///
/// # Errors
///
/// Returns an error if configuration generation fails, or
/// [`crate::config_check::InvalidBind9Config`] if the rendered configuration
/// does not parse (ADR-0013).
pub fn build_cluster_configmap(
    cluster_name: &str,
    namespace: &str,
    cluster: &Bind9Cluster,
) -> Result<ConfigMap, anyhow::Error> {
    debug!(
        cluster_name = %cluster_name,
        namespace = %namespace,
        "Building cluster-level shared ConfigMap"
    );

    let named_conf = build_named_conf(cluster.spec.common.config_map_refs.as_ref());
    // No instance config and no role ACL: the cluster's `spec.global` alone.
    let options_conf = build_options_conf(None, cluster.spec.common.global.as_ref(), None)?;

    configuration_configmap(
        cluster_name,
        namespace,
        build_cluster_labels(cluster_name),
        None,
        Some(named_conf),
        Some(options_conf),
    )
}

/// The configuration `ConfigMap` both builders publish: the rendered files
/// given, plus the always-present `rndc.conf`, refused if `named` could not
/// parse them.
///
/// `named.conf.zones` is never generated: a user who wants zones there
/// provides a `namedConfZones` `ConfigMap`.
///
/// # Arguments
///
/// * `owner_name` - Instance or cluster name; the `ConfigMap` is `{owner_name}-config`
/// * `namespace` - Kubernetes namespace
/// * `labels` - Labels of the owning instance or cluster
/// * `owner_references` - The instance owner reference; `None` for a cluster
/// * `named_conf` - Rendered `named.conf`, `None` when a custom ref supplies it
/// * `options_conf` - Rendered `named.conf.options`, `None` when a custom ref supplies it
///
/// # Errors
///
/// Returns [`crate::config_check::InvalidBind9Config`] if a rendered file
/// does not parse (ADR-0013).
fn configuration_configmap(
    owner_name: &str,
    namespace: &str,
    labels: BTreeMap<String, String>,
    owner_references: Option<Vec<OwnerReference>>,
    named_conf: Option<String>,
    options_conf: Option<String>,
) -> anyhow::Result<ConfigMap> {
    let mut data = BTreeMap::new();
    if let Some(named_conf) = named_conf {
        data.insert(NAMED_CONF_FILENAME.into(), named_conf);
    }
    if let Some(options_conf) = options_conf {
        data.insert(NAMED_CONF_OPTIONS_FILENAME.into(), options_conf);
    }
    // rndc.conf references the key file mounted from the RNDC Secret. It is
    // never overridable, so the generated ConfigMap always exists.
    data.insert(RNDC_CONF_FILENAME.into(), RNDC_CONF_TEMPLATE.to_string());

    // ADR-0013: never publish a configuration `named` cannot parse. A broken
    // render fails here, so the ConfigMap is not written and the pods keep
    // the last configuration that was.
    for warning in crate::config_check::check_named_conf_files(&data)? {
        debug!("{warning}");
    }

    Ok(ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!("{owner_name}{CONFIGMAP_NAME_SUFFIX}")),
            namespace: Some(namespace.into()),
            labels: Some(labels),
            owner_references,
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    })
}
