// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 Kubernetes resource builders
//!
//! This module provides functions to build Kubernetes resources (`Deployment`, `ConfigMap`, `Service`)
//! for BIND9 instances. All functions are pure and easily testable.

use crate::constants::{
    API_GROUP_VERSION, BIND9_MALLOC_CONF, BIND9_NONROOT_UID, BIND9_SERVICE_ACCOUNT,
    CONTAINER_NAME_BIND9, CONTAINER_NAME_BINDCAR, DEFAULT_BIND9_VERSION, DNS_CONTAINER_PORT,
    DNS_PORT, KIND_BIND9_INSTANCE, LIVENESS_FAILURE_THRESHOLD, LIVENESS_INITIAL_DELAY_SECS,
    LIVENESS_PERIOD_SECS, LIVENESS_TIMEOUT_SECS, READINESS_FAILURE_THRESHOLD,
    READINESS_INITIAL_DELAY_SECS, READINESS_PERIOD_SECS, READINESS_TIMEOUT_SECS, RNDC_PORT,
};
use crate::crd::{Bind9Cluster, Bind9Instance, ConfigMapRefs, ImageConfig};
use crate::labels::{
    APP_NAME_BIND9, COMPONENT_DNS_CLUSTER, COMPONENT_DNS_SERVER, K8S_COMPONENT, K8S_INSTANCE,
    K8S_MANAGED_BY, K8S_NAME, K8S_PART_OF, MANAGED_BY_BIND9_CLUSTER, MANAGED_BY_BIND9_INSTANCE,
    PART_OF_BINDY,
};
use k8s_openapi::api::{
    apps::v1::{Deployment, DeploymentSpec},
    core::v1::{
        Capabilities, ConfigMap, Container, ContainerPort, EnvVar, EnvVarSource,
        PodSecurityContext, PodSpec, PodTemplateSpec, Probe, SecretKeySelector, SecurityContext,
        Service, ServiceAccount, ServicePort, ServiceSpec, TCPSocketAction, Volume, VolumeMount,
    },
};
use k8s_openapi::apimachinery::pkg::{
    apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference},
    util::intstr::IntOrString,
};
use kube::ResourceExt;
use std::collections::BTreeMap;
use tracing::debug;

// Embed configuration templates at compile time
const NAMED_CONF_TEMPLATE: &str = include_str!("../templates/named.conf.tmpl");
const NAMED_CONF_OPTIONS_TEMPLATE: &str = include_str!("../templates/named.conf.options.tmpl");
const RNDC_CONF_TEMPLATE: &str = include_str!("../templates/rndc.conf.tmpl");

// BIND configuration file paths and mount points
const BIND_ZONES_PATH: &str = "/etc/bind/zones";
const BIND_CACHE_PATH: &str = "/var/cache/bind";
const BIND_KEYS_PATH: &str = "/etc/bind/keys";
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

/// Builds standardized Kubernetes labels for BIND9 instance resources.
///
/// Creates labels for resources managed by `Bind9Instance` controller.
/// Use `build_cluster_labels()` for resources managed by `Bind9Cluster`.
///
/// # Arguments
///
/// * `instance_name` - Name of the `Bind9Instance` resource
///
/// # Returns
///
/// A `BTreeMap` of label key-value pairs
///
/// Builds standardized Kubernetes labels for BIND9 cluster resources.
///
/// Creates labels for resources managed by `Bind9Cluster` controller.
/// Use `build_labels()` for resources managed by `Bind9Instance`.
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
#[must_use]
pub fn build_owner_references(instance: &Bind9Instance) -> Vec<OwnerReference> {
    vec![OwnerReference {
        api_version: API_GROUP_VERSION.to_string(),
        kind: KIND_BIND9_INSTANCE.to_string(),
        name: instance.name_any(),
        uid: instance.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }]
}

/// Builds a Kubernetes `ConfigMap` containing BIND9 configuration files.
///
/// Creates a `ConfigMap` with:
/// - `named.conf` - Main BIND9 configuration
/// - `named.conf.options` - BIND9 options (recursion, ACLs, DNSSEC, etc.)
///
/// If custom `ConfigMaps` are referenced in the cluster or instance spec, this function
/// will not generate configuration files, as they should be provided by the user.
///
/// # Arguments
///
/// * `name` - Name for the `ConfigMap` (typically `{instance-name}-config`)
/// * `namespace` - Kubernetes namespace
/// * `instance` - `Bind9Instance` spec containing configuration options
/// * `cluster` - Optional `Bind9Cluster` containing shared configuration
///
/// # Returns
///
/// A Kubernetes `ConfigMap` resource ready for creation/update, or None if custom `ConfigMaps` are used
#[must_use]
pub fn build_configmap(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    role_allow_transfer: Option<&Vec<String>>,
) -> Option<ConfigMap> {
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

    // If custom ConfigMaps are specified, don't generate a ConfigMap
    if let Some(refs) = config_map_refs {
        if refs.named_conf.is_some() || refs.named_conf_options.is_some() {
            debug!(
                named_conf_ref = ?refs.named_conf,
                named_conf_options_ref = ?refs.named_conf_options,
                "Custom ConfigMaps specified, skipping generation"
            );
            // User is providing custom ConfigMaps, so we don't create one
            return None;
        }
    }

    // Generate default configuration
    let mut data = BTreeMap::new();
    let labels = build_labels_from_instance(name, instance);

    // Build named.conf
    let named_conf = build_named_conf(instance, cluster);
    data.insert(NAMED_CONF_FILENAME.into(), named_conf);

    // Build named.conf.options
    let options_conf = build_options_conf(instance, cluster, role_allow_transfer);
    data.insert(NAMED_CONF_OPTIONS_FILENAME.into(), options_conf);

    // Build rndc.conf (references key file mounted from Secret)
    data.insert(RNDC_CONF_FILENAME.into(), RNDC_CONF_TEMPLATE.to_string());

    // Note: We do NOT auto-generate named.conf.zones anymore.
    // Users must explicitly provide a namedConfZones ConfigMap if they want zones support.

    let owner_refs = build_owner_references(instance);

    Some(ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!("{name}-config")),
            namespace: Some(namespace.into()),
            labels: Some(labels),
            owner_references: Some(owner_refs),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    })
}

