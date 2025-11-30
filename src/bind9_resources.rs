// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 Kubernetes resource builders
//!
//! This module provides functions to build Kubernetes resources (`Deployment`, `ConfigMap`, `Service`)
//! for BIND9 instances. All functions are pure and easily testable.

use crate::crd::{Bind9Cluster, Bind9Instance, ConfigMapRefs, ImageConfig};
use k8s_openapi::api::{
    apps::v1::{Deployment, DeploymentSpec},
    core::v1::{
        ConfigMap, Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec, Probe, Service,
        ServicePort, ServiceSpec, TCPSocketAction, Volume, VolumeMount,
    },
};
use k8s_openapi::apimachinery::pkg::{
    apis::meta::v1::{LabelSelector, ObjectMeta},
    util::intstr::IntOrString,
};
use std::collections::BTreeMap;
use tracing::debug;

// Embed configuration templates at compile time
const NAMED_CONF_TEMPLATE: &str = include_str!("../templates/named.conf.tmpl");
const NAMED_CONF_OPTIONS_TEMPLATE: &str = include_str!("../templates/named.conf.options.tmpl");

/// Builds standardized Kubernetes labels for BIND9 resources.
///
/// Creates a consistent set of labels following Kubernetes best practices,
/// including recommended `app.kubernetes.io/*` labels.
///
/// # Arguments
///
/// * `instance_name` - Name of the `Bind9Instance` resource
///
/// # Returns
///
/// A `BTreeMap` of label key-value pairs
///
/// # Example
///
/// ```rust
/// use bindy::bind9_resources::build_labels;
///
/// let labels = build_labels("my-dns-server");
/// assert_eq!(labels.get("app").unwrap(), "bind9");
/// assert_eq!(labels.get("instance").unwrap(), "my-dns-server");
/// ```
#[must_use]
pub fn build_labels(instance_name: &str) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert("app".into(), "bind9".into());
    labels.insert("instance".into(), instance_name.into());
    labels.insert("app.kubernetes.io/name".into(), "bind9".into());
    labels.insert("app.kubernetes.io/instance".into(), instance_name.into());
    labels.insert("app.kubernetes.io/component".into(), "dns-server".into());
    labels.insert("app.kubernetes.io/managed-by".into(), "bindy".into());
    labels
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
        .or_else(|| cluster.and_then(|c| c.spec.config_map_refs.as_ref()));

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
    let labels = build_labels(name);

    // Build named.conf
    let named_conf = build_named_conf(instance, cluster);
    data.insert("named.conf".into(), named_conf);

    // Build named.conf.options
    let options_conf = build_options_conf(instance, role_allow_transfer);
    data.insert("named.conf.options".into(), options_conf);

    // Note: We do NOT auto-generate named.conf.zones anymore.
    // Users must explicitly provide a namedConfZones ConfigMap if they want zones support.

    Some(ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!("{name}-config")),
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
        .or_else(|| cluster.and_then(|c| c.spec.config_map_refs.as_ref()));

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

    NAMED_CONF_TEMPLATE.replace("{{ZONES_INCLUDE}}", &zones_include)
}

