// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! BIND9 Kubernetes resource builders
//!
//! This module provides functions to build Kubernetes resources (`Deployment`, `ConfigMap`, `Service`)
//! for BIND9 instances. All functions are pure and easily testable.
//!
//! - `named_conf` - `named.conf` and `named.conf.options` rendering, and
//!   the `dnssec-policy` statement
//! - `configmap` - the instance and cluster configuration `ConfigMap`s
//! - `dnssec` - the DNSSEC signing config, key volumes and key init container
//! - `meta` - labels, owner references, the config hash and child names
//! - `pod` - the `Deployment`, its pod spec, volumes and mounts
//! - `sidecar` - the bindcar sidecar container and its merged configuration
//! - `service` - the `Service`, `ServiceAccount` and `PodDisruptionBudget`
//!
//! Every public item is re-exported here, so callers keep the
//! `bind9_resources::<item>` path.

mod configmap;
mod dnssec;
mod meta;
mod named_conf;
mod pod;
mod service;
mod sidecar;

pub use configmap::{build_cluster_configmap, build_configmap};
pub use dnssec::{get_dnssec_signing_config, resolve_zone_dnssec_policy};
pub use meta::{
    build_cluster_labels, build_labels_from_instance, build_owner_references,
    build_pod_labels_from_instance, configmap_data_hash, controller_owner_reference,
    default_rndc_secret_name, instance_configmap_name, rndc_secret_name,
    rndc_secret_name_from_deployment, stamp_config_hash, CONFIG_HASH_ANNOTATION,
};
pub use pod::{build_deployment, zones_loaded_readiness_gates};
pub use service::{build_pod_disruption_budget, build_service, build_service_account};
pub use sidecar::resolve_bindcar_config;

#[cfg(test)]
pub(crate) use dnssec::{
    build_dnssec_key_volumes, build_dnssec_keys_init_container, DNSSEC_KEYS_INIT_SCRIPT,
};
#[cfg(test)]
pub(crate) use named_conf::dnssec_policy_statement;
#[cfg(test)]
pub(crate) use sidecar::{build_api_sidecar_container, BINDCAR_TLS_PATH, VOLUME_BINDCAR_TLS};

// BIND configuration file paths and mount points
const BIND_ZONES_PATH: &str = "/etc/bind/zones";
const BIND_CACHE_PATH: &str = "/var/cache/bind";
const BIND_KEYS_PATH: &str = "/etc/bind/keys";
const BIND_DNSSEC_KEYS_PATH: &str = "/var/cache/bind/keys";
const BIND_NAMED_CONF_PATH: &str = "/etc/bind/named.conf";
const BIND_NAMED_CONF_OPTIONS_PATH: &str = "/etc/bind/named.conf.options";
const BIND_NAMED_CONF_ZONES_PATH: &str = "/etc/bind/named.conf.zones";
const BIND_RNDC_CONF_PATH: &str = "/etc/bind/rndc.conf";

// BIND configuration file names
const NAMED_CONF_FILENAME: &str = "named.conf";
const NAMED_CONF_OPTIONS_FILENAME: &str = "named.conf.options";
const NAMED_CONF_ZONES_FILENAME: &str = "named.conf.zones";
const RNDC_CONF_FILENAME: &str = "rndc.conf";

// Volume mount names
const VOLUME_ZONES: &str = "zones";
const VOLUME_CACHE: &str = "cache";
const VOLUME_RNDC_KEY: &str = "rndc-key";
const VOLUME_CONFIG: &str = "config";
const VOLUME_NAMED_CONF: &str = "named-conf";
const VOLUME_NAMED_CONF_OPTIONS: &str = "named-conf-options";
const VOLUME_NAMED_CONF_ZONES: &str = "named-conf-zones";
const VOLUME_DNSSEC_KEYS: &str = "dnssec-keys";

/// `emptyDir` medium for a tmpfs volume.
const EMPTY_DIR_MEDIUM_MEMORY: &str = "Memory";

/// Memory-backed writable scratch volume for the bindcar sidecar (`TMPDIR`).
/// Required because the sidecar runs with `readOnlyRootFilesystem: true` under
/// Pod Security Admission `restricted` yet must write a `0600` TSIG key file
/// for `nsupdate -k`.
const VOLUME_TMP: &str = "tmp";