/// Builds a cluster-level shared `ConfigMap` containing BIND9 configuration files.
///
/// This `ConfigMap` is shared across all instances in a cluster, containing configuration
/// from `spec.global`. This eliminates the need for per-instance `ConfigMaps` when all
/// instances share the same configuration.
///
/// # Arguments
///
/// * `cluster_name` - Name of the cluster (used for `ConfigMap` naming)
/// * `namespace` - Kubernetes namespace
/// * `cluster` - `Bind9Cluster` containing shared configuration
///
/// # Returns
///
/// A Kubernetes `ConfigMap` resource ready for creation/update
///
/// # Errors
///
/// Returns an error if configuration generation fails
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

    // Generate default configuration from cluster spec
    let mut data = BTreeMap::new();
    let labels = build_cluster_labels(cluster_name);

    // Build named.conf from cluster
    let named_conf = build_cluster_named_conf(cluster);
    data.insert(NAMED_CONF_FILENAME.into(), named_conf);

    // Build named.conf.options from cluster.spec.common.global
    let options_conf = build_cluster_options_conf(cluster);
    data.insert(NAMED_CONF_OPTIONS_FILENAME.into(), options_conf);

    // Build rndc.conf (references key file mounted from Secret)
    data.insert(RNDC_CONF_FILENAME.into(), RNDC_CONF_TEMPLATE.to_string());

    Ok(ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!("{cluster_name}-config")),
            namespace: Some(namespace.into()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    })
}

/// Build the main named.conf configuration from template
///
/// Generates the main BIND9 configuration file with conditional zones include.
/// The zones include directive is only added if the user provides a `namedConfZones` `ConfigMap`.
///
/// # Arguments
///
/// * `instance` - `Bind9Instance` spec (checked first for config refs)
/// * `cluster` - Optional `Bind9Cluster` (fallback for config refs)
///
/// # Returns
///
/// A string containing the complete named.conf configuration
fn build_named_conf(instance: &Bind9Instance, cluster: Option<&Bind9Cluster>) -> String {
    // Check if user provided a custom zones ConfigMap
    let config_map_refs = instance
        .spec
        .config_map_refs
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.common.config_map_refs.as_ref()));

    let zones_include = if let Some(refs) = config_map_refs {
        if refs.named_conf_zones.is_some() {
            // User provided custom zones file, include it from custom ConfigMap location
            "\n// Include zones file from user-provided ConfigMap\ninclude \"/etc/bind/named.conf.zones\";\n".to_string()
        } else {
            // No zones ConfigMap provided, don't include zones file
            String::new()
        }
    } else {
        // No config refs at all, don't include zones file
        String::new()
    };

    // Build RNDC key includes and key names for controls block
    // For now, we support a single key per instance (bindy-operator)
    // Future enhancement: support multiple keys from spec
    let rndc_key_includes = "include \"/etc/bind/keys/rndc.key\";";
    let rndc_key_names = "\"bindy-operator\"";

    NAMED_CONF_TEMPLATE
        .replace("{{ZONES_INCLUDE}}", &zones_include)
        .replace("{{RNDC_KEY_INCLUDES}}", rndc_key_includes)
        .replace("{{RNDC_KEY_NAMES}}", rndc_key_names)
}

/// Build the named.conf.options configuration from template
///
/// Generates the BIND9 options configuration file from the instance's config spec.
/// Includes settings for recursion, ACLs (allow-query, allow-transfer), and DNSSEC.
///
/// Priority for configuration values (highest to lowest):
/// 1. Instance-level settings (`instance.spec.config`)
/// 2. Role-specific settings (`role_allow_transfer` from cluster primary/secondary spec)
/// 3. Global cluster settings (`cluster.spec.common.global`)
/// 4. Defaults (BIND9 defaults or no setting)
///
/// # Arguments
///
/// * `instance` - `Bind9Instance` spec containing the BIND9 configuration
/// * `cluster` - Optional `Bind9Cluster` containing global configuration
/// * `role_allow_transfer` - Role-specific allow-transfer override from cluster spec (primary/secondary)
///
/// # Returns
///
/// A string containing the complete named.conf.options configuration
#[allow(clippy::too_many_lines)]
fn build_options_conf(
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    role_allow_transfer: Option<&Vec<String>>,
) -> String {
    let recursion;
    let mut allow_query = String::new();
    let allow_transfer;
    let mut dnssec_validate = String::new();

    // Get global config from cluster if available
    let global_config = cluster.and_then(|c| c.spec.common.global.as_ref());

    if let Some(config) = &instance.spec.config {
        // Recursion setting - instance overrides global
        let recursion_value = if let Some(rec) = config.recursion {
            if rec {
                "yes"
            } else {
                "no"
            }
        } else if let Some(global) = global_config {
            if global.recursion.unwrap_or(false) {
                "yes"
            } else {
                "no"
            }
        } else {
            "no"
        };
        recursion = format!("recursion {recursion_value};");

        // Allow-query ACL - instance overrides global
        if let Some(acls) = &config.allow_query {
            if !acls.is_empty() {
                let acl_list = acls.join("; ");
                allow_query = format!("allow-query {{ {acl_list}; }};");
            }
        } else if let Some(global) = global_config {
            if let Some(global_acls) = &global.allow_query {
                if !global_acls.is_empty() {
                    let acl_list = global_acls.join("; ");
                    allow_query = format!("allow-query {{ {acl_list}; }};");
                }
            }
        }

        // Allow-transfer ACL - priority: instance config > role-specific > global > no default
        if let Some(acls) = &config.allow_transfer {
            // Instance-level config takes highest priority
            let acl_list = if acls.is_empty() {
                "none".to_string()
            } else {
                acls.join("; ")
            };
            allow_transfer = format!("allow-transfer {{ {acl_list}; }};");
        } else if let Some(role_acls) = role_allow_transfer {
            // Role-specific override from cluster config (primary/secondary)
            let acl_list = if role_acls.is_empty() {
                "none".to_string()
            } else {
                role_acls.join("; ")
            };
            allow_transfer = format!("allow-transfer {{ {acl_list}; }};");
        } else if let Some(global) = global_config {
            // Global cluster settings
            if let Some(global_acls) = &global.allow_transfer {
                let acl_list = if global_acls.is_empty() {
                    "none".to_string()
                } else {
                    global_acls.join("; ")
                };
                allow_transfer = format!("allow-transfer {{ {acl_list}; }};");
            } else {
                allow_transfer = String::new();
            }
        } else {
            // No default - let BIND9 use its own defaults (none)
            allow_transfer = String::new();
        }

        // DNSSEC configuration - instance overrides global
        // Note: dnssec-enable was removed in BIND 9.15+ (DNSSEC is always enabled)
        // Only dnssec-validation is configurable now
        if let Some(dnssec) = &config.dnssec {
            if dnssec.validation.unwrap_or(false) {
                dnssec_validate = "dnssec-validation yes;".to_string();
            } else {
                dnssec_validate = "dnssec-validation no;".to_string();
            }
        } else if let Some(global) = global_config {
            if let Some(global_dnssec) = &global.dnssec {
                if global_dnssec.validation.unwrap_or(false) {
                    dnssec_validate = "dnssec-validation yes;".to_string();
                } else {
                    dnssec_validate = "dnssec-validation no;".to_string();
                }
            }
        }
    } else {
        // No instance config - use global config if available, otherwise defaults
        if let Some(global) = global_config {
            // Recursion from global
            let recursion_value = if global.recursion.unwrap_or(false) {
                "yes"
            } else {
                "no"
            };
            recursion = format!("recursion {recursion_value};");

            // Allow-query from global
            if let Some(acls) = &global.allow_query {
                if !acls.is_empty() {
                    let acl_list = acls.join("; ");
                    allow_query = format!("allow-query {{ {acl_list}; }};");
                }
            }

            // Allow-transfer - priority: role-specific > global > no default
            if let Some(role_acls) = role_allow_transfer {
                let acl_list = if role_acls.is_empty() {
                    "none".to_string()
                } else {
                    role_acls.join("; ")
                };
                allow_transfer = format!("allow-transfer {{ {acl_list}; }};");
            } else if let Some(global_acls) = &global.allow_transfer {
                let acl_list = if global_acls.is_empty() {
                    "none".to_string()
                } else {
                    global_acls.join("; ")
                };
                allow_transfer = format!("allow-transfer {{ {acl_list}; }};");
            } else {
                allow_transfer = String::new();
            }

            // DNSSEC from global
            if let Some(dnssec) = &global.dnssec {
                if dnssec.validation.unwrap_or(false) {
                    dnssec_validate = "dnssec-validation yes;".to_string();
                }
            }
        } else {
            // Defaults when no config is specified
            recursion = "recursion no;".to_string();
            // No default for allow-transfer - let BIND9 use its own defaults (none)
            allow_transfer = String::new();
        }
    }

    // Perform template substitutions
    NAMED_CONF_OPTIONS_TEMPLATE
        .replace("{{RECURSION}}", &recursion)
        .replace("{{ALLOW_QUERY}}", &allow_query)
        .replace("{{ALLOW_TRANSFER}}", &allow_transfer)
        .replace("{{DNSSEC_VALIDATE}}", &dnssec_validate)
}