/// Build the named.conf.options configuration from template
///
/// Generates the BIND9 options configuration file from the instance's config spec.
/// Includes settings for recursion, ACLs (allow-query, allow-transfer), and DNSSEC.
///
/// # Arguments
///
/// * `instance` - `Bind9Instance` spec containing the BIND9 configuration
/// * `pod_cidrs` - Pod CIDR ranges from the cluster nodes, used for default allow-transfer
/// * `role_allow_transfer` - Role-specific allow-transfer override from cluster spec (primary/secondary)
///
/// # Returns
///
/// A string containing the complete named.conf.options configuration
fn build_options_conf(
    instance: &Bind9Instance,
    role_allow_transfer: Option<&Vec<String>>,
) -> String {
    let recursion;
    let mut allow_query = String::new();
    let allow_transfer;
    let mut dnssec_validate = String::new();

    if let Some(config) = &instance.spec.config {
        // Recursion setting
        let recursion_value = if config.recursion.unwrap_or(false) {
            "yes"
        } else {
            "no"
        };
        recursion = format!("\n    recursion {recursion_value};");

        // Allow-query ACL
        if let Some(acls) = &config.allow_query {
            if !acls.is_empty() {
                let acl_list = acls.join("; ");
                allow_query = format!("\n    allow-query {{ {acl_list}; }};");
            }
        }

        // Allow-transfer ACL - priority: instance config > role-specific > no default (use BIND9's default)
        if let Some(acls) = &config.allow_transfer {
            // Instance-level config takes highest priority
            let acl_list = if acls.is_empty() {
                "none".to_string()
            } else {
                acls.join("; ")
            };
            allow_transfer = format!("\n    allow-transfer {{ {acl_list}; }};");
        } else if let Some(role_acls) = role_allow_transfer {
            // Role-specific override from cluster config (primary/secondary)
            let acl_list = if role_acls.is_empty() {
                "none".to_string()
            } else {
                role_acls.join("; ")
            };
            allow_transfer = format!("\n    allow-transfer {{ {acl_list}; }};");
        } else {
            // No default - let BIND9 use its own defaults (none)
            allow_transfer = String::new();
        }

        // DNSSEC configuration
        // Note: dnssec-enable was removed in BIND 9.15+ (DNSSEC is always enabled)
        // Only dnssec-validation is configurable now
        if let Some(dnssec) = &config.dnssec {
            if dnssec.validation.unwrap_or(false) {
                dnssec_validate = "\n    dnssec-validation yes;".to_string();
            }
        }
    } else {
        // Defaults when no config is specified
        recursion = "\n    recursion no;".to_string();
        // No default for allow-transfer - let BIND9 use its own defaults (none)
        allow_transfer = String::new();
    }

    // Perform template substitutions
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
pub fn build_deployment(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
) -> Deployment {
    debug!(
        name = %name,
        namespace = %namespace,
        has_cluster = cluster.is_some(),
        "Building Deployment for Bind9Instance"
    );

    let labels = build_labels(name);
    let replicas = instance.spec.replicas.unwrap_or(1);
    debug!(replicas, "Deployment replica count");

    // Get image config (instance overrides cluster)
    let image_config = instance
        .spec
        .image
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.image.as_ref()));

    // Get ConfigMap references (instance overrides cluster)
    let config_map_refs = instance
        .spec
        .config_map_refs
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.config_map_refs.as_ref()));

    // Get version (instance overrides cluster)
    let version = instance
        .spec
        .version
        .as_deref()
        .or_else(|| cluster.and_then(|c| c.spec.version.as_deref()))
        .unwrap_or("9.18");

    // Get volumes (instance overrides cluster)
    let volumes = instance
        .spec
        .volumes
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.volumes.as_ref()));

    // Get volume mounts (instance overrides cluster)
    let volume_mounts = instance
        .spec
        .volume_mounts
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.volume_mounts.as_ref()));

    Deployment {
        metadata: ObjectMeta {
            name: Some(name.into()),
            namespace: Some(namespace.into()),
            labels: Some(labels.clone()),
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
                    name,
                    version,
                    image_config,
                    config_map_refs,
                    volumes,
                    volume_mounts,
                )),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Build the `PodSpec` for BIND9
