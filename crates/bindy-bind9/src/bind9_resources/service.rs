// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The DNS `Service`, the shared `ServiceAccount` and the per-role
//! `PodDisruptionBudget`.

use super::meta::{
    build_cluster_labels, build_labels_from_instance, build_owner_references,
    controller_owner_reference,
};
use crate::constants::{
    BIND9_SERVICE_ACCOUNT, DNS_CONTAINER_PORT, DNS_PORT, MAX_UNAVAILABLE_OPERANDS,
};
use crate::crd::{Bind9Cluster, Bind9Instance, ServerRole};
use crate::labels::{
    APP_NAME_BIND9, BINDY_CLUSTER_LABEL, BINDY_ROLE_LABEL, COMPONENT_DNS_SERVER, K8S_COMPONENT,
    K8S_NAME, K8S_PART_OF, PART_OF_BINDY,
};
use k8s_openapi::api::{
    core::v1::{Service, ServiceAccount, ServicePort, ServiceSpec},
    policy::v1::{PodDisruptionBudget, PodDisruptionBudgetSpec},
};
use k8s_openapi::apimachinery::pkg::{
    apis::meta::v1::{LabelSelector, ObjectMeta},
    util::intstr::IntOrString,
};
use std::collections::BTreeMap;

/// Builds a `PodDisruptionBudget` covering every BIND9 Pod of one cluster in one role.
///
/// Voluntary disruption — a node drain, a cluster upgrade, a descheduler — is
/// otherwise free to evict every primary of a cluster simultaneously. That is
/// not hypothetical: deleting all primaries at once leaves the replacement Pods
/// `Ready` (the readiness probe is a bare TCP connect) while they serve no
/// zones at all, because BIND9 keeps zone data in the Pod and the operator has
/// to push every zone back. Measured at roughly 115 seconds of REFUSED answers,
/// against about 1 second when a single primary is replaced and its peers keep
/// serving. Capping disruption at one Pod keeps a restart in the second regime.
///
/// Uses `maxUnavailable: 1` rather than `minAvailable`. A cluster with a single
/// primary and `minAvailable: 1` can never release that Pod, which does not
/// protect the zone — it just hangs `kubectl drain` forever.
///
/// # Arguments
///
/// * `cluster_name` - Name of the owning `Bind9Cluster`
/// * `namespace` - Namespace to create the budget in
/// * `role` - Which role's Pods this budget covers; primaries and secondaries
///   get separate budgets so draining one never spends the other's allowance
/// * `cluster` - The owning `Bind9Cluster`, when available, for the owner
///   reference that garbage collects this budget with its cluster
///
/// # Returns
///
/// A `PodDisruptionBudget` selecting the operand Pods of `cluster_name` in `role`
///
/// # Errors
///
/// Returns an error when `cluster` is given but has no UID
/// ([`controller_owner_reference`]).
pub fn build_pod_disruption_budget(
    cluster_name: &str,
    namespace: &str,
    role: ServerRole,
    cluster: Option<&Bind9Cluster>,
) -> anyhow::Result<PodDisruptionBudget> {
    let role_str = role.as_str();

    // Matches the labels build_labels_from_instance puts on the operand Pods.
    let mut match_labels = BTreeMap::new();
    match_labels.insert("app".to_string(), APP_NAME_BIND9.to_string());
    match_labels.insert(BINDY_CLUSTER_LABEL.to_string(), cluster_name.to_string());
    match_labels.insert(BINDY_ROLE_LABEL.to_string(), role_str.to_string());

    let owner_references = cluster
        .map(|c| controller_owner_reference(c).map(|owner| vec![owner]))
        .transpose()?;

    Ok(PodDisruptionBudget {
        metadata: ObjectMeta {
            name: Some(format!("{cluster_name}-{role_str}-pdb")),
            namespace: Some(namespace.to_string()),
            labels: Some(build_cluster_labels(cluster_name)),
            owner_references,
            ..Default::default()
        },
        spec: Some(PodDisruptionBudgetSpec {
            max_unavailable: Some(IntOrString::Int(MAX_UNAVAILABLE_OPERANDS)),
            selector: Some(LabelSelector {
                match_labels: Some(match_labels),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
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
/// * `bindcar_config` - The instance's bindcar configuration merged across
///   instance, cluster and provider ([`resolve_bindcar_config`](super::resolve_bindcar_config)), the same
///   one the Deployment's sidecar is built from, so the `http` port targets
///   the port the sidecar listens on
/// * `custom_config` - Optional custom `ServiceConfig` with spec and annotations to merge with defaults
///
/// # Returns
///
/// A Kubernetes Service resource ready for creation/update
///
/// # Errors
///
/// Returns an error when the instance has no UID for the owner reference
/// ([`controller_owner_reference`]).
///
/// # Example
///
/// ```rust,no_run
/// use bindy_bind9::bind9_resources::{build_service, resolve_bindcar_config};
/// use bindy_api::crd::{Bind9Instance, ServiceConfig};
/// use std::collections::BTreeMap;
///
/// # fn example(instance: Bind9Instance) -> anyhow::Result<()> {
/// let mut annotations = BTreeMap::new();
/// annotations.insert("metallb.universe.tf/address-pool".to_string(), "my-pool".to_string());
///
/// let config = ServiceConfig {
///     annotations: Some(annotations),
///     spec: None,
/// };
///
/// let bindcar = resolve_bindcar_config(&instance, None, None);
/// let service = build_service(
///     "dns-primary",
///     "bindy-system",
///     &instance,
///     bindcar.as_ref(),
///     Some(&config),
/// )?;
/// # Ok(())
/// # }
/// ```
pub fn build_service(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    bindcar_config: Option<&crate::crd::BindcarConfig>,
    custom_config: Option<&crate::crd::ServiceConfig>,
) -> anyhow::Result<Service> {
    // Build labels, checking if instance is managed by a cluster
    let labels = build_labels_from_instance(name, instance);
    let owner_refs = build_owner_references(instance)?;

    // The port the sidecar listens on, default BINDCAR_API_PORT
    let api_container_port = bindcar_config
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
    if let Some(bindcar_service_spec) = bindcar_config.and_then(|c| c.service_spec.as_ref()) {
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

    Ok(Service {
        metadata,
        spec: Some(default_spec),
        ..Default::default()
    })
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
///
/// # Returns
///
/// A `ServiceAccount` configured for BIND9 pods
///
/// # Example
///
/// ```rust,no_run
/// use bindy_bind9::bind9_resources::build_service_account;
///
/// let service_account = build_service_account("bindy-system");
/// assert_eq!(service_account.metadata.name, Some("bind9".to_string()));
/// ```
#[must_use]
pub fn build_service_account(namespace: &str) -> ServiceAccount {
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
/// `ports` are merged by name: a custom port replaces the default port of the
/// same name whole, and a custom port with a new name is appended. The
/// `selector` is never overridden, so the service always routes traffic to
/// the BIND9 pods.
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