/// Build the main named.conf configuration for a cluster from template
///
/// Generates the main BIND9 configuration file with conditional zones include.
/// The zones include directive is only added if the user provides a `namedConfZones` `ConfigMap`.
///
/// # Arguments
///
/// * `cluster` - `Bind9Cluster` spec (checked for config refs)
///
/// # Returns
///
/// A string containing the complete named.conf configuration
fn build_cluster_named_conf(cluster: &Bind9Cluster) -> String {
    // Check if user provided a custom zones ConfigMap
    let zones_include = if let Some(refs) = &cluster.spec.common.config_map_refs {
        if refs.named_conf_zones.is_some() {
            // User provided custom zones file, include it from custom ConfigMap location
            "\n// Include zones file from user-provided ConfigMap\ninclude \"/etc/bind/named.conf.zones\";\n".to_string()
        } else {
            // No zones ConfigMap provided, don't include zones file
            String::new()
        }
    } else {
        // No config refs at all, don't include zones file
        String::new()
    };

    // Build RNDC key includes and key names for controls block
    // For now, we support a single key per instance (bindy-operator)
    // Future enhancement: support multiple keys from spec
    let rndc_key_includes = "include \"/etc/bind/keys/rndc.key\";";
    let rndc_key_names = "\"bindy-operator\"";

    NAMED_CONF_TEMPLATE
        .replace("{{ZONES_INCLUDE}}", &zones_include)
        .replace("{{RNDC_KEY_INCLUDES}}", rndc_key_includes)
        .replace("{{RNDC_KEY_NAMES}}", rndc_key_names)
}

/// Build the named.conf.options configuration for a cluster from template
///
/// Generates the BIND9 options configuration file from the cluster's `spec.global` config.
/// Includes settings for recursion, ACLs (allow-query, allow-transfer), and DNSSEC.
///
/// # Arguments
///
/// * `cluster` - `Bind9Cluster` containing global configuration
///
/// # Returns
///
/// A string containing the complete named.conf.options configuration
#[allow(clippy::too_many_lines)]
fn build_cluster_options_conf(cluster: &Bind9Cluster) -> String {
    let recursion;
    let mut allow_query = String::new();
    let mut allow_transfer = String::new();
    let mut dnssec_validate = String::new();

    // Use cluster global config
    if let Some(global) = &cluster.spec.common.global {
        // Recursion setting
        let recursion_value = if global.recursion.unwrap_or(false) {
            "yes"
        } else {
            "no"
        };
        recursion = format!("recursion {recursion_value};");

        // allow-query ACL
        if let Some(aq) = &global.allow_query {
            if !aq.is_empty() {
                allow_query = format!(
                    "allow-query {{ {}; }};",
                    aq.iter().map(String::as_str).collect::<Vec<_>>().join("; ")
                );
            }
        }

        // allow-transfer ACL
        if let Some(at) = &global.allow_transfer {
            if !at.is_empty() {
                allow_transfer = format!(
                    "allow-transfer {{ {}; }};",
                    at.iter().map(String::as_str).collect::<Vec<_>>().join("; ")
                );
            }
        }

        // DNSSEC validation
        if let Some(dnssec) = &global.dnssec {
            if dnssec.validation.unwrap_or(false) {
                dnssec_validate = "dnssec-validation yes;".to_string();
            } else {
                dnssec_validate = "dnssec-validation no;".to_string();
            }
        }
    } else {
        // No global config, use defaults
        recursion = "recursion no;".to_string();
    }

    NAMED_CONF_OPTIONS_TEMPLATE
        .replace("{{RECURSION}}", &recursion)
        .replace("{{ALLOW_QUERY}}", &allow_query)
        .replace("{{ALLOW_TRANSFER}}", &allow_transfer)
        .replace("{{DNSSEC_VALIDATE}}", &dnssec_validate)
}