fn build_pod_spec(
    instance_name: &str,
    version: &str,
    image_config: Option<&ImageConfig>,
    config_map_refs: Option<&ConfigMapRefs>,
    custom_volumes: Option<&Vec<Volume>>,
    custom_volume_mounts: Option<&Vec<VolumeMount>>,
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
        name: "bind9".into(),
        image: Some(image),
        image_pull_policy: Some(image_pull_policy),
        command: Some(vec!["named".into()]),
        args: Some(vec![
            "-c".into(),
            "/etc/bind/named.conf".into(),
            "-g".into(), // Run in foreground (required for containers)
        ]),
        ports: Some(vec![
            ContainerPort {
                name: Some("dns-tcp".into()),
                container_port: 53,
                protocol: Some("TCP".into()),
                ..Default::default()
            },
            ContainerPort {
                name: Some("dns-udp".into()),
                container_port: 53,
                protocol: Some("UDP".into()),
                ..Default::default()
            },
            ContainerPort {
                name: Some("rndc".into()),
                container_port: 953,
                protocol: Some("TCP".into()),
                ..Default::default()
            },
        ]),
        env: Some(vec![EnvVar {
            name: "TZ".into(),
            value: Some("UTC".into()),
            ..Default::default()
        }]),
        volume_mounts: Some(build_volume_mounts(config_map_refs, custom_volume_mounts)),
        liveness_probe: Some(Probe {
            tcp_socket: Some(TCPSocketAction {
                port: IntOrString::Int(53),
                ..Default::default()
            }),
            initial_delay_seconds: Some(30),
            period_seconds: Some(10),
            timeout_seconds: Some(5),
            failure_threshold: Some(3),
            ..Default::default()
        }),
        readiness_probe: Some(Probe {
            tcp_socket: Some(TCPSocketAction {
                port: IntOrString::Int(53),
                ..Default::default()
            }),
            initial_delay_seconds: Some(10),
            period_seconds: Some(5),
            timeout_seconds: Some(3),
            failure_threshold: Some(3),
            ..Default::default()
        }),
        ..Default::default()
    };

    // Build image pull secrets if specified
    let image_pull_secrets = image_config.and_then(|cfg| {
        cfg.image_pull_secrets.as_ref().map(|secrets| {
            secrets
                .iter()
                .map(|s| k8s_openapi::api::core::v1::LocalObjectReference {
                    name: Some(s.clone()),
                })
                .collect()
        })
    });

    PodSpec {
        containers: vec![bind9_container],
        volumes: Some(build_volumes(
            instance_name,
            config_map_refs,
            custom_volumes,
        )),
        image_pull_secrets,
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
            name: "zones".into(),
            mount_path: "/etc/bind/zones".into(),
            ..Default::default()
        },
        VolumeMount {
            name: "cache".into(),
            mount_path: "/var/cache/bind".into(),
            ..Default::default()
        },
    ];

    // Add named.conf mount
    if let Some(refs) = config_map_refs {
        if let Some(_configmap_name) = &refs.named_conf {
            mounts.push(VolumeMount {
                name: "named-conf".into(),
                mount_path: "/etc/bind/named.conf".into(),
                sub_path: Some("named.conf".into()),
                ..Default::default()
            });
        } else {
            // Use default generated ConfigMap
            mounts.push(VolumeMount {
                name: "config".into(),
                mount_path: "/etc/bind/named.conf".into(),
                sub_path: Some("named.conf".into()),
                ..Default::default()
            });
        }

        if let Some(_configmap_name) = &refs.named_conf_options {
            mounts.push(VolumeMount {
                name: "named-conf-options".into(),
                mount_path: "/etc/bind/named.conf.options".into(),
                sub_path: Some("named.conf.options".into()),
                ..Default::default()
            });
        } else {
            // Use default generated ConfigMap
            mounts.push(VolumeMount {
                name: "config".into(),
                mount_path: "/etc/bind/named.conf.options".into(),
                sub_path: Some("named.conf.options".into()),
                ..Default::default()
            });
        }

        // Add zones file mount only if user provided a ConfigMap
        if let Some(_configmap_name) = &refs.named_conf_zones {
            mounts.push(VolumeMount {
                name: "named-conf-zones".into(),
                mount_path: "/etc/bind/named.conf.zones".into(),
                sub_path: Some("named.conf.zones".into()),
                ..Default::default()
            });
        }
        // Note: No else block - if user doesn't provide zones ConfigMap, we don't mount it
    } else {
        // No custom ConfigMaps, use default
        mounts.push(VolumeMount {
            name: "config".into(),
            mount_path: "/etc/bind/named.conf".into(),
            sub_path: Some("named.conf".into()),
            ..Default::default()
        });
        mounts.push(VolumeMount {
            name: "config".into(),
            mount_path: "/etc/bind/named.conf.options".into(),
            sub_path: Some("named.conf.options".into()),
            ..Default::default()
        });
        // Note: No zones mount - users must explicitly provide namedConfZones ConfigMap
    }

    // Append custom volume mounts from cluster/instance
    if let Some(custom_mounts) = custom_volume_mounts {
        mounts.extend(custom_mounts.iter().cloned());
    }

    mounts
}

