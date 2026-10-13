// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The BIND9 `Deployment`: its pod spec, the `named` container, volumes and
//! mounts.

use super::dnssec::{build_dnssec_key_volumes, build_dnssec_keys_init_container};
use super::meta::{
    build_labels_from_instance, build_owner_references, build_pod_labels_from_instance,
    instance_configmap_name,
};
use super::sidecar::{build_api_sidecar_container, resolve_bindcar_config, VOLUME_BINDCAR_TLS};
use super::{
    BIND_CACHE_PATH, BIND_KEYS_PATH, BIND_NAMED_CONF_OPTIONS_PATH, BIND_NAMED_CONF_PATH,
    BIND_NAMED_CONF_ZONES_PATH, BIND_RNDC_CONF_PATH, BIND_ZONES_PATH, EMPTY_DIR_MEDIUM_MEMORY,
    NAMED_CONF_FILENAME, NAMED_CONF_OPTIONS_FILENAME, NAMED_CONF_ZONES_FILENAME,
    RNDC_CONF_FILENAME, VOLUME_CACHE, VOLUME_CONFIG, VOLUME_NAMED_CONF, VOLUME_NAMED_CONF_OPTIONS,
    VOLUME_NAMED_CONF_ZONES, VOLUME_RNDC_KEY, VOLUME_TMP, VOLUME_ZONES,
};
use crate::constants::{
    BIND9_MALLOC_CONF, BIND9_NONROOT_UID, BIND9_PRESTOP_DRAIN_SECS, BIND9_SERVICE_ACCOUNT,
    BIND9_TERMINATION_GRACE_PERIOD_SECS, CONTAINER_NAME_BIND9, DEFAULT_BIND9_VERSION,
    DNS_CONTAINER_PORT, LIVENESS_FAILURE_THRESHOLD, LIVENESS_INITIAL_DELAY_SECS,
    LIVENESS_PERIOD_SECS, LIVENESS_TIMEOUT_SECS, READINESS_FAILURE_THRESHOLD,
    READINESS_INITIAL_DELAY_SECS, READINESS_PERIOD_SECS, READINESS_TIMEOUT_SECS, RNDC_PORT,
};
use crate::crd::{
    Bind9Cluster, Bind9ClusterCommonSpec, Bind9Instance, ConfigMapRefs, ImageConfig, ServerRole,
};
use k8s_openapi::api::{
    apps::v1::{Deployment, DeploymentSpec},
    core::v1::{
        Capabilities, ConfigMapVolumeSource, Container, ContainerPort, EmptyDirVolumeSource,
        EnvVar, EnvVarSource, ExecAction, Lifecycle, LifecycleHandler, PodSecurityContext, PodSpec,
        PodTemplateSpec, Probe, SeccompProfile, SecretKeySelector, SecurityContext,
        TCPSocketAction, Volume, VolumeMount,
    },
};
use k8s_openapi::apimachinery::pkg::{
    apis::meta::v1::{LabelSelector, ObjectMeta},
    util::intstr::IntOrString,
};
use tracing::debug;

/// The configuration a `Bind9Instance`'s Deployment is built from, resolved
/// across instance, cluster and cluster provider.
struct DeploymentConfig<'a> {
    image_config: Option<&'a ImageConfig>,
    config_map_refs: Option<&'a ConfigMapRefs>,
    version: &'a str,
    volumes: Option<&'a Vec<Volume>>,
    volume_mounts: Option<&'a Vec<VolumeMount>>,
    bindcar_config: Option<crate::crd::BindcarConfig>,
    configmap_name: String,
}

/// The instance's own value, else the first inherited level that sets it.
///
/// # Arguments
///
/// * `own` - The value on the `Bind9Instance` itself
/// * `inherited` - The common specs it inherits from, most specific first
///   (its `Bind9Cluster`, then its `ClusterBind9Provider`)
/// * `get` - Reads the field from a common spec
fn pick<'a, T: ?Sized>(
    own: Option<&'a T>,
    inherited: &[Option<&'a Bind9ClusterCommonSpec>],
    get: impl Fn(&'a Bind9ClusterCommonSpec) -> Option<&'a T>,
) -> Option<&'a T> {
    own.or_else(|| inherited.iter().flatten().find_map(|common| get(common)))
}