/// Builds a Kubernetes Deployment for running BIND9 pods.
///
/// Creates a Deployment with:
/// - BIND9 container using configured or default image
/// - `ConfigMap` volume mounts for configuration
/// - `EmptyDir` volumes for zones and cache
/// - TCP/UDP port 53 exposed
/// - Liveness and readiness probes
///
/// # Arguments
///
/// * `name` - Name for the Deployment
/// * `namespace` - Kubernetes namespace
/// * `instance` - `Bind9Instance` spec containing replicas, version, etc.
/// * `cluster` - Optional `Bind9Cluster` containing shared configuration
///
/// # Returns
///
/// A Kubernetes Deployment resource ready for creation/update
#[must_use]
/// Helper struct to hold resolved configuration for a `Bind9Instance` deployment
struct DeploymentConfig<'a> {
    image_config: Option<&'a ImageConfig>,
    config_map_refs: Option<&'a ConfigMapRefs>,
    version: &'a str,
    volumes: Option<&'a Vec<Volume>>,
    volume_mounts: Option<&'a Vec<VolumeMount>>,
    bindcar_config: Option<&'a crate::crd::BindcarConfig>,
    configmap_name: String,
    rndc_secret_name: String,
}

/// Extract and resolve deployment configuration from instance and cluster
fn resolve_deployment_config<'a>(
    name: &str,
    instance: &'a Bind9Instance,
    cluster: Option<&'a Bind9Cluster>,
    cluster_provider: Option<&'a crate::crd::ClusterBind9Provider>,
) -> DeploymentConfig<'a> {
    // Get image config (instance overrides cluster overrides cluster provider)
    let image_config = instance
        .spec
        .image
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.common.image.as_ref()))
        .or_else(|| cluster_provider.and_then(|cp| cp.spec.common.image.as_ref()));

    // Get ConfigMap references (instance overrides cluster overrides cluster provider)
    let config_map_refs = instance
        .spec
        .config_map_refs
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.common.config_map_refs.as_ref()))
        .or_else(|| cluster_provider.and_then(|cp| cp.spec.common.config_map_refs.as_ref()));

    // Get version (instance overrides cluster overrides cluster provider)
    let version = instance
        .spec
        .version
        .as_deref()
        .or_else(|| cluster.and_then(|c| c.spec.common.version.as_deref()))
        .or_else(|| cluster_provider.and_then(|cp| cp.spec.common.version.as_deref()))
        .unwrap_or(DEFAULT_BIND9_VERSION);

    // Get volumes (instance overrides cluster overrides cluster provider)
    let volumes = instance
        .spec
        .volumes
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.common.volumes.as_ref()))
        .or_else(|| cluster_provider.and_then(|cp| cp.spec.common.volumes.as_ref()));

    // Get volume mounts (instance overrides cluster overrides cluster provider)
    let volume_mounts = instance
        .spec
        .volume_mounts
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.common.volume_mounts.as_ref()))
        .or_else(|| cluster_provider.and_then(|cp| cp.spec.common.volume_mounts.as_ref()));

    // Get bindcar_config (instance overrides cluster global overrides cluster provider global)
    let bindcar_config = instance
        .spec
        .bindcar_config
        .as_ref()
        .or_else(|| {
            cluster.and_then(|c| {
                c.spec
                    .common
                    .global
                    .as_ref()
                    .and_then(|g| g.bindcar_config.as_ref())
            })
        })
        .or_else(|| {
            cluster_provider.and_then(|cp| {
                cp.spec
                    .common
                    .global
                    .as_ref()
                    .and_then(|g| g.bindcar_config.as_ref())
            })
        });

    // Determine ConfigMap name: use cluster ConfigMap if instance belongs to a cluster
    let configmap_name = if instance.spec.cluster_ref.is_empty() {
        // Use instance-specific ConfigMap
        format!("{name}-config")
    } else {
        // Use cluster-level shared ConfigMap
        format!("{}-config", instance.spec.cluster_ref)
    };

    // Determine RNDC secret name
    // TODO: Use actual RNDC config precedence resolution when implemented
    let rndc_secret_name = format!("{name}-rndc-key");

    DeploymentConfig {
        image_config,
        config_map_refs,
        version,
        volumes,
        volume_mounts,
        bindcar_config,
        configmap_name,
        rndc_secret_name,
    }
}