/// Build volumes for the BIND9 pod
///
/// Creates volumes for:
/// - `zones` - `EmptyDir` for zone files
/// - `cache` - `EmptyDir` for BIND9 cache
/// - `ConfigMap` volumes (custom or default generated)
///
/// If custom `ConfigMaps` are specified via `config_map_refs`, individual volumes are created
/// for each custom `ConfigMap`. If `namedConfZones` is not specified, no zones `ConfigMap` volume
/// is created.
///
/// # Arguments
///
/// * `instance_name` - Name of the instance (used for default `ConfigMap` name)
/// * `config_map_refs` - Optional references to custom `ConfigMaps`
/// * `custom_volumes` - Optional additional volumes from instance/cluster spec
///
/// # Returns
///
/// A vector of `Volume` objects for the pod spec
fn build_volumes(
    instance_name: &str,
    config_map_refs: Option<&ConfigMapRefs>,
    custom_volumes: Option<&Vec<Volume>>,
) -> Vec<Volume> {
    let mut volumes = vec![
        Volume {
            name: "zones".into(),
            empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "cache".into(),
            empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource::default()),
            ..Default::default()
        },
    ];

    // Add ConfigMap volumes
    if let Some(refs) = config_map_refs {
        if let Some(configmap_name) = &refs.named_conf {
            volumes.push(Volume {
                name: "named-conf".into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: Some(configmap_name.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }

        if let Some(configmap_name) = &refs.named_conf_options {
            volumes.push(Volume {
                name: "named-conf-options".into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: Some(configmap_name.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }

        if let Some(configmap_name) = &refs.named_conf_zones {
            volumes.push(Volume {
                name: "named-conf-zones".into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: Some(configmap_name.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }

        // If any of the named.conf or named.conf.options use defaults, add the config volume
        // This ensures volume mounts have a corresponding volume
        if refs.named_conf.is_none() || refs.named_conf_options.is_none() {
            volumes.push(Volume {
                name: "config".into(),
                config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                    name: Some(format!("{instance_name}-config")),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
    } else {
        // No custom ConfigMaps, use default generated one
        volumes.push(Volume {
            name: "config".into(),
            config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                name: Some(format!("{instance_name}-config")),
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
///
/// Custom service spec fields are merged with defaults. This allows partial
/// customization while maintaining safe defaults for unspecified fields.
///
/// # Arguments
///
/// * `name` - Name for the Service
/// * `namespace` - Kubernetes namespace
/// * `custom_spec` - Optional custom `ServiceSpec` fields to merge with defaults
///
/// # Returns
///
/// A Kubernetes Service resource ready for creation/update
#[must_use]
pub fn build_service(name: &str, namespace: &str, custom_spec: Option<&ServiceSpec>) -> Service {
    let labels = build_labels(name);

    // Build default service spec
    let mut default_spec = ServiceSpec {
        selector: Some(labels.clone()),
        ports: Some(vec![
            ServicePort {
                name: Some("dns-tcp".into()),
                port: 53,
                target_port: Some(IntOrString::Int(53)),
                protocol: Some("TCP".into()),
                ..Default::default()
            },
            ServicePort {
                name: Some("dns-udp".into()),
                port: 53,
                target_port: Some(IntOrString::Int(53)),
                protocol: Some("UDP".into()),
                ..Default::default()
            },
        ]),
        type_: Some("ClusterIP".into()),
        ..Default::default()
    };

    // Merge custom spec if provided
    if let Some(custom) = custom_spec {
        merge_service_spec(&mut default_spec, custom);
    }

    Service {
        metadata: ObjectMeta {
            name: Some(name.into()),
            namespace: Some(namespace.into()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(default_spec),
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

    // Note: We intentionally don't merge ports or selector as they need to match
    // the deployment configuration to ensure traffic is routed correctly.
}