/// Extract and resolve deployment configuration from instance and cluster
fn resolve_deployment_config<'a>(
    name: &str,
    instance: &'a Bind9Instance,
    cluster: Option<&'a Bind9Cluster>,
    cluster_provider: Option<&'a crate::crd::ClusterBind9Provider>,
) -> DeploymentConfig<'a> {
    // Each field: instance overrides cluster overrides cluster provider.
    let inherited = [
        cluster.map(|c| &c.spec.common),
        cluster_provider.map(|cp| &cp.spec.common),
    ];
    let spec = &instance.spec;

    let image_config = pick(spec.image.as_ref(), &inherited, |c| c.image.as_ref());
    let config_map_refs = pick(spec.config_map_refs.as_ref(), &inherited, |c| {
        c.config_map_refs.as_ref()
    });
    let version = pick(spec.version.as_deref(), &inherited, |c| {
        c.version.as_deref()
    })
    .unwrap_or(DEFAULT_BIND9_VERSION);
    let volumes = pick(spec.volumes.as_ref(), &inherited, |c| c.volumes.as_ref());
    let volume_mounts = pick(spec.volume_mounts.as_ref(), &inherited, |c| {
        c.volume_mounts.as_ref()
    });

    // Merged per field, so an instance setting one field does not discard the
    // rest of the cluster's sidecar configuration. See resolve_bindcar_config.
    let bindcar_config = resolve_bindcar_config(instance, cluster, cluster_provider);

    // The cluster's shared ConfigMap for an instance with a clusterRef, its
    // own otherwise; the drift check reads the same name.
    let configmap_name = instance_configmap_name(name, instance);

    DeploymentConfig {
        image_config,
        config_map_refs,
        version,
        volumes,
        volume_mounts,
        bindcar_config,
        configmap_name,
    }
}