pub fn build_deployment(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Deployment {
    debug!(
        name = %name,
        namespace = %namespace,
        has_cluster = cluster.is_some(),
        has_cluster_provider = cluster_provider.is_some(),
        "Building Deployment for Bind9Instance"
    );

    // Build labels, checking if instance is managed by a cluster
    let labels = build_labels_from_instance(name, instance);
    let replicas = instance.spec.replicas.unwrap_or(1);
    debug!(replicas, "Deployment replica count");

    let config = resolve_deployment_config(name, instance, cluster, cluster_provider);

    let owner_refs = build_owner_references(instance);

    Deployment {
        metadata: ObjectMeta {
            name: Some(name.into()),
            namespace: Some(namespace.into()),
            labels: Some(labels.clone()),
            owner_references: Some(owner_refs),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(replicas),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels.clone()),
                    ..Default::default()
                }),
                spec: Some(build_pod_spec(
                    namespace,
                    &config.configmap_name,
                    &config.rndc_secret_name,
                    config.version,
                    config.image_config,
                    config.config_map_refs,
                    config.volumes,
                    config.volume_mounts,
                    config.bindcar_config,
                )),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Builds pod specification with BIND9 container and API sidecar
///
/// # Arguments
/// * `namespace` - Namespace where the pod will be deployed
/// * `configmap_name` - Name of the `ConfigMap` with BIND9 configuration
/// * `rndc_secret_name` - Name of the Secret with RNDC keys
/// * `version` - BIND9 version tag
/// * `image_config` - Optional custom image configuration
/// * `config_map_refs` - Optional custom `ConfigMap` references
/// * `custom_volumes` - Optional custom volumes to add
/// * `custom_volume_mounts` - Optional custom volume mounts to add
/// * `bindcar_config` - Optional API sidecar configuration
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn build_pod_spec(
    namespace: &str,
    configmap_name: &str,
    rndc_secret_name: &str,
    version: &str,
    image_config: Option<&ImageConfig>,
    config_map_refs: Option<&ConfigMapRefs>,
    custom_volumes: Option<&Vec<Volume>>,
    custom_volume_mounts: Option<&Vec<VolumeMount>>,
    bindcar_config: Option<&crate::crd::BindcarConfig>,
) -> PodSpec {
    // Determine image to use
    let image = if let Some(img_cfg) = image_config {
        img_cfg
            .image
            .clone()
            .unwrap_or_else(|| format!("internetsystemsconsortium/bind9:{version}"))
    } else {
        format!("internetsystemsconsortium/bind9:{version}")
    };

    // Determine image pull policy
    let image_pull_policy = image_config
        .and_then(|cfg| cfg.image_pull_policy.clone())
        .unwrap_or_else(|| "IfNotPresent".into());

    // BIND9 container
    let bind9_container = Container {
        name: CONTAINER_NAME_BIND9.into(),
        image: Some(image),
        image_pull_policy: Some(image_pull_policy),
        command: Some(vec!["named".into()]),
        args: Some(vec![
            "-c".into(),
            BIND_NAMED_CONF_PATH.into(),
            "-g".into(), // Run in foreground (required for containers)
        ]),
        ports: Some(vec![
            ContainerPort {
                name: Some("dns-tcp".into()),
                container_port: i32::from(DNS_CONTAINER_PORT),
                protocol: Some("TCP".into()),
                ..Default::default()
            },
            ContainerPort {
                name: Some("dns-udp".into()),
                container_port: i32::from(DNS_CONTAINER_PORT),
                protocol: Some("UDP".into()),
                ..Default::default()
            },
            ContainerPort {
                name: Some("rndc".into()),
                container_port: i32::from(RNDC_PORT),
                protocol: Some("TCP".into()),
                ..Default::default()
            },
        ]),
        env: Some(vec![
            EnvVar {
                name: "TZ".into(),
                value: Some("UTC".into()),
                ..Default::default()
            },
            EnvVar {
                name: "MALLOC_CONF".into(),
                value: Some(BIND9_MALLOC_CONF.into()),
                ..Default::default()
            },
        ]),
        volume_mounts: Some(build_volume_mounts(config_map_refs, custom_volume_mounts)),
        liveness_probe: Some(Probe {
            tcp_socket: Some(TCPSocketAction {
                port: IntOrString::Int(i32::from(DNS_CONTAINER_PORT)),
                ..Default::default()
            }),
            initial_delay_seconds: Some(LIVENESS_INITIAL_DELAY_SECS),
            period_seconds: Some(LIVENESS_PERIOD_SECS),
            timeout_seconds: Some(LIVENESS_TIMEOUT_SECS),
            failure_threshold: Some(LIVENESS_FAILURE_THRESHOLD),
            ..Default::default()
        }),
        readiness_probe: Some(Probe {
            tcp_socket: Some(TCPSocketAction {
                port: IntOrString::Int(i32::from(DNS_CONTAINER_PORT)),
                ..Default::default()
            }),
            initial_delay_seconds: Some(READINESS_INITIAL_DELAY_SECS),
            period_seconds: Some(READINESS_PERIOD_SECS),
            timeout_seconds: Some(READINESS_TIMEOUT_SECS),
            failure_threshold: Some(READINESS_FAILURE_THRESHOLD),
            ..Default::default()
        }),
        security_context: Some(SecurityContext {
            run_as_non_root: Some(true),
            run_as_user: Some(BIND9_NONROOT_UID),
            run_as_group: Some(BIND9_NONROOT_UID),
            allow_privilege_escalation: Some(false),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    // Build image pull secrets if specified
    let image_pull_secrets = image_config.and_then(|cfg| {
        cfg.image_pull_secrets.as_ref().map(|secrets| {
            secrets
                .iter()
                .map(|s| k8s_openapi::api::core::v1::LocalObjectReference { name: s.clone() })
                .collect()
        })
    });

    PodSpec {
        containers: {
            let mut containers = vec![bind9_container];
            containers.push(build_api_sidecar_container(
                namespace,
                bindcar_config,
                rndc_secret_name,
            ));
            containers
        },
        volumes: Some(build_volumes(
            configmap_name,
            rndc_secret_name,
            config_map_refs,
            custom_volumes,
        )),
        image_pull_secrets,
        service_account_name: Some(BIND9_SERVICE_ACCOUNT.into()),
        security_context: Some(PodSecurityContext {
            run_as_user: Some(BIND9_NONROOT_UID),
            run_as_group: Some(BIND9_NONROOT_UID),
            fs_group: Some(BIND9_NONROOT_UID),
            run_as_non_root: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Build the Bindcar API sidecar container
///
/// # Arguments
///
/// * `namespace` - Namespace where the container will be deployed
/// * `bindcar_config` - Optional Bindcar container configuration from the instance spec
/// * `rndc_secret_name` - Name of the Secret containing the RNDC key
///
/// # Returns
///
/// A `Container` configured to run the Bindcar RNDC API sidecar
#[allow(clippy::too_many_lines)]
fn build_api_sidecar_container(
    namespace: &str,
    bindcar_config: Option<&crate::crd::BindcarConfig>,
    rndc_secret_name: &str,
) -> Container {
    // Use defaults if bindcar_config is not provided
    let image = bindcar_config
        .and_then(|c| c.image.clone())
        .unwrap_or_else(|| crate::constants::DEFAULT_BINDCAR_IMAGE.to_string());

    let image_pull_policy = bindcar_config
        .and_then(|c| c.image_pull_policy.clone())
        .unwrap_or_else(|| "IfNotPresent".to_string());

    let port = bindcar_config
        .and_then(|c| c.port)
        .unwrap_or(i32::from(crate::constants::BINDCAR_API_PORT));

    let log_level = bindcar_config
        .and_then(|c| c.log_level.clone())
        .unwrap_or_else(|| "info".to_string());

    let resources = bindcar_config.and_then(|c| c.resources.clone());

    // Build required environment variables
    let mut env_vars = vec![
        EnvVar {
            name: "BIND_ZONE_DIR".into(),
            value: Some(BIND_CACHE_PATH.into()),
            ..Default::default()
        },
        EnvVar {
            name: "API_PORT".into(),
            value: Some(port.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "RUST_LOG".into(),
            value: Some(log_level),
            ..Default::default()
        },
        EnvVar {
            name: "BIND_ALLOWED_SERVICE_ACCOUNTS".into(),
            value: Some(format!(
                "system:serviceaccount:{namespace}:{BIND9_SERVICE_ACCOUNT}"
            )),
            ..Default::default()
        },
        EnvVar {
            name: "RNDC_SECRET".into(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: rndc_secret_name.to_string(),
                    key: "secret".to_string(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        EnvVar {
            name: "RNDC_ALGORITHM".into(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: rndc_secret_name.to_string(),
                    key: "algorithm".to_string(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];

    // Add user-provided environment variables if any
    if let Some(config) = bindcar_config {
        if let Some(user_env_vars) = &config.env_vars {
            env_vars.extend(user_env_vars.clone());
        }
    }

    Container {
        name: CONTAINER_NAME_BINDCAR.into(),
        image: Some(image),
        image_pull_policy: Some(image_pull_policy),
        ports: Some(vec![ContainerPort {
            name: Some("http".into()),
            container_port: port,
            protocol: Some("TCP".into()),
            ..Default::default()
        }]),
        env: Some(env_vars),
        volume_mounts: Some(vec![
            VolumeMount {
                name: "cache".into(),
                mount_path: BIND_CACHE_PATH.into(),
                ..Default::default()
            },
            VolumeMount {
                name: "rndc-key".into(),
                mount_path: BIND_KEYS_PATH.into(),
                read_only: Some(true),
                ..Default::default()
            },
            VolumeMount {
                name: VOLUME_CONFIG.into(),
                mount_path: BIND_RNDC_CONF_PATH.into(),
                sub_path: Some(RNDC_CONF_FILENAME.into()),
                ..Default::default()
            },
        ]),
        resources,
        security_context: Some(SecurityContext {
            run_as_non_root: Some(true),
            run_as_user: Some(BIND9_NONROOT_UID),
            run_as_group: Some(BIND9_NONROOT_UID),
            allow_privilege_escalation: Some(false),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Build volume mounts for the BIND9 container
///
/// Creates volume mounts for:
/// - `zones` - `EmptyDir` for zone files
/// - `cache` - `EmptyDir` for BIND9 cache
/// - `named.conf` - From `ConfigMap` (custom or generated)
/// - `named.conf.options` - From `ConfigMap` (custom or generated)
/// - `named.conf.zones` - From custom `ConfigMap` (only if `namedConfZones` is specified)
///
/// # Arguments
///
/// * `config_map_refs` - Optional references to custom `ConfigMaps`
/// * `custom_volume_mounts` - Optional additional volume mounts from instance/cluster spec
///
/// # Returns
///
/// A vector of `VolumeMount` objects for the BIND9 container
fn build_volume_mounts(
    config_map_refs: Option<&ConfigMapRefs>,
    custom_volume_mounts: Option<&Vec<VolumeMount>>,
) -> Vec<VolumeMount> {
    let mut mounts = vec![
        VolumeMount {
            name: VOLUME_ZONES.into(),
            mount_path: BIND_ZONES_PATH.into(),
            ..Default::default()
        },
        VolumeMount {
            name: VOLUME_CACHE.into(),
            mount_path: BIND_CACHE_PATH.into(),
            ..Default::default()
        },
        VolumeMount {
            name: VOLUME_RNDC_KEY.into(),
            mount_path: BIND_KEYS_PATH.into(),
            read_only: Some(true),
            ..Default::default()
        },
    ];

    // Add named.conf mount
    if let Some(refs) = config_map_refs {
        if let Some(_configmap_name) = &refs.named_conf {
            mounts.push(VolumeMount {
                name: VOLUME_NAMED_CONF.into(),
                mount_path: BIND_NAMED_CONF_PATH.into(),
                sub_path: Some(NAMED_CONF_FILENAME.into()),
                ..Default::default()
            });
        } else {
            // Use default generated ConfigMap
            mounts.push(VolumeMount {
                name: VOLUME_CONFIG.into(),
                mount_path: BIND_NAMED_CONF_PATH.into(),
                sub_path: Some(NAMED_CONF_FILENAME.into()),
                ..Default::default()
            });
        }

        if let Some(_configmap_name) = &refs.named_conf_options {
            mounts.push(VolumeMount {
                name: VOLUME_NAMED_CONF_OPTIONS.into(),
                mount_path: BIND_NAMED_CONF_OPTIONS_PATH.into(),
                sub_path: Some(NAMED_CONF_OPTIONS_FILENAME.into()),
                ..Default::default()
            });
        } else {
            // Use default generated ConfigMap
            mounts.push(VolumeMount {
                name: VOLUME_CONFIG.into(),
                mount_path: BIND_NAMED_CONF_OPTIONS_PATH.into(),
                sub_path: Some(NAMED_CONF_OPTIONS_FILENAME.into()),
                ..Default::default()
            });
        }

        // Add zones file mount only if user provided a ConfigMap
        if let Some(_configmap_name) = &refs.named_conf_zones {
            mounts.push(VolumeMount {
                name: VOLUME_NAMED_CONF_ZONES.into(),
                mount_path: BIND_NAMED_CONF_ZONES_PATH.into(),
                sub_path: Some(NAMED_CONF_ZONES_FILENAME.into()),
                ..Default::default()
            });
        }
        // Note: No else block - if user doesn't provide zones ConfigMap, we don't mount it
    } else {
        // No custom ConfigMaps, use default
        mounts.push(VolumeMount {
            name: VOLUME_CONFIG.into(),
            mount_path: BIND_NAMED_CONF_PATH.into(),
            sub_path: Some(NAMED_CONF_FILENAME.into()),
            ..Default::default()
        });
        mounts.push(VolumeMount {
            name: VOLUME_CONFIG.into(),
            mount_path: BIND_NAMED_CONF_OPTIONS_PATH.into(),
            sub_path: Some(NAMED_CONF_OPTIONS_FILENAME.into()),
            ..Default::default()
        });
        // Note: No zones mount - users must explicitly provide namedConfZones ConfigMap
    }

    // Always add rndc.conf mount from default ConfigMap (contains rndc.conf)
    mounts.push(VolumeMount {
        name: VOLUME_CONFIG.into(),
        mount_path: BIND_RNDC_CONF_PATH.into(),
        sub_path: Some(RNDC_CONF_FILENAME.into()),
        ..Default::default()
    });

    // Append custom volume mounts from cluster/instance
    if let Some(custom_mounts) = custom_volume_mounts {
        mounts.extend(custom_mounts.iter().cloned());
    }

    mounts
}

/// Build volumes for the BIND9 pod
///
/// Creates volumes for:
/// - `zones` (`EmptyDir`) - Zone files storage
/// - `cache` (`EmptyDir`) - BIND9 cache
/// - `ConfigMap` volumes (custom or default generated - can be instance or cluster `ConfigMap`)
///
/// If custom `ConfigMaps` are specified via `config_map_refs`, individual volumes are created
/// for each custom `ConfigMap`. If `namedConfZones` is not specified, no zones `ConfigMap` volume
/// is created.
///
/// # Arguments
///
/// * `configmap_name` - Name of the `ConfigMap` to mount (instance or cluster `ConfigMap`)
/// * `config_map_refs` - Optional references to custom `ConfigMaps`
/// * `custom_volumes` - Optional additional volumes from instance/cluster spec
///
/// # Returns
///
/// A vector of `Volume` objects for the pod spec
fn build_volumes(
    configmap_name: &str,
    rndc_secret_name: &str,
    config_map_refs: Option<&ConfigMapRefs>,
    custom_volumes: Option<&Vec<Volume>>,
) -> Vec<Volume> {
    let mut volumes = vec![
        Volume {
            name: VOLUME_ZONES.into(),
            empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: VOLUME_CACHE.into(),
            empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: VOLUME_RNDC_KEY.into(),
            secret: Some(k8s_openapi::api::core::v1::SecretVolumeSource {
                secret_name: Some(rndc_secret_name.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];

    // Add ConfigMap volumes
    if let Some(refs) = config_map_refs {
        if let Some(configmap_name) = &refs.named_conf {
            volumes.push(Volume {
                name: VOLUME_NAMED_CONF.into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: configmap_name.clone(),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }

        if let Some(configmap_name) = &refs.named_conf_options {
            volumes.push(Volume {
                name: VOLUME_NAMED_CONF_OPTIONS.into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: configmap_name.clone(),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }

        if let Some(configmap_name) = &refs.named_conf_zones {
            volumes.push(Volume {
                name: VOLUME_NAMED_CONF_ZONES.into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: configmap_name.clone(),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }

        // If any of the named.conf or named.conf.options use defaults, add the config volume
        // This ensures volume mounts have a corresponding volume
        if refs.named_conf.is_none() || refs.named_conf_options.is_none() {
            volumes.push(Volume {
                name: VOLUME_CONFIG.into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: configmap_name.to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
    } else {
        // No custom ConfigMaps, use default generated one (cluster or instance ConfigMap)
        volumes.push(Volume {
            name: VOLUME_CONFIG.into(),
            config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                name: configmap_name.to_string(),
                ..Default::default()
            }),
            ..Default::default()
        });
    }

    // Append custom volumes from cluster/instance
    if let Some(custom_vols) = custom_volumes {
        volumes.extend(custom_vols.iter().cloned());
    }

    volumes
}

/// Builds a Kubernetes Service for exposing BIND9 DNS ports.
///
/// Creates a Service exposing:
/// - TCP port 53 (for zone transfers and large queries)
/// - UDP port 53 (for standard DNS queries)
/// - HTTP port 80 (mapped to bindcar API port)
///
/// Custom service configuration includes both spec fields and metadata annotations.
/// These are merged with defaults, allowing partial customization while maintaining
/// safe defaults for unspecified fields.
///
/// # Arguments
///
/// * `name` - Name for the Service
/// * `namespace` - Kubernetes namespace
/// * `instance` - The `Bind9Instance` that owns this Service
/// * `custom_config` - Optional custom `ServiceConfig` with spec and annotations to merge with defaults
///
/// # Returns
///
/// A Kubernetes Service resource ready for creation/update
///
/// # Example
///
/// ```rust,no_run
/// use bindy::bind9_resources::build_service;
/// use bindy::crd::{Bind9Instance, ServiceConfig};
/// use std::collections::BTreeMap;
///
/// # fn example(instance: Bind9Instance) {
/// let mut annotations = BTreeMap::new();
/// annotations.insert("metallb.universe.tf/address-pool".to_string(), "my-pool".to_string());
///
/// let config = ServiceConfig {
///     annotations: Some(annotations),
///     spec: None,
/// };
///
/// let service = build_service("dns-primary", "dns-system", &instance, Some(&config));
/// # }
/// ```
#[must_use]
pub fn build_service(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    custom_config: Option<&crate::crd::ServiceConfig>,
) -> Service {
    // Build labels, checking if instance is managed by a cluster
    let labels = build_labels_from_instance(name, instance);
    let owner_refs = build_owner_references(instance);

    // Get API container port from instance spec, default to BINDCAR_API_PORT
    let api_container_port = instance
        .spec
        .bindcar_config
        .as_ref()
        .and_then(|c| c.port)
        .unwrap_or(i32::from(crate::constants::BINDCAR_API_PORT));

    // Build default service spec
    let mut default_spec = ServiceSpec {
        selector: Some(labels.clone()),
        ports: Some(vec![
            ServicePort {
                name: Some("dns-tcp".into()),
                port: i32::from(DNS_PORT),
                target_port: Some(IntOrString::Int(i32::from(DNS_CONTAINER_PORT))),
                protocol: Some("TCP".into()),
                ..Default::default()
            },
            ServicePort {
                name: Some("dns-udp".into()),
                port: i32::from(DNS_PORT),
                target_port: Some(IntOrString::Int(i32::from(DNS_CONTAINER_PORT))),
                protocol: Some("UDP".into()),
                ..Default::default()
            },
            ServicePort {
                name: Some("http".into()),
                port: i32::from(crate::constants::BINDCAR_SERVICE_PORT),
                target_port: Some(IntOrString::Int(api_container_port)),
                protocol: Some("TCP".into()),
                ..Default::default()
            },
        ]),
        type_: Some("ClusterIP".into()),
        ..Default::default()
    };

    // Merge bindcar service spec if provided (applies before custom_config)
    if let Some(bindcar_service_spec) = instance
        .spec
        .bindcar_config
        .as_ref()
        .and_then(|c| c.service_spec.as_ref())
    {
        merge_service_spec(&mut default_spec, bindcar_service_spec);
    }

    // Extract custom spec and annotations from service config
    let (custom_spec, custom_annotations) = custom_config.map_or((None, None), |config| {
        (config.spec.as_ref(), config.annotations.as_ref())
    });

    // Merge custom spec if provided (applies after bindcar config)
    if let Some(custom) = custom_spec {
        merge_service_spec(&mut default_spec, custom);
    }

    // Build metadata with optional annotations
    let mut metadata = ObjectMeta {
        name: Some(name.into()),
        namespace: Some(namespace.into()),
        labels: Some(labels),
        owner_references: Some(owner_refs),
        ..Default::default()
    };

    // Apply custom annotations if provided
    if let Some(annotations) = custom_annotations {
        metadata.annotations = Some(annotations.clone());
    }

    Service {
        metadata,
        spec: Some(default_spec),
        ..Default::default()
    }
}

/// Builds a Kubernetes `ServiceAccount` for BIND9 pods.
///
/// Creates a `ServiceAccount` that will be used by BIND9 pods for authentication
/// to the bindcar API sidecar. This enables service-to-service authentication
/// using Kubernetes service account tokens.
///
/// # Arguments
///
/// * `namespace` - The namespace where the `ServiceAccount` will be created
/// * `instance` - The `Bind9Instance` that owns this `ServiceAccount`
///
/// # Returns
///
/// A `ServiceAccount` configured for BIND9 pods
///
/// # Example
///
/// ```rust,no_run
/// use bindy::bind9_resources::build_service_account;
/// use bindy::crd::Bind9Instance;
///
/// # fn example(instance: Bind9Instance) {
/// let service_account = build_service_account("dns-system", &instance);
/// assert_eq!(service_account.metadata.name, Some("bind9".to_string()));
/// # }
/// ```
#[must_use]
pub fn build_service_account(namespace: &str, _instance: &Bind9Instance) -> ServiceAccount {
    // IMPORTANT: ServiceAccount is SHARED across all Bind9Instance resources in the namespace.
    // Do NOT set ownerReferences, as multiple instances would conflict (only one can have Controller=true).
    // Do NOT use instance-specific labels like managed-by, as multiple instances would conflict during Server-Side Apply.
    // The ServiceAccount will be cleaned up manually or via namespace deletion.

    // Use static labels that don't vary between instances
    let mut labels = BTreeMap::new();
    labels.insert(K8S_NAME.into(), APP_NAME_BIND9.into());
    labels.insert(K8S_COMPONENT.into(), COMPONENT_DNS_SERVER.into());
    labels.insert(K8S_PART_OF.into(), PART_OF_BINDY.into());

    ServiceAccount {
        metadata: ObjectMeta {
            name: Some(BIND9_SERVICE_ACCOUNT.into()),
            namespace: Some(namespace.into()),
            labels: Some(labels),
            owner_references: None, // Shared resource - no owner
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Merge custom service spec fields into the default spec
///
/// Only updates fields that are explicitly specified in the custom spec.
/// This allows partial customization while preserving defaults for other fields.
///
/// The `selector` and `ports` fields are never overridden to ensure the service
/// correctly routes traffic to the BIND9 pods.
fn merge_service_spec(default: &mut ServiceSpec, custom: &ServiceSpec) {
    // Merge type
    if let Some(ref type_) = custom.type_ {
        default.type_ = Some(type_.clone());
    }

    // Merge loadBalancerIP
    if let Some(ref lb_ip) = custom.load_balancer_ip {
        default.load_balancer_ip = Some(lb_ip.clone());
    }

    // Merge sessionAffinity
    if let Some(ref affinity) = custom.session_affinity {
        default.session_affinity = Some(affinity.clone());
    }

    // Merge sessionAffinityConfig
    if let Some(ref config) = custom.session_affinity_config {
        default.session_affinity_config = Some(config.clone());
    }

    // Merge clusterIP
    if let Some(ref cluster_ip) = custom.cluster_ip {
        default.cluster_ip = Some(cluster_ip.clone());
    }

    // Merge externalTrafficPolicy
    if let Some(ref policy) = custom.external_traffic_policy {
        default.external_traffic_policy = Some(policy.clone());
    }

    // Merge loadBalancerSourceRanges
    if let Some(ref ranges) = custom.load_balancer_source_ranges {
        default.load_balancer_source_ranges = Some(ranges.clone());
    }

    // Merge externalIPs
    if let Some(ref ips) = custom.external_ips {
        default.external_ips = Some(ips.clone());
    }

    // Merge loadBalancerClass
    if let Some(ref class) = custom.load_balancer_class {
        default.load_balancer_class = Some(class.clone());
    }

    // Merge healthCheckNodePort
    if let Some(port) = custom.health_check_node_port {
        default.health_check_node_port = Some(port);
    }

    // Merge publishNotReadyAddresses
    if let Some(publish) = custom.publish_not_ready_addresses {
        default.publish_not_ready_addresses = Some(publish);
    }

    // Merge allocateLoadBalancerNodePorts
    if let Some(allocate) = custom.allocate_load_balancer_node_ports {
        default.allocate_load_balancer_node_ports = Some(allocate);
    }

    // Merge internalTrafficPolicy
    if let Some(ref policy) = custom.internal_traffic_policy {
        default.internal_traffic_policy = Some(policy.clone());
    }

    // Merge ipFamilies
    if let Some(ref families) = custom.ip_families {
        default.ip_families = Some(families.clone());
    }

    // Merge ipFamilyPolicy
    if let Some(ref policy) = custom.ip_family_policy {
        default.ip_family_policy = Some(policy.clone());
    }

    // Merge clusterIPs
    if let Some(ref ips) = custom.cluster_ips {
        default.cluster_ips = Some(ips.clone());
    }

    // Merge ports (merge by name, custom ports override defaults)
    if let Some(ref custom_ports) = custom.ports {
        if let Some(ref mut default_ports) = default.ports {
            // Replace ports with matching names, add new ports
            for custom_port in custom_ports {
                if let Some(existing_port) = default_ports
                    .iter_mut()
                    .find(|p| p.name == custom_port.name)
                {
                    // Replace the entire port spec
                    *existing_port = custom_port.clone();
                } else {
                    // Add new port
                    default_ports.push(custom_port.clone());
                }
            }
        } else {
            // No default ports, use custom ports
            default.ports = Some(custom_ports.clone());
        }
    }

    // Note: We intentionally don't merge selector as it needs to match
    // the deployment configuration to ensure traffic is routed correctly.
}