/// Counts how many instances of each role the owning cluster asks for.
///
/// Returns `(role_instance_count, cluster_instance_count)`, defaulting to
/// `(1, 1)` for a standalone `Bind9Instance` that has no owning cluster.
///
/// These counts drive the *default* spread decision only. They matter because
/// a cluster-managed instance always has `replicas: 1` (the cluster
/// controller creates one single-Pod Deployment per nameserver), so replica
/// count alone would never reach the "two or more Pods" threshold, and the
/// default would never fire for exactly the topology it exists to protect.
fn resolve_role_counts(
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> (i32, i32) {
    let common = cluster
        .map(|c| &c.spec.common)
        .or_else(|| cluster_provider.map(|p| &p.spec.common));

    let Some(common) = common else {
        return (1, 1);
    };

    let primaries = common.role_replicas(ServerRole::Primary).unwrap_or(0);
    let secondaries = common.role_replicas(ServerRole::Secondary).unwrap_or(0);
    let role_count = common.role_replicas(instance.spec.role).unwrap_or(0);

    (role_count.max(1), (primaries + secondaries).max(1))
}

/// Builds the BIND9 `Deployment` of a `Bind9Instance`, with the bindcar
/// sidecar, owned by the instance.
///
/// The Deployment runs:
/// - the BIND9 container, from the configured or default image, with
///   liveness and readiness probes on the DNS port
/// - the bindcar API sidecar
/// - the DNSSEC key init container, when signing keys come from a Secret
///
/// and mounts the configuration `ConfigMap` (generated or custom), the RNDC
/// key Secret, and `emptyDir` volumes for zones and cache.
///
/// # Arguments
///
/// * `name` - Name of the Deployment (the instance name)
/// * `namespace` - Kubernetes namespace
/// * `instance` - The owning `Bind9Instance`
/// * `cluster` - The instance's `Bind9Cluster`, if any
/// * `cluster_provider` - The instance's `ClusterBind9Provider`, if any
/// * `rndc_secret_name` - The RNDC `Secret` to mount ([`rndc_secret_name`](super::rndc_secret_name)):
///   from `rndcKey.secretRef`, an inline secret spec, or the generated
///   `{name}-rndc-key` default
///
/// # Returns
///
/// The desired Deployment.
///
/// # Errors
///
/// Returns an error when the instance has no UID for the owner reference
/// ([`controller_owner_reference`](super::controller_owner_reference)).
pub fn build_deployment(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
    rndc_secret_name: &str,
) -> anyhow::Result<Deployment> {
    debug!(
        name = %name,
        namespace = %namespace,
        has_cluster = cluster.is_some(),
        has_cluster_provider = cluster_provider.is_some(),
        "Building Deployment for Bind9Instance"
    );

    // Two label sets, deliberately. `selector_labels` is frozen and owns the
    // Deployment's immutable `spec.selector`; `pod_labels` is a superset
    // stamped on the Pod template. See `build_pod_labels_from_instance`.
    let selector_labels = build_labels_from_instance(name, instance);
    let pod_labels = build_pod_labels_from_instance(name, instance);
    let replicas = instance.spec.replicas.unwrap_or(1);
    debug!(replicas, "Deployment replica count");

    // Resolve scheduling: instance -> role -> cluster, then turn the winning
    // block (or the operator default) into concrete Pod spec fields.
    let (role_instance_count, cluster_instance_count) =
        resolve_role_counts(instance, cluster, cluster_provider);
    let placement_config = crate::placement::resolve_placement(instance, cluster, cluster_provider);
    let placement_ctx = crate::placement::PlacementContext {
        instance_name: name,
        cluster_name: (!instance.spec.cluster_ref.is_empty())
            .then_some(instance.spec.cluster_ref.as_str()),
        role: instance.spec.role,
        instance_replicas: replicas,
        role_instance_count,
        cluster_instance_count,
        instance_selector_labels: &selector_labels,
    };
    let placement = crate::placement::build_pod_placement(placement_config, &placement_ctx);

    let config = resolve_deployment_config(name, instance, cluster, cluster_provider);

    let owner_refs = build_owner_references(instance)?;

    // Get global and instance configs for DNSSEC
    let global_config = cluster.and_then(|c| c.spec.common.global.as_ref());
    let instance_config = instance.spec.config.as_ref();

    // Build DNSSEC key volumes if signing is enabled
    let (dnssec_volumes, dnssec_volume_mounts) =
        build_dnssec_key_volumes(global_config, instance_config);

    // Copies Secret-supplied keys into the key directory (ADR-0012).
    let (bind9_image, bind9_pull_policy) = resolve_bind9_image(config.image_config, config.version);
    let dnssec_init_container = build_dnssec_keys_init_container(
        global_config,
        instance_config,
        &bind9_image,
        &bind9_pull_policy,
    );

    // Merge DNSSEC volumes with custom volumes from spec
    let all_volumes = if dnssec_volumes.is_empty() {
        config.volumes.map(std::borrow::ToOwned::to_owned)
    } else {
        let mut merged = dnssec_volumes;
        if let Some(custom) = config.volumes {
            merged.extend(custom.iter().cloned());
        }
        Some(merged)
    };

    // Merge DNSSEC volume mounts with custom volume mounts from spec
    let all_volume_mounts = if dnssec_volume_mounts.is_empty() {
        config.volume_mounts.map(std::borrow::ToOwned::to_owned)
    } else {
        let mut merged = dnssec_volume_mounts;
        if let Some(custom) = config.volume_mounts {
            merged.extend(custom.iter().cloned());
        }
        Some(merged)
    };

    Ok(Deployment {
        metadata: ObjectMeta {
            name: Some(name.into()),
            namespace: Some(namespace.into()),
            // Deployment metadata keeps the original (selector) label set:
            // widening it is not needed for scheduling and would change a
            // contract other tooling may select on. Only the Pod template
            // gains the cluster label.
            labels: Some(selector_labels.clone()),
            owner_references: Some(owner_refs),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(replicas),
            // IMMUTABLE. Never widen this set — see
            // `build_pod_labels_from_instance` for why new labels go on the
            // Pod template instead.
            selector: LabelSelector {
                match_labels: Some(selector_labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(pod_labels.clone()),
                    ..Default::default()
                }),
                spec: Some(build_pod_spec(
                    &config.configmap_name,
                    rndc_secret_name,
                    config.version,
                    config.image_config,
                    config.config_map_refs,
                    all_volumes.as_ref(),
                    all_volume_mounts.as_ref(),
                    config.bindcar_config.as_ref(),
                    &placement,
                    dnssec_init_container.map(|c| vec![c]),
                )),
            },
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// The BIND9 image and its pull policy: the configured image, else the ISC
/// image at `version`, pulled `IfNotPresent` unless configured otherwise.
fn resolve_bind9_image(image_config: Option<&ImageConfig>, version: &str) -> (String, String) {
    let image = image_config
        .and_then(|img_cfg| img_cfg.image.clone())
        .unwrap_or_else(|| format!("internetsystemsconsortium/bind9:{version}"));
    let image_pull_policy = image_config
        .and_then(|cfg| cfg.image_pull_policy.clone())
        .unwrap_or_else(|| DEFAULT_IMAGE_PULL_POLICY.into());
    (image, image_pull_policy)
}

/// Builds pod specification with BIND9 container and API sidecar
///
/// # Arguments
/// * `configmap_name` - Name of the `ConfigMap` with BIND9 configuration
/// * `rndc_secret_name` - Name of the Secret with RNDC keys
/// * `version` - BIND9 version tag
/// * `image_config` - Optional custom image configuration
/// * `config_map_refs` - Optional custom `ConfigMap` references
/// * `custom_volumes` - Optional custom volumes to add
/// * `custom_volume_mounts` - Optional custom volume mounts to add
/// * `bindcar_config` - Optional API sidecar configuration
/// * `placement` - Resolved topology spread constraints from `crate::placement`
/// * `init_containers` - Containers to run before `named` (the DNSSEC key copy)
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn build_pod_spec(
    configmap_name: &str,
    rndc_secret_name: &str,
    version: &str,
    image_config: Option<&ImageConfig>,
    config_map_refs: Option<&ConfigMapRefs>,
    custom_volumes: Option<&Vec<Volume>>,
    custom_volume_mounts: Option<&Vec<VolumeMount>>,
    bindcar_config: Option<&crate::crd::BindcarConfig>,
    placement: &crate::placement::ResolvedPlacement,
    init_containers: Option<Vec<Container>>,
) -> PodSpec {
    let (image, image_pull_policy) = resolve_bind9_image(image_config, version);

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
            env("TZ", "UTC"),
            env("MALLOC_CONF", BIND9_MALLOC_CONF),
        ]),
        volume_mounts: Some(build_volume_mounts(config_map_refs, custom_volume_mounts)),
        liveness_probe: Some(timed_probe(&LIVENESS_TIMING, dns_tcp_probe())),
        readiness_probe: Some(timed_probe(&READINESS_TIMING, dns_tcp_probe())),
        // Graceful shutdown. Kubernetes removes the Pod from the Service
        // endpoints and signals the container at the same time, so `named` can
        // otherwise exit while kube-proxy still forwards queries to it. Sleep
        // first so the removal propagates, then flush journals to disk — that
        // last part only matters when the zone directory is a PVC rather than
        // the default emptyDir, but it is cheap and correct either way.
        //
        // Every step is best-effort: a preStop hook that exits non-zero is
        // logged as a Pod event, and a zone that cannot be frozen must not
        // block termination.
        lifecycle: Some(Lifecycle {
            pre_stop: Some(LifecycleHandler {
                exec: Some(ExecAction {
                    command: Some(vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        format!(
                            "sleep {BIND9_PRESTOP_DRAIN_SECS}; \
                             rndc -c {BIND_RNDC_CONF_PATH} sync -clean || true"
                        ),
                    ]),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        // ALL capabilities dropped and none added back: `named` binds the
        // unprivileged DNS port 5353 (DNS_CONTAINER_PORT), so it no longer
        // needs NET_BIND_SERVICE. Unlike the other containers, `named` does
        // not set `readOnlyRootFilesystem`.
        security_context: Some(restricted_security_context(false)),
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
            bindcar_config.and_then(|c| c.tls.as_ref()),
        )),
        init_containers,
        image_pull_secrets,
        service_account_name: Some(BIND9_SERVICE_ACCOUNT.into()),
        // Must outlast the preStop drain above, or the kubelet SIGKILLs the
        // container mid-hook.
        termination_grace_period_seconds: Some(BIND9_TERMINATION_GRACE_PERIOD_SECS),
        // Scheduling. Topology spreading only — see `crate::placement` for why
        // this is not a general pod-spec passthrough.
        topology_spread_constraints: placement.topology_spread_constraints.clone(),
        // The pod is Ready only once the operator has loaded every live zone
        // of its instance onto it (ADR-0017). Without the gate a replacement
        // pod, which starts with an empty zone directory, joins the Service as
        // soon as `named` listens and answers REFUSED until the zones arrive.
        readiness_gates: Some(zones_loaded_readiness_gates()),
        security_context: Some(PodSecurityContext {
            run_as_user: Some(BIND9_NONROOT_UID),
            run_as_group: Some(BIND9_NONROOT_UID),
            fs_group: Some(BIND9_NONROOT_UID),
            run_as_non_root: Some(true),
            // Pod-level RuntimeDefault seccomp profile so the pod satisfies Pod
            // Security Admission `restricted` (inherited by any container that
            // does not set its own).
            seccomp_profile: Some(runtime_default_seccomp()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Seccomp profile type Pod Security Admission `restricted` requires on
/// every container (`Localhost` is the only alternative).
const SECCOMP_RUNTIME_DEFAULT: &str = "RuntimeDefault";

/// The capability every container drops, which drops them all.
const CAPABILITY_ALL: &str = "ALL";

/// Image pull policy when the image configuration names none.
pub(super) const DEFAULT_IMAGE_PULL_POLICY: &str = "IfNotPresent";

/// `defaultMode` of the bindcar TLS Secret volume: owner read-only, so the
/// private key is unreadable by anything but the container's own user.
const BINDCAR_TLS_SECRET_MODE: i32 = 0o400;

/// The `RuntimeDefault` seccomp profile.
fn runtime_default_seccomp() -> SeccompProfile {
    SeccompProfile {
        type_: SECCOMP_RUNTIME_DEFAULT.to_string(),
        ..Default::default()
    }
}

/// The container security context every bindy container runs under: non-root
/// as [`BIND9_NONROOT_UID`], no privilege escalation, ALL capabilities dropped
/// and none added, and the `RuntimeDefault` seccomp profile, set on the
/// container as well as the pod. The strictest posture under Pod Security
/// Admission `restricted`.
///
/// # Arguments
///
/// * `read_only_root` - Set `readOnlyRootFilesystem: true`; when false the
///   field is left unset
///
/// # Returns
///
/// The `SecurityContext`.
pub(super) fn restricted_security_context(read_only_root: bool) -> SecurityContext {
    SecurityContext {
        run_as_non_root: Some(true),
        run_as_user: Some(BIND9_NONROOT_UID),
        run_as_group: Some(BIND9_NONROOT_UID),
        allow_privilege_escalation: Some(false),
        read_only_root_filesystem: read_only_root.then_some(true),
        capabilities: Some(Capabilities {
            drop: Some(vec![CAPABILITY_ALL.to_string()]),
            add: None,
        }),
        seccomp_profile: Some(runtime_default_seccomp()),
        ..Default::default()
    }
}

/// An environment variable with a literal value.
///
/// # Arguments
///
/// * `name` - Variable name
/// * `value` - Its value
pub(super) fn env(name: &str, value: impl Into<String>) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: Some(value.into()),
        ..Default::default()
    }
}

/// An environment variable read from a required key of a `Secret`.
///
/// # Arguments
///
/// * `name` - Variable name
/// * `secret` - Name of the `Secret`
/// * `key` - Key within the `Secret`
pub(super) fn secret_env(name: &str, secret: &str, key: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                name: secret.to_string(),
                key: key.to_string(),
                optional: Some(false),
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A pod volume backed by a `ConfigMap`.
///
/// # Arguments
///
/// * `name` - Volume name
/// * `configmap` - Name of the `ConfigMap`
fn configmap_volume(name: &str, configmap: &str) -> Volume {
    Volume {
        name: name.to_string(),
        config_map: Some(ConfigMapVolumeSource {
            name: configmap.to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// When a probe starts, how often it runs, how long it waits and how many
/// failures it tolerates.
pub(super) struct ProbeTiming {
    initial_delay_seconds: i32,
    period_seconds: i32,
    timeout_seconds: i32,
    failure_threshold: i32,
}

/// Timing of the `named` liveness probe.
const LIVENESS_TIMING: ProbeTiming = ProbeTiming {
    initial_delay_seconds: LIVENESS_INITIAL_DELAY_SECS,
    period_seconds: LIVENESS_PERIOD_SECS,
    timeout_seconds: LIVENESS_TIMEOUT_SECS,
    failure_threshold: LIVENESS_FAILURE_THRESHOLD,
};

/// Timing of every readiness probe (`named` and bindcar).
pub(super) const READINESS_TIMING: ProbeTiming = ProbeTiming {
    initial_delay_seconds: READINESS_INITIAL_DELAY_SECS,
    period_seconds: READINESS_PERIOD_SECS,
    timeout_seconds: READINESS_TIMEOUT_SECS,
    failure_threshold: READINESS_FAILURE_THRESHOLD,
};

/// A probe with `timing` applied to `handler`, a `Probe` that sets only its
/// check (`tcpSocket`, `httpGet`, ...).
///
/// # Arguments
///
/// * `timing` - The probe timing
/// * `handler` - The probe's check
pub(super) fn timed_probe(timing: &ProbeTiming, handler: Probe) -> Probe {
    Probe {
        initial_delay_seconds: Some(timing.initial_delay_seconds),
        period_seconds: Some(timing.period_seconds),
        timeout_seconds: Some(timing.timeout_seconds),
        failure_threshold: Some(timing.failure_threshold),
        ..handler
    }
}

/// A TCP connect check on `named`'s DNS port.
fn dns_tcp_probe() -> Probe {
    Probe {
        tcp_socket: Some(TCPSocketAction {
            port: IntOrString::Int(i32::from(DNS_CONTAINER_PORT)),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The readiness gates every BIND9 pod template carries: the zones-loaded
/// condition the operator sets once the pod's zones are loaded (ADR-0017).
///
/// # Returns
/// A one-element list with [`crate::constants::ZONES_LOADED_CONDITION_TYPE`].
#[must_use]
pub fn zones_loaded_readiness_gates() -> Vec<k8s_openapi::api::core::v1::PodReadinessGate> {
    vec![k8s_openapi::api::core::v1::PodReadinessGate {
        condition_type: crate::constants::ZONES_LOADED_CONDITION_TYPE.to_string(),
    }]
}

/// Mount a generated-or-custom BIND9 config file into the container.
///
/// Uses the user's custom `ConfigMap` volume (`custom_volume`) when the
/// corresponding reference is set, otherwise the default generated `config`
/// volume ([`VOLUME_CONFIG`]).
fn config_file_mount(
    custom_volume: &str,
    custom_ref: Option<&String>,
    mount_path: &str,
    sub_path: &str,
) -> VolumeMount {
    let name = if custom_ref.is_some() {
        custom_volume
    } else {
        VOLUME_CONFIG
    };
    VolumeMount {
        name: name.into(),
        mount_path: mount_path.into(),
        sub_path: Some(sub_path.into()),
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

    // named.conf and named.conf.options come from the user's custom ConfigMap
    // volume when referenced, otherwise from the default generated ConfigMap.
    mounts.push(config_file_mount(
        VOLUME_NAMED_CONF,
        config_map_refs.and_then(|refs| refs.named_conf.as_ref()),
        BIND_NAMED_CONF_PATH,
        NAMED_CONF_FILENAME,
    ));
    mounts.push(config_file_mount(
        VOLUME_NAMED_CONF_OPTIONS,
        config_map_refs.and_then(|refs| refs.named_conf_options.as_ref()),
        BIND_NAMED_CONF_OPTIONS_PATH,
        NAMED_CONF_OPTIONS_FILENAME,
    ));

    // The zones file is mounted only when the user provides a ConfigMap for
    // it - there is no generated default.
    if config_map_refs
        .and_then(|refs| refs.named_conf_zones.as_ref())
        .is_some()
    {
        mounts.push(VolumeMount {
            name: VOLUME_NAMED_CONF_ZONES.into(),
            mount_path: BIND_NAMED_CONF_ZONES_PATH.into(),
            sub_path: Some(NAMED_CONF_ZONES_FILENAME.into()),
            ..Default::default()
        });
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
/// The generated `config` volume is ALWAYS present regardless of custom refs:
/// it backs the unconditional `rndc.conf` mounts in both containers and the
/// generated `ConfigMap` always exists (it always contains at least `rndc.conf`).
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
    bindcar_tls: Option<&crate::crd::BindcarTlsConfig>,
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
        // Memory-backed writable scratch dir mounted at /tmp in the bindcar
        // sidecar (TMPDIR). Needed because the sidecar runs with a read-only
        // root filesystem under Pod Security Admission `restricted`.
        Volume {
            name: VOLUME_TMP.into(),
            empty_dir: Some(EmptyDirVolumeSource {
                medium: Some(EMPTY_DIR_MEDIUM_MEMORY.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];

    // The sidecar's TLS key pair (ADR-0004), owner read-only
    // (BINDCAR_TLS_SECRET_MODE).
    if let Some(tls) = bindcar_tls.filter(|t| t.is_enabled()) {
        if let Some(secret_name) = tls.secret_name.as_ref() {
            volumes.push(Volume {
                name: VOLUME_BINDCAR_TLS.into(),
                secret: Some(k8s_openapi::api::core::v1::SecretVolumeSource {
                    secret_name: Some(secret_name.clone()),
                    default_mode: Some(BINDCAR_TLS_SECRET_MODE),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
    }

    // Add ConfigMap volumes
    if let Some(refs) = config_map_refs {
        if let Some(configmap_name) = &refs.named_conf {
            volumes.push(configmap_volume(VOLUME_NAMED_CONF, configmap_name));
        }

        if let Some(configmap_name) = &refs.named_conf_options {
            volumes.push(configmap_volume(VOLUME_NAMED_CONF_OPTIONS, configmap_name));
        }

        if let Some(configmap_name) = &refs.named_conf_zones {
            volumes.push(configmap_volume(VOLUME_NAMED_CONF_ZONES, configmap_name));
        }
    }

    // ALWAYS add the generated config volume. The generated ConfigMap always
    // exists (it always carries at least rndc.conf, which is not overridable),
    // and rndc.conf is mounted from this volume unconditionally in both the
    // bind9 and bindcar containers. Omitting it when custom refs are set would
    // leave those mounts dangling and make the API server reject the Deployment.
    volumes.push(configmap_volume(VOLUME_CONFIG, configmap_name));

    // Append custom volumes from cluster/instance
    if let Some(custom_vols) = custom_volumes {
        volumes.extend(custom_vols.iter().cloned());
    }

    volumes
}
