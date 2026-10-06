// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! BIND9 Kubernetes resource builders
//!
//! This module provides functions to build Kubernetes resources (`Deployment`, `ConfigMap`, `Service`)
//! for BIND9 instances. All functions are pure and easily testable.

use crate::bind9_acl::parse_acl_list;
use crate::constants::{
    API_GROUP_VERSION, BIND9_MALLOC_CONF, BIND9_NONROOT_UID, BIND9_PRESTOP_DRAIN_SECS,
    BIND9_SERVICE_ACCOUNT, BIND9_TERMINATION_GRACE_PERIOD_SECS, CONTAINER_NAME_BIND9,
    CONTAINER_NAME_BINDCAR, DEFAULT_BIND9_VERSION, DNS_CONTAINER_PORT, DNS_PORT,
    KIND_BIND9_CLUSTER, KIND_BIND9_INSTANCE, LIVENESS_FAILURE_THRESHOLD,
    LIVENESS_INITIAL_DELAY_SECS, LIVENESS_PERIOD_SECS, LIVENESS_TIMEOUT_SECS,
    MAX_UNAVAILABLE_OPERANDS, READINESS_FAILURE_THRESHOLD, READINESS_INITIAL_DELAY_SECS,
    READINESS_PERIOD_SECS, READINESS_TIMEOUT_SECS, RNDC_PORT,
};
use crate::crd::{
    Bind9Cluster, Bind9Config, Bind9Instance, ConfigMapRefs, ImageConfig, ServerRole,
};
use crate::labels::{
    APP_NAME_BIND9, BINDY_CLUSTER_LABEL, BINDY_ROLE_LABEL, COMPONENT_DNS_CLUSTER,
    COMPONENT_DNS_SERVER, K8S_COMPONENT, K8S_INSTANCE, K8S_MANAGED_BY, K8S_NAME, K8S_PART_OF,
    MANAGED_BY_BIND9_CLUSTER, MANAGED_BY_BIND9_INSTANCE, PART_OF_BINDY, ROLE_PRIMARY,
    ROLE_SECONDARY,
};
use anyhow::Context;
use hornet_bind9::named_conf::{
    AddressMatchElement, AddressMatchList, ControlsBlock, DnssecKeyLifetime, DnssecKeyRole,
    DnssecPolicyKey, DnssecPolicyStmt, DnssecValidation, InetControl, ListenOn, LogCategory,
    LogChannel, LogDestination, LogSeverity, LoggingBlock, NamedConf, Nsec3Param, OptionsBlock,
    PrintTime, RateLimit, Statement,
};
use hornet_bind9::writer::WriteOptions;
use k8s_openapi::api::{
    apps::v1::{Deployment, DeploymentSpec},
    core::v1::{
        Capabilities, ConfigMap, Container, ContainerPort, EmptyDirVolumeSource, EnvVar,
        EnvVarSource, ExecAction, HTTPGetAction, Lifecycle, LifecycleHandler, PodSecurityContext,
        PodSpec, PodTemplateSpec, Probe, SeccompProfile, SecretKeySelector, SecurityContext,
        Service, ServiceAccount, ServicePort, ServiceSpec, TCPSocketAction, Volume, VolumeMount,
    },
    policy::v1::{PodDisruptionBudget, PodDisruptionBudgetSpec},
};
use k8s_openapi::apimachinery::pkg::{
    apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference},
    util::intstr::IntOrString,
};
use kube::ResourceExt;
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use tracing::{debug, warn};

// rndc.conf is the rndc client's file, not named.conf grammar, so it stays a
// template; named.conf and named.conf.options are written by hornet (ADR-0013).
const RNDC_CONF_TEMPLATE: &str = include_str!("../../../templates/rndc.conf.tmpl");

/// Name of the RNDC key `named` accepts on its control channel. The key file
/// ([`RNDC_KEY_FILENAME`] in [`BIND_KEYS_PATH`]) is mounted from the RNDC Secret.
const RNDC_KEY_NAME: &str = "bindy-operator";
/// File name of the RNDC key inside [`BIND_KEYS_PATH`].
const RNDC_KEY_FILENAME: &str = "rndc.key";

// Files `named` writes in its working directory (`BIND_CACHE_PATH`).
const NAMED_PID_FILENAME: &str = "named.pid";
const NAMED_SESSION_KEY_FILENAME: &str = "session.key";
const NAMED_DUMP_FILENAME: &str = "cache_dump.db";
const NAMED_STATISTICS_FILENAME: &str = "named_stats.txt";
const NAMED_MEMSTATISTICS_FILENAME: &str = "named_mem_stats.txt";

// Logging: every channel goes to stderr, where the kubelet collects it.
const LOG_CHANNEL_DEFAULT: &str = "default_stderr";
const LOG_CHANNEL_QUERIES: &str = "queries_stderr";
const LOG_CHANNEL_SECURITY: &str = "security_stderr";
/// Number of logging categories bindy routes.
const LOG_CATEGORY_COUNT: usize = 12;
/// Each logging category and the channel it is routed to.
const LOG_CATEGORIES: [(&str, &str); LOG_CATEGORY_COUNT] = [
    ("default", LOG_CHANNEL_DEFAULT),
    ("general", LOG_CHANNEL_DEFAULT),
    ("config", LOG_CHANNEL_DEFAULT),
    ("network", LOG_CHANNEL_DEFAULT),
    ("queries", LOG_CHANNEL_QUERIES),
    ("security", LOG_CHANNEL_SECURITY),
    ("dnssec", LOG_CHANNEL_SECURITY),
    ("xfer-in", LOG_CHANNEL_DEFAULT),
    ("xfer-out", LOG_CHANNEL_DEFAULT),
    ("notify", LOG_CHANNEL_DEFAULT),
    ("update", LOG_CHANNEL_DEFAULT),
    ("update-security", LOG_CHANNEL_SECURITY),
];

// Timing clauses of the DNSSEC policy bindy defines.
const DNSSEC_SIGNATURES_REFRESH: &str = "5d";
const DNSSEC_SIGNATURES_VALIDITY: &str = "30d";
const DNSSEC_SIGNATURES_VALIDITY_DNSKEY: &str = "30d";
/// Time for zone updates to reach all servers: 5 minutes, in seconds.
const DNSSEC_ZONE_PROPAGATION_DELAY: &str = "300";
/// Time for DS updates to reach the parent zone: 1 hour, in seconds.
const DNSSEC_PARENT_PROPAGATION_DELAY: &str = "3600";
/// Maximum TTL in a signed zone, which bounds key rollover timing: 24 hours,
/// in seconds.
const DNSSEC_MAX_ZONE_TTL: &str = "86400";

// BIND configuration file paths and mount points
const BIND_ZONES_PATH: &str = "/etc/bind/zones";
const BIND_CACHE_PATH: &str = "/var/cache/bind";
const BIND_KEYS_PATH: &str = "/etc/bind/keys";
const BIND_DNSSEC_KEYS_PATH: &str = "/var/cache/bind/keys";
/// Where the init container sees the user's DNSSEC key Secret (ADR-0012).
/// Mounted read-only, and only into that container: `named` never sees it.
const BIND_DNSSEC_KEYS_SOURCE_PATH: &str = "/etc/bind/dnssec-keys-source";
const BIND_NAMED_CONF_PATH: &str = "/etc/bind/named.conf";
const BIND_NAMED_CONF_OPTIONS_PATH: &str = "/etc/bind/named.conf.options";
const BIND_NAMED_CONF_ZONES_PATH: &str = "/etc/bind/named.conf.zones";
const BIND_RNDC_CONF_PATH: &str = "/etc/bind/rndc.conf";

/// bindcar's unauthenticated readiness endpoint. It reports whether the zone
/// directory is usable and rndc answers; it does not require any zone.
const BINDCAR_READY_PATH: &str = "/api/v1/ready";

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
/// The user's DNSSEC key Secret, the source the init container copies from.
const VOLUME_DNSSEC_KEYS_SOURCE: &str = "dnssec-keys-source";
/// Mode of the Secret's files: readable by the bind group (the pod's
/// `fsGroup`), which the init container runs as. Secret volumes are owned by
/// root, so an owner-only mode would leave them unreadable.
const DNSSEC_KEYS_SOURCE_MODE: i32 = 0o440;
/// Init container that copies Secret-supplied DNSSEC keys into the writable
/// key directory (ADR-0012).
const CONTAINER_NAME_DNSSEC_KEYS_INIT: &str = "dnssec-keys-init";
/// `emptyDir` medium for a tmpfs volume.
const EMPTY_DIR_MEDIUM_MEMORY: &str = "Memory";
/// BIND's keyword for a key that never rolls; the only lifetime shared keys
/// may have (ADR-0012).
const DNSSEC_LIFETIME_UNLIMITED: &str = "unlimited";

/// Shell script the [`CONTAINER_NAME_DNSSEC_KEYS_INIT`] container runs as
/// `sh -c SCRIPT sh SRC DST`: copy every DNSSEC key file in `SRC` (the Secret)
/// to `DST` (the key directory), owner-only.
///
/// Secret data keys cannot contain `+`, so the Secret names a key
/// `K<zone>._<alg>_<id>.<ext>` and the copy restores BIND's
/// `K<zone>.+<alg>+<id>.<ext>`. The match is anchored at the end, so an
/// underscore inside the zone name survives. Other entries are skipped. No
/// key at all fails the container: `named` would otherwise generate keys of
/// its own, different in every pod.
pub(crate) const DNSSEC_KEYS_INIT_SCRIPT: &str = r#"set -eu
src="$1"; dst="$2"; n=0
for f in "$src"/*; do
  [ -f "$f" ] || continue
  base="${f##*/}"
  name="$(printf '%s' "$base" | sed -nE 's/^(K.+)_([0-9]{3})_([0-9]{5})\.(key|private|state)$/\1+\2+\3.\4/p')"
  if [ -z "$name" ]; then
    echo "dnssec-keys-init: skipping $base (not K<zone>._<alg>_<id>.key|private|state)" >&2
    continue
  fi
  cp -L "$f" "$dst/$name"
  chmod 0600 "$dst/$name"
  echo "dnssec-keys-init: $base -> $name"
  n=$((n + 1))
done
if [ "$n" -eq 0 ]; then
  echo "dnssec-keys-init: no DNSSEC key files in $src" >&2
  exit 1
fi
"#;
/// Memory-backed writable scratch volume for the bindcar sidecar (`TMPDIR`).
/// Required because the sidecar runs with `readOnlyRootFilesystem: true` under
/// Pod Security Admission `restricted` yet must write a `0600` TSIG key file
/// for `nsupdate -k`.
const VOLUME_TMP: &str = "tmp";

// named.conf.options directive names for listen addresses
const LISTEN_ON_DIRECTIVE: &str = "listen-on";
const LISTEN_ON_V6_DIRECTIVE: &str = "listen-on-v6";

// Default DNSSEC signing parameters
/// Name of the policy bindy defines when the cluster names none. Not
/// `"default"`: BIND reserves that for a built-in policy (see
/// [`BIND_BUILTIN_DNSSEC_POLICIES`]).
const DEFAULT_DNSSEC_POLICY_NAME: &str = "bindy";

/// Policy names BIND reserves for its built-in policies. A
/// `dnssec-policy "<name>" { ... }` block with one of these makes named refuse
/// to load its configuration ("dnssec-policy name may not be 'insecure',
/// 'none', or 'default'"), so a definition may never use them.
const BIND_BUILTIN_DNSSEC_POLICIES: [&str; 3] = ["default", "insecure", "none"];
const DEFAULT_DNSSEC_ALGORITHM: &str = "ECDSAP256SHA256";
const DEFAULT_KSK_LIFETIME: &str = "unlimited";
const DEFAULT_ZSK_LIFETIME: &str = "unlimited";
const DEFAULT_NSEC3_SALT_LENGTH: u8 = 16;

/// Max length of a DNSSEC policy NAME, matching the CRD schema pattern
/// `^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$` and ValidatingAdmissionPolicy 09.
const MAX_DNSSEC_POLICY_NAME_LEN: usize = 63;

/// Max length of a DNSSEC signing TOKEN (algorithm, KSK/ZSK lifetime), matching
/// the CRD schema pattern `^[A-Za-z0-9]{1,32}$` and ValidatingAdmissionPolicy 09.
const MAX_DNSSEC_TOKEN_LEN: usize = 32;

/// Validate a DNSSEC policy name against `^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$`,
/// and refuse BIND's built-in policy names (`default`, `insecure`, `none`).
///
/// This is the RUNTIME arm of a three-layer defence. The CRD schema rejects a
/// bad value at the API server and ValidatingAdmissionPolicy 09 rejects it at
/// admission — but a cluster running a stale CRD, or one that never installed
/// the policy suite, would otherwise interpolate the value straight into the
/// `dnssec-policy { ... }` block of `named.conf` (audit finding P2-5). All three
/// layers deliberately share one grammar so they cannot disagree.
///
/// # Errors
/// Returns an error naming the field when `name` does not match the grammar.
fn validate_dnssec_policy_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.len() > MAX_DNSSEC_POLICY_NAME_LEN {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: must be 1-{MAX_DNSSEC_POLICY_NAME_LEN} characters"
        );
    }

    let mut chars = name.chars();
    // Guard clause: the first character may not be '-' or '_'.
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: must start with an ASCII letter or digit"
        );
    }
    if let Some(bad) = chars.find(|c| !c.is_ascii_alphanumeric() && *c != '-' && *c != '_') {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: illegal character {bad:?} \
             (allowed: ASCII letters, digits, '-', '_')"
        );
    }
    if BIND_BUILTIN_DNSSEC_POLICIES
        .iter()
        .any(|builtin| name.eq_ignore_ascii_case(builtin))
    {
        anyhow::bail!(
            "invalid dnssec policy name {name:?}: it is a BIND built-in policy, which \
             named refuses to redefine; choose another name (e.g. {DEFAULT_DNSSEC_POLICY_NAME:?})"
        );
    }

    Ok(())
}

/// Validate a DNSSEC signing token (algorithm, KSK/ZSK lifetime) against
/// `^[A-Za-z0-9]{1,32}$`.
///
/// These are interpolated UNQUOTED into `named.conf`, so the grammar is stricter
/// than for policy names: alphanumeric only. `field` names the offending input in
/// the error so an operator can find it without reading the template.
///
/// # Errors
/// Returns an error naming `field` when `value` does not match the grammar.
fn validate_dnssec_token(field: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty() || value.len() > MAX_DNSSEC_TOKEN_LEN {
        anyhow::bail!(
            "invalid {field} {value:?}: must be 1-{MAX_DNSSEC_TOKEN_LEN} alphanumeric characters"
        );
    }
    if let Some(bad) = value.chars().find(|c| !c.is_ascii_alphanumeric()) {
        anyhow::bail!(
            "invalid {field} {value:?}: illegal character {bad:?} (allowed: ASCII letters and digits)"
        );
    }

    Ok(())
}

/// The `dnssec-policy` statement bindy defines, from cluster or instance config
///
/// Checks both instance and global configuration for DNSSEC signing settings.
/// Instance config takes precedence over global config.
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
///
/// # Returns
///
/// The policy statement, or `None` if signing is not enabled anywhere (which
/// is not an error).
///
/// # Errors
///
/// Returns an error if `policy`, `algorithm`, `kskLifetime` or `zskLifetime`
/// fails the runtime whitelist (see [`validate_dnssec_policy_name`] and
/// [`validate_dnssec_token`], audit finding P2-5), or if a lifetime is not a
/// duration BIND accepts (see [`policy_key_lifetime`]).
pub(crate) fn dnssec_policy_statement(
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
) -> anyhow::Result<Option<DnssecPolicyStmt>> {
    // Resolve the signing config with the same precedence/fallback semantics
    // as get_dnssec_signing_config: instance config wins when it enables
    // signing, otherwise fall back to the global config.
    let Some(signing) = get_dnssec_signing_config(global_config, instance_config) else {
        return Ok(None);
    };

    // Extract policy parameters with defaults
    let policy_name = signing
        .policy
        .as_deref()
        .unwrap_or(DEFAULT_DNSSEC_POLICY_NAME);
    let algorithm = signing
        .algorithm
        .as_deref()
        .unwrap_or(DEFAULT_DNSSEC_ALGORITHM);
    let ksk_lifetime = signing
        .ksk_lifetime
        .as_deref()
        .unwrap_or(DEFAULT_KSK_LIFETIME);
    let zsk_lifetime = signing
        .zsk_lifetime
        .as_deref()
        .unwrap_or(DEFAULT_ZSK_LIFETIME);

    // P2-5: the algorithm and lifetimes are written unquoted. hornet's writer
    // emits them as given, so they are still held to the CRD's grammar here.
    validate_dnssec_policy_name(policy_name)?;
    validate_dnssec_token("dnssec algorithm", algorithm)?;
    validate_dnssec_token("dnssec ksk lifetime", ksk_lifetime)?;
    validate_dnssec_token("dnssec zsk lifetime", zsk_lifetime)?;
    if dnssec_key_secret(signing).is_some() {
        require_unlimited_lifetime("kskLifetime", ksk_lifetime)?;
        require_unlimited_lifetime("zskLifetime", zsk_lifetime)?;
    }

    let key = |role, lifetime| DnssecPolicyKey {
        role,
        storage: None,
        lifetime,
        algorithm: algorithm.to_string(),
        tag_range: None,
        bits: None,
    };
    let keys = vec![
        key(
            DnssecKeyRole::Ksk,
            policy_key_lifetime("kskLifetime", ksk_lifetime)?,
        ),
        key(
            DnssecKeyRole::Zsk,
            policy_key_lifetime("zskLifetime", zsk_lifetime)?,
        ),
    ];

    // NSEC is selected by having no `nsec3param` clause.
    let nsec3param = signing.nsec3.unwrap_or(false).then(|| Nsec3Param {
        iterations: Some(signing.nsec3_iterations.unwrap_or(0)),
        optout: Some(false),
        salt_length: Some(u32::from(DEFAULT_NSEC3_SALT_LENGTH)),
    });

    Ok(Some(DnssecPolicyStmt {
        name: policy_name.to_string(),
        keys: Some(keys),
        nsec3param,
        signatures_refresh: Some(DNSSEC_SIGNATURES_REFRESH.to_string()),
        signatures_validity: Some(DNSSEC_SIGNATURES_VALIDITY.to_string()),
        signatures_validity_dnskey: Some(DNSSEC_SIGNATURES_VALIDITY_DNSKEY.to_string()),
        zone_propagation_delay: Some(DNSSEC_ZONE_PROPAGATION_DELAY.to_string()),
        parent_propagation_delay: Some(DNSSEC_PARENT_PROPAGATION_DELAY.to_string()),
        max_zone_ttl: Some(DNSSEC_MAX_ZONE_TTL.to_string()),
        ..Default::default()
    }))
}

/// A policy key lifetime: `unlimited`, or a duration BIND accepts.
///
/// BIND has no `y` TTL unit, so `1y` makes `named` refuse the whole
/// configuration; one year is `P1Y`, `365d` or `8760h`.
///
/// # Errors
/// Returns an error naming `field` when `value` is neither `unlimited` nor a
/// BIND duration.
fn policy_key_lifetime(field: &str, value: &str) -> anyhow::Result<DnssecKeyLifetime> {
    if value.eq_ignore_ascii_case(DNSSEC_LIFETIME_UNLIMITED) {
        return Ok(DnssecKeyLifetime::Unlimited);
    }
    if !hornet_bind9::named_conf::is_duration(value) {
        anyhow::bail!(
            "invalid dnssec {field} {value:?}: not a duration BIND accepts; use a TTL value \
             such as 365d or 8760h, an ISO 8601 duration such as P1Y, or \
             {DNSSEC_LIFETIME_UNLIMITED:?}"
        );
    }
    Ok(DnssecKeyLifetime::Duration(value.to_string()))
}

/// Refuse a finite key lifetime for keys supplied from a Secret (ADR-0012).
///
/// The keys are shared by every primary; a finite lifetime makes each `named`
/// roll a successor of its own, and the pods diverge. The CRD rejects this at
/// admission; this is the runtime arm for a cluster with an older CRD.
///
/// # Errors
/// Returns an error naming `field` when `value` is not `unlimited`.
fn require_unlimited_lifetime(field: &str, value: &str) -> anyhow::Result<()> {
    if value.eq_ignore_ascii_case(DNSSEC_LIFETIME_UNLIMITED) {
        return Ok(());
    }
    anyhow::bail!(
        "invalid dnssec {field} {value:?}: keys from keysFrom.secretRef are shared by every \
         primary and must not roll on their own; use {DNSSEC_LIFETIME_UNLIMITED:?} (or leave \
         it unset) and rotate by updating the Secret"
    )
}

/// The Secret a signing config takes its keys from, if any (ADR-0012).
fn dnssec_key_secret(
    signing: &crate::crd::DNSSECSigningConfig,
) -> Option<&crate::crd::SecretReference> {
    signing
        .keys_from
        .as_ref()
        .and_then(|k| k.secret_ref.as_ref())
}

/// The `dnssec-policy` a zone is created or updated with.
///
/// An explicit `spec.dnssecPolicy` on the zone always wins, including BIND's
/// built-in `insecure` and `none`, which a zone names to unsign itself.
/// Otherwise a zone inherits the signing policy of the instance that serves
/// it (instance config over cluster `global`, as `dnssec_policy_statement`
/// renders it): its `policy`, or `DEFAULT_DNSSEC_POLICY_NAME` when unnamed.
/// With signing off everywhere, the zone is unsigned.
///
/// # Arguments
///
/// * `zone_policy` - The zone's `spec.dnssecPolicy`, if set
/// * `global_config` - The cluster's (or cluster provider's) `global` config
/// * `instance_config` - The serving instance's own `spec.config`
///
/// # Returns
///
/// The policy name to configure on the zone, or `None` for an unsigned zone.
pub fn resolve_zone_dnssec_policy(
    zone_policy: Option<&str>,
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
) -> Option<String> {
    if let Some(explicit) = zone_policy {
        return Some(explicit.to_string());
    }
    let signing = get_dnssec_signing_config(global_config, instance_config)?;
    Some(
        signing
            .policy
            .clone()
            .unwrap_or_else(|| DEFAULT_DNSSEC_POLICY_NAME.to_string()),
    )
}

/// The `key-directory` for `named.conf.options`: the DNSSEC key mount
/// ([`BIND_DNSSEC_KEYS_PATH`]) when signing is enabled, else none.
///
/// Without it BIND keeps `dnssec-policy` keys in its working directory, a
/// scratch volume, so keys from `keysFrom` are never read and generated keys
/// never reach the volume that is meant to keep them.
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
fn key_directory(
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
) -> Option<String> {
    get_dnssec_signing_config(global_config, instance_config)
        .map(|_| BIND_DNSSEC_KEYS_PATH.to_string())
}

/// Check if DNSSEC signing is enabled in either instance or global config
///
/// Instance config takes precedence over global config.
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
///
/// # Returns
///
/// `true` if DNSSEC signing is enabled, `false` otherwise
#[allow(dead_code)]
pub(crate) fn is_dnssec_signing_enabled(
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
) -> bool {
    // Check instance config first, then fall back to global config
    let dnssec_config = if let Some(instance) = instance_config {
        instance.dnssec.as_ref().and_then(|d| d.signing.as_ref())
    } else {
        global_config.and_then(|g| g.dnssec.as_ref().and_then(|d| d.signing.as_ref()))
    };

    dnssec_config.is_some_and(|signing| signing.enabled)
}

/// Get DNSSEC signing configuration from either instance or global config
///
/// Instance config takes precedence over global config.
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
///
/// # Returns
///
/// Reference to `DNSSECSigningConfig` if signing is enabled, `None` otherwise
pub fn get_dnssec_signing_config<'a>(
    global_config: Option<&'a crate::crd::Bind9Config>,
    instance_config: Option<&'a crate::crd::Bind9Config>,
) -> Option<&'a crate::crd::DNSSECSigningConfig> {
    // Check instance config first, then fall back to global config
    if let Some(instance) = instance_config {
        if let Some(config) = instance.dnssec.as_ref().and_then(|d| d.signing.as_ref()) {
            if config.enabled {
                return Some(config);
            }
        }
    }

    global_config
        .and_then(|g| g.dnssec.as_ref().and_then(|d| d.signing.as_ref()))
        .filter(|config| config.enabled)
}

/// Build DNSSEC key volumes and volume mounts based on configuration
///
/// Creates appropriate volumes for DNSSEC keys based on the key source configuration:
/// - User-supplied Secret: the Secret as a source volume plus a writable
///   `emptyDir` key directory, which is the only one `named` mounts; see
///   [`build_dnssec_keys_init_container`] (ADR-0012)
/// - Auto-generated: Use `emptyDir` for BIND9 to generate keys
/// - Persistent storage: Use `PersistentVolumeClaim` for keys
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
///
/// # Returns
///
/// Tuple of (volumes, `volume_mounts`) to add to the pod spec
pub(crate) fn build_dnssec_key_volumes(
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
) -> (Vec<Volume>, Vec<VolumeMount>) {
    use k8s_openapi::api::core::v1::{
        EmptyDirVolumeSource, SecretVolumeSource, Volume, VolumeMount,
    };

    let Some(signing_config) = get_dnssec_signing_config(global_config, instance_config) else {
        return (vec![], vec![]);
    };

    let mut volumes = Vec::new();
    let mut volume_mounts = Vec::new();

    // Determine key source and create appropriate volume
    match &signing_config.keys_from {
        // Option 1: User-supplied keys from Secret
        // Secret volumes are always read-only and named must write `.state`
        // files beside its keys, so the Secret is only a source: the init
        // container (`build_dnssec_keys_init_container`) copies it into a
        // writable emptyDir, which is all named mounts (ADR-0012).
        Some(crate::crd::DNSSECKeySource {
            secret_ref: Some(secret),
            ..
        }) => {
            // Memory-backed, like the Secret volume itself: the private key
            // copies never reach the node's disk.
            volumes.push(Volume {
                name: VOLUME_DNSSEC_KEYS.to_string(),
                empty_dir: Some(EmptyDirVolumeSource {
                    medium: Some(EMPTY_DIR_MEDIUM_MEMORY.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            });
            volumes.push(Volume {
                name: VOLUME_DNSSEC_KEYS_SOURCE.to_string(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(secret.name.clone()),
                    default_mode: Some(DNSSEC_KEYS_SOURCE_MODE),
                    ..Default::default()
                }),
                ..Default::default()
            });

            volume_mounts.push(VolumeMount {
                name: VOLUME_DNSSEC_KEYS.to_string(),
                mount_path: BIND_DNSSEC_KEYS_PATH.to_string(),
                ..Default::default()
            });

            debug!(
                secret_name = %secret.name,
                "DNSSEC keys from Secret, copied into the key directory by an init container"
            );
        }

        // Option 2: Auto-generated keys (emptyDir + Secret backup)
        // This is also the default if no keys_from is specified
        None
        | Some(crate::crd::DNSSECKeySource {
            secret_ref: None,
            persistent_volume: None,
        }) => {
            if signing_config.auto_generate.unwrap_or(true) {
                volumes.push(Volume {
                    name: VOLUME_DNSSEC_KEYS.to_string(),
                    empty_dir: Some(EmptyDirVolumeSource::default()),
                    ..Default::default()
                });

                volume_mounts.push(VolumeMount {
                    name: VOLUME_DNSSEC_KEYS.to_string(),
                    mount_path: BIND_DNSSEC_KEYS_PATH.to_string(),
                    ..Default::default()
                });

                debug!("DNSSEC keys will be auto-generated by BIND9 in emptyDir");

                if signing_config.export_to_secret.unwrap_or(true) {
                    debug!("Auto-generated keys will be exported to Secret for backup/restore");
                }
            }
        }

        // Option 3: Persistent storage (not implemented yet - requires StatefulSet)
        Some(crate::crd::DNSSECKeySource {
            persistent_volume: Some(_pvc),
            ..
        }) => {
            warn!("Persistent storage for DNSSEC keys is not yet implemented - using emptyDir");
            volumes.push(Volume {
                name: VOLUME_DNSSEC_KEYS.to_string(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
                ..Default::default()
            });

            volume_mounts.push(VolumeMount {
                name: VOLUME_DNSSEC_KEYS.to_string(),
                mount_path: BIND_DNSSEC_KEYS_PATH.to_string(),
                ..Default::default()
            });
        }
    }

    (volumes, volume_mounts)
}

/// The init container that copies Secret-supplied DNSSEC keys into the
/// writable key directory before `named` starts (ADR-0012).
///
/// It runs `named`'s own image, which has `sh`, `sed` and `cp`, under the same
/// restricted security context plus a read-only root filesystem: its only
/// write is to the key directory.
///
/// # Arguments
///
/// * `global_config` - Optional global cluster configuration
/// * `instance_config` - Optional instance-specific configuration
/// * `image` - The BIND9 image `named` runs
/// * `image_pull_policy` - That image's pull policy
///
/// # Returns
///
/// The container when signing takes its keys from `keysFrom.secretRef`,
/// otherwise `None`.
pub(crate) fn build_dnssec_keys_init_container(
    global_config: Option<&crate::crd::Bind9Config>,
    instance_config: Option<&crate::crd::Bind9Config>,
    image: &str,
    image_pull_policy: &str,
) -> Option<Container> {
    let signing = get_dnssec_signing_config(global_config, instance_config)?;
    dnssec_key_secret(signing)?;

    Some(Container {
        name: CONTAINER_NAME_DNSSEC_KEYS_INIT.into(),
        image: Some(image.into()),
        image_pull_policy: Some(image_pull_policy.into()),
        command: Some(vec!["sh".into()]),
        args: Some(vec![
            "-c".into(),
            DNSSEC_KEYS_INIT_SCRIPT.into(),
            "sh".into(),
            BIND_DNSSEC_KEYS_SOURCE_PATH.into(),
            BIND_DNSSEC_KEYS_PATH.into(),
        ]),
        volume_mounts: Some(vec![
            VolumeMount {
                name: VOLUME_DNSSEC_KEYS_SOURCE.into(),
                mount_path: BIND_DNSSEC_KEYS_SOURCE_PATH.into(),
                read_only: Some(true),
                ..Default::default()
            },
            VolumeMount {
                name: VOLUME_DNSSEC_KEYS.into(),
                mount_path: BIND_DNSSEC_KEYS_PATH.into(),
                ..Default::default()
            },
        ]),
        security_context: Some(SecurityContext {
            run_as_non_root: Some(true),
            run_as_user: Some(BIND9_NONROOT_UID),
            run_as_group: Some(BIND9_NONROOT_UID),
            allow_privilege_escalation: Some(false),
            read_only_root_filesystem: Some(true),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                add: None,
            }),
            seccomp_profile: Some(SeccompProfile {
                type_: "RuntimeDefault".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

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
    use crate::labels::{BINDY_CLUSTER_LABEL, BINDY_ROLE_LABEL, ROLE_PRIMARY, ROLE_SECONDARY};

    let mut labels = build_labels_from_instance(instance_name, instance);

    if !instance.spec.cluster_ref.is_empty() {
        labels.insert(
            BINDY_CLUSTER_LABEL.to_string(),
            instance.spec.cluster_ref.clone(),
        );
    }

    labels
        .entry(BINDY_ROLE_LABEL.to_string())
        .or_insert_with(|| {
            match instance.spec.role {
                crate::crd::ServerRole::Primary => ROLE_PRIMARY,
                crate::crd::ServerRole::Secondary => ROLE_SECONDARY,
            }
            .to_string()
        });

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

/// Resolves the Bindcar sidecar configuration for an instance by merging the
/// instance, cluster and cluster-provider settings field by field.
///
/// Precedence is instance > cluster `global` > `ClusterBind9Provider` `global`,
/// applied **per field** rather than to the block as a whole. Picking the first
/// non-`None` block instead — which is what this used to do — meant a single
/// instance-level field silently discarded everything the cluster had
/// configured: setting `logLevel: debug` on one instance dropped the cluster's
/// pinned sidecar image, its port, its resources and every environment variable
/// with it. The CRD documents this field as "inherited by all instances unless
/// overridden", and per-field is what that reads as.
///
/// `envVars` merge by variable name, so an instance can override or add a single
/// variable without restating the cluster's whole list. `resources` and
/// `serviceSpec` are taken whole from the most specific level that sets them:
/// they are Kubernetes objects with their own internal defaulting, and splicing
/// them together field-wise would produce combinations nobody wrote.
///
/// Note this is about **operator-facing** precedence between two
/// operator-trusted levels. It does not address the separate concern that
/// user-supplied `envVars` can shadow operator-managed ones on the sidecar
/// (`.github/community/23-bindcar-migration-v0-7-2.md` §14), which needs a
/// reserved-name filter and an admission policy.
///
/// # Arguments
///
/// * `instance` - The `Bind9Instance` being reconciled
/// * `cluster` - The owning `Bind9Cluster`, if any
/// * `cluster_provider` - The owning `ClusterBind9Provider`, if any
///
/// # Returns
///
/// The merged configuration, or `None` when no level configures the sidecar.
#[must_use]
pub fn resolve_bindcar_config(
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
) -> Option<crate::crd::BindcarConfig> {
    let instance_cfg = instance.spec.bindcar_config.as_ref();
    let cluster_cfg = cluster.and_then(|c| {
        c.spec
            .common
            .global
            .as_ref()
            .and_then(|g| g.bindcar_config.as_ref())
    });
    let provider_cfg = cluster_provider.and_then(|cp| {
        cp.spec
            .common
            .global
            .as_ref()
            .and_then(|g| g.bindcar_config.as_ref())
    });

    // Most specific first; every field below takes the first level that sets it.
    let levels = [instance_cfg, cluster_cfg, provider_cfg];
    if levels.iter().all(Option::is_none) {
        return None;
    }

    let pick_string = |get: fn(&crate::crd::BindcarConfig) -> Option<&String>| {
        levels.iter().flatten().find_map(|cfg| get(cfg).cloned())
    };

    // Least specific first, so more specific names overwrite inherited ones.
    let mut merged_env: BTreeMap<String, k8s_openapi::api::core::v1::EnvVar> = BTreeMap::new();
    for cfg in levels.iter().flatten().rev() {
        if let Some(vars) = cfg.env_vars.as_ref() {
            for var in vars {
                merged_env.insert(var.name.clone(), var.clone());
            }
        }
    }

    Some(crate::crd::BindcarConfig {
        image: pick_string(|c| c.image.as_ref()),
        image_pull_policy: pick_string(|c| c.image_pull_policy.as_ref()),
        log_level: pick_string(|c| c.log_level.as_ref()),
        port: levels.iter().flatten().find_map(|c| c.port),
        resources: levels
            .iter()
            .flatten()
            .find_map(|c| c.resources.as_ref().cloned()),
        service_spec: levels
            .iter()
            .flatten()
            .find_map(|c| c.service_spec.as_ref().cloned()),
        // TLS is taken whole from the most specific level that sets it rather
        // than merged field-by-field: a half-inherited trust configuration (say
        // an instance's secret with a provider's CA bundle) is a footgun, and
        // the fields only make sense together.
        tls: levels
            .iter()
            .flatten()
            .find_map(|c| c.tls.as_ref().cloned()),
        env_vars: if merged_env.is_empty() {
            None
        } else {
            Some(merged_env.into_values().collect())
        },
    })
}

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
#[must_use]
pub fn build_pod_disruption_budget(
    cluster_name: &str,
    namespace: &str,
    role: ServerRole,
    cluster: Option<&Bind9Cluster>,
) -> PodDisruptionBudget {
    let role_str = match role {
        ServerRole::Primary => ROLE_PRIMARY,
        ServerRole::Secondary => ROLE_SECONDARY,
    };

    // Matches the labels build_labels_from_instance puts on the operand Pods.
    let mut match_labels = BTreeMap::new();
    match_labels.insert("app".to_string(), APP_NAME_BIND9.to_string());
    match_labels.insert(BINDY_CLUSTER_LABEL.to_string(), cluster_name.to_string());
    match_labels.insert(BINDY_ROLE_LABEL.to_string(), role_str.to_string());

    let owner_references = cluster.map(|c| {
        vec![OwnerReference {
            api_version: API_GROUP_VERSION.to_string(),
            kind: KIND_BIND9_CLUSTER.to_string(),
            name: c.name_any(),
            uid: c.metadata.uid.clone().unwrap_or_default(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }]
    });

    PodDisruptionBudget {
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
    }
}

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
/// created — even when both `namedConf` and `namedConfOptions` refs are set —
/// so the pod's `config` volume always has a backing `ConfigMap`.
///
/// # Arguments
///
/// * `name` - Name for the `ConfigMap` (typically `{instance-name}-config`)
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
/// if the rendered configuration does not parse (ADR-0013).
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

    // Generate configuration files not overridden by custom ConfigMap refs
    let mut data = BTreeMap::new();
    let labels = build_labels_from_instance(name, instance);

    // Build named.conf (unless the user supplies it via namedConf ref)
    if !named_conf_overridden {
        let named_conf = build_named_conf(instance, cluster);
        data.insert(NAMED_CONF_FILENAME.into(), named_conf);
    }

    // Build named.conf.options (unless the user supplies it via namedConfOptions ref);
    // validates ACL entries before templating
    if !options_overridden {
        let options_conf = build_options_conf(instance, cluster, role_allow_transfer)?;
        data.insert(NAMED_CONF_OPTIONS_FILENAME.into(), options_conf);
    }

    // Build rndc.conf (references key file mounted from Secret). This file is
    // never overridable, so the generated ConfigMap always exists.
    data.insert(RNDC_CONF_FILENAME.into(), RNDC_CONF_TEMPLATE.to_string());

    // Note: We do NOT auto-generate named.conf.zones anymore.
    // Users must explicitly provide a namedConfZones ConfigMap if they want zones support.

    let owner_refs = build_owner_references(instance);

    // ADR-0013: never publish a configuration `named` cannot parse. A broken
    // render fails here, so the ConfigMap is not written and the pods keep
    // the last configuration that was.
    for warning in crate::config_check::check_named_conf_files(&data)? {
        debug!("{warning}");
    }

    Ok(ConfigMap {
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

    // Generate default configuration from cluster spec
    let mut data = BTreeMap::new();
    let labels = build_cluster_labels(cluster_name);

    // Build named.conf from cluster
    let named_conf = build_cluster_named_conf(cluster);
    data.insert(NAMED_CONF_FILENAME.into(), named_conf);

    // Build named.conf.options from cluster.spec.common.global
    let options_conf = build_cluster_options_conf(cluster)?;
    data.insert(NAMED_CONF_OPTIONS_FILENAME.into(), options_conf);

    // Build rndc.conf (references key file mounted from Secret)
    data.insert(RNDC_CONF_FILENAME.into(), RNDC_CONF_TEMPLATE.to_string());

    // ADR-0013: never publish a configuration `named` cannot parse. A broken
    // render fails here, so the ConfigMap is not written and the pods keep
    // the last configuration that was.
    for warning in crate::config_check::check_named_conf_files(&data)? {
        debug!("{warning}");
    }

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

/// Write statements as BIND9 configuration text with hornet's writer
/// (ADR-0013 stage 3): every value is quoted or escaped for its position by
/// the writer, not by each caller.
fn write_conf(statements: Vec<Statement>) -> String {
    hornet_bind9::write_named_conf(&NamedConf { statements }, &WriteOptions::default())
}

/// Build the main named.conf for an instance
///
/// Includes `named.conf.options`, the zones file when the user provides a
/// `namedConfZones` `ConfigMap`, and the RNDC key; then the control channel
/// and logging.
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
    let config_map_refs = instance
        .spec
        .config_map_refs
        .as_ref()
        .or_else(|| cluster.and_then(|c| c.spec.common.config_map_refs.as_ref()));
    render_named_conf(includes_zones_file(config_map_refs))
}

/// Build the main named.conf for a cluster
///
/// # Arguments
///
/// * `cluster` - `Bind9Cluster` spec (checked for config refs)
///
/// # Returns
///
/// A string containing the complete named.conf configuration
fn build_cluster_named_conf(cluster: &Bind9Cluster) -> String {
    render_named_conf(includes_zones_file(
        cluster.spec.common.config_map_refs.as_ref(),
    ))
}

/// Whether the user supplies a `namedConfZones` `ConfigMap` to include.
fn includes_zones_file(config_map_refs: Option<&ConfigMapRefs>) -> bool {
    config_map_refs.is_some_and(|refs| refs.named_conf_zones.is_some())
}

/// Render named.conf.
///
/// # Arguments
///
/// * `include_zones` - Include the user-provided zones file
fn render_named_conf(include_zones: bool) -> String {
    let mut statements = vec![Statement::Include(BIND_NAMED_CONF_OPTIONS_PATH.to_string())];
    if include_zones {
        statements.push(Statement::Include(BIND_NAMED_CONF_ZONES_PATH.to_string()));
    }
    statements.push(Statement::Include(format!(
        "{BIND_KEYS_PATH}/{RNDC_KEY_FILENAME}"
    )));
    statements.push(Statement::Controls(rndc_controls()));
    statements.push(Statement::Logging(logging()));
    write_conf(statements)
}

/// The RNDC control channel: localhost only, with the operator's key. The
/// bindcar sidecar handles access from outside the pod.
fn rndc_controls() -> ControlsBlock {
    ControlsBlock {
        inet: vec![InetControl {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: RNDC_PORT,
            allow: vec![AddressMatchElement::Localhost],
            keys: vec![RNDC_KEY_NAME.to_string()],
            read_only: None,
        }],
        unix: Vec::new(),
    }
}

/// Logging: three stderr channels, ISO 8601 timestamps, and the
/// [`LOG_CATEGORIES`] routing.
fn logging() -> LoggingBlock {
    let channel = |name: &str, severity| LogChannel {
        name: name.to_string(),
        destination: LogDestination::Stderr,
        severity: Some(severity),
        print_time: Some(PrintTime::Iso8601),
        print_severity: Some(true),
        print_category: Some(true),
        buffered: None,
    };
    LoggingBlock {
        channels: vec![
            channel(LOG_CHANNEL_DEFAULT, LogSeverity::Info),
            channel(LOG_CHANNEL_QUERIES, LogSeverity::Info),
            channel(LOG_CHANNEL_SECURITY, LogSeverity::Warning),
        ],
        categories: LOG_CATEGORIES
            .iter()
            .map(|(name, channel)| LogCategory {
                name: (*name).to_string(),
                channels: vec![(*channel).to_string()],
            })
            .collect(),
    }
}

/// The `options` fields every rendered configuration has: `named`'s working
/// files under [`BIND_CACHE_PATH`], and `allow-new-zones` for bindcar's
/// `rndc addzone`.
fn base_options() -> OptionsBlock {
    OptionsBlock {
        directory: Some(BIND_CACHE_PATH.to_string()),
        pid_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_PID_FILENAME}")),
        session_keyfile: Some(format!("{BIND_CACHE_PATH}/{NAMED_SESSION_KEY_FILENAME}")),
        dump_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_DUMP_FILENAME}")),
        statistics_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_STATISTICS_FILENAME}")),
        memstatistics_file: Some(format!("{BIND_CACHE_PATH}/{NAMED_MEMSTATISTICS_FILENAME}")),
        allow_new_zones: Some(true),
        ..Default::default()
    }
}

/// Render named.conf.options: the `options` block, then the DNSSEC policy it
/// signs with, if any.
fn render_options_conf(options: OptionsBlock, policy: Option<DnssecPolicyStmt>) -> String {
    let mut statements = vec![Statement::Options(options)];
    statements.extend(policy.map(Statement::DnssecPolicy));
    write_conf(statements)
}

/// `allow-transfer { none; }`: emitted at the options level when no explicit
/// transfer ACL is configured anywhere (no instance, role, or global
/// `allow_transfer`).
///
/// BIND9's built-in default is `allow-transfer { any; }`, which would expose
/// every zone served by the instance to AXFR from any client: bulk zone
/// enumeration (threat model I2) and an amplification vector (D3). We deny by
/// default instead. Zones that legitimately need transfers get a **zone-level**
/// `allow-transfer` ACL scoped to their secondary IPs (see
/// `bind9::zone_ops`), and a zone-level ACL overrides this options-level
/// default in BIND9, so replication is unaffected by this hardening.
fn deny_all_transfers() -> AddressMatchList {
    vec![AddressMatchElement::None]
}

/// Default `responses-per-second` for BIND9 Response Rate Limiting (RRL) when
/// `spec.config.rateLimit` is not set. RRL is on by default (threat model
/// D1/D3, DNS amplification/reflection); a per-source-prefix cap of 15/s is
/// ISC's recommended conservative starting point and rarely affects legitimate
/// clients. Set `rateLimit.responsesPerSecond: 0` in the CRD to disable.
const DEFAULT_RATE_LIMIT_RESPONSES_PER_SECOND: u32 = 15;

/// Error-context name for the cluster-level `allow_query` field, shared by the
/// instance-level (global fallback) and cluster-level options builders.
const SOURCE_GLOBAL_ALLOW_QUERY: &str = "cluster spec.global.allow_query";

/// Error-context name for the cluster-level `allow_transfer` field, shared by
/// the instance-level (global fallback) and cluster-level options builders.
const SOURCE_GLOBAL_ALLOW_TRANSFER: &str = "cluster spec.global.allow_transfer";

/// Build the named.conf.options configuration for an instance
///
/// Generates the BIND9 options configuration file from the instance's config spec.
/// Includes settings for recursion, ACLs (allow-query, allow-transfer), DNSSEC,
/// forwarders, and listen addresses (listen-on / listen-on-v6).
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
///
/// # Errors
///
/// Returns an error if an ACL, forwarder, listen address or DNSSEC signing
/// value fails validation.
fn build_options_conf(
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    role_allow_transfer: Option<&Vec<String>>,
) -> anyhow::Result<String> {
    let instance_cfg = instance.spec.config.as_ref();
    let global_config = cluster.and_then(|c| c.spec.common.global.as_ref());

    // Allow-query ACL: the first configured level wins (instance, then
    // global); an explicitly empty list renders no directive.
    let allow_query = if let Some(acls) = instance_cfg.and_then(|c| c.allow_query.as_ref()) {
        acl_list(Some(acls), "instance spec.config.allow_query")?
    } else {
        acl_list(
            global_config.and_then(|g| g.allow_query.as_ref()),
            SOURCE_GLOBAL_ALLOW_QUERY,
        )?
    };

    // Allow-transfer ACL: priority instance config > role-specific > global.
    // An explicitly empty list at any level means `none`; with no explicit ACL
    // anywhere, deny by default (see `deny_all_transfers`).
    let allow_transfer = if let Some(acls) = instance_cfg.and_then(|c| c.allow_transfer.as_ref()) {
        transfer_acl(acls, "instance spec.config.allow_transfer")?
    } else if let Some(role_acls) = role_allow_transfer {
        transfer_acl(role_acls, "cluster role-specific allow_transfer")?
    } else if let Some(global_acls) = global_config.and_then(|g| g.allow_transfer.as_ref()) {
        transfer_acl(global_acls, SOURCE_GLOBAL_ALLOW_TRANSFER)?
    } else {
        deny_all_transfers()
    };

    // Every other option: instance overrides global, per field.
    let options = OptionsBlock {
        listen_on: vec![listen_on(
            LISTEN_ON_DIRECTIVE,
            instance_cfg
                .and_then(|c| c.listen_on.as_ref())
                .or_else(|| global_config.and_then(|g| g.listen_on.as_ref())),
        )?],
        listen_on_v6: vec![listen_on(
            LISTEN_ON_V6_DIRECTIVE,
            instance_cfg
                .and_then(|c| c.listen_on_v6.as_ref())
                .or_else(|| global_config.and_then(|g| g.listen_on_v6.as_ref())),
        )?],
        // Recursion is off when neither level sets it.
        recursion: Some(
            instance_cfg
                .and_then(|c| c.recursion)
                .or_else(|| global_config.and_then(|g| g.recursion))
                .unwrap_or(false),
        ),
        forwarders: forwarders(
            instance_cfg
                .and_then(|c| c.forwarders.as_ref())
                .or_else(|| global_config.and_then(|g| g.forwarders.as_ref())),
        )?,
        allow_query,
        allow_transfer: Some(allow_transfer),
        // Response Rate Limiting is on by default.
        rate_limit: rate_limit(
            instance_cfg
                .and_then(|c| c.rate_limit.as_ref())
                .or_else(|| global_config.and_then(|g| g.rate_limit.as_ref())),
        ),
        // dnssec-enable was removed in BIND 9.15+ (DNSSEC is always enabled);
        // only validation is configurable.
        dnssec_validation: resolve_dnssec_validation(instance_cfg, global_config),
        key_directory: key_directory(global_config, instance_cfg),
        ..base_options()
    };

    Ok(render_options_conf(
        options,
        dnssec_policy_statement(global_config, instance_cfg)?,
    ))
}

/// The `forwarders` list for named.conf.options.
///
/// Emits only the `forwarders` block (no `forward` mode statement), matching
/// BIND defaults. Empty when `forwarders` is `None` or empty, so no directive
/// is rendered.
///
/// # Arguments
///
/// * `forwarders` - Optional list of upstream DNS server IP addresses
///
/// # Errors
///
/// Returns an error if any entry is not a plain IPv4 or IPv6 address.
fn forwarders(forwarders: Option<&Vec<String>>) -> anyhow::Result<Vec<IpAddr>> {
    forwarders
        .into_iter()
        .flatten()
        .map(|entry| {
            let trimmed = entry.trim();
            trimmed.parse().map_err(|_| {
                anyhow::anyhow!(
                    "invalid forwarder {trimmed:?}: must be a plain IPv4 or IPv6 address"
                )
            })
        })
        .collect()
}

/// The `rate-limit { responses-per-second N; }` block for named.conf.options.
///
/// Response Rate Limiting (RRL) is **on by default**: when `rate_limit` is
/// `None`, or its `responses_per_second` is `None`, the conservative default
/// [`DEFAULT_RATE_LIMIT_RESPONSES_PER_SECOND`] is used. An explicit value of
/// `0` disables RRL: no block is emitted.
///
/// # Arguments
///
/// * `rate_limit` - Optional RRL config (instance value takes priority over the
///   cluster `global` value; resolve that before calling).
fn rate_limit(rate_limit: Option<&crate::crd::RateLimitConfig>) -> Option<RateLimit> {
    let rps = rate_limit
        .and_then(|r| r.responses_per_second)
        .unwrap_or(DEFAULT_RATE_LIMIT_RESPONSES_PER_SECOND);
    if rps == 0 {
        return None;
    }
    Some(RateLimit {
        responses_per_second: Some(rps),
        ..Default::default()
    })
}

/// A `listen-on` / `listen-on-v6` directive for named.conf.options.
///
/// Defaults to `{ any; }` when `addresses` is `None` or empty. The port is
/// [`DNS_CONTAINER_PORT`] (the unprivileged port `named` binds inside the pod;
/// the DNS Service exposes [`DNS_PORT`] to clients and forwards to it), not
/// the client-facing port.
///
/// # Arguments
///
/// * `directive` - Either [`LISTEN_ON_DIRECTIVE`] or [`LISTEN_ON_V6_DIRECTIVE`],
///   for error context
/// * `addresses` - Optional address match list from the CRD
///
/// # Errors
///
/// Returns an error if any entry fails address-match-list validation; see
/// [`crate::bind9_acl`] for the accepted syntax.
fn listen_on(directive: &str, addresses: Option<&Vec<String>>) -> anyhow::Result<ListenOn> {
    let addresses = match addresses {
        Some(addrs) if !addrs.is_empty() => parse_acl_list(addrs)
            .with_context(|| format!("invalid entry in {directive} address list"))?,
        _ => vec![AddressMatchElement::Any],
    };
    Ok(ListenOn {
        port: Some(DNS_CONTAINER_PORT),
        addresses,
    })
}

/// `dnssec-validation auto` or `no`.
///
/// Enabled is `auto`, which validates with BIND's built-in root trust anchor.
/// `yes` needs `trust-anchors` configured, which bindy does not do: BIND 9.18
/// then validates nothing, and BIND 9.20 refuses to load the configuration.
fn dnssec_validation(enabled: bool) -> DnssecValidation {
    if enabled {
        return DnssecValidation::Auto;
    }
    DnssecValidation::No
}

/// Resolve `dnssec-validation` for the instance-level options builder: the
/// instance config overrides the cluster global config.
///
/// Whichever level configures `dnssec` first (instance, then global) renders
/// an explicit directive, regardless of whether the instance has a `config`
/// block at all (ADR-0007: an absent directive means `auto` to `named`,
/// which would silently re-enable validation a user explicitly disabled).
/// When neither level configures `dnssec`, no directive is emitted and
/// `named`'s own default applies.
///
/// # Arguments
///
/// * `instance_config` - The instance's `spec.config`, if any
/// * `global_config` - The cluster's `spec.common.global`, if any
fn resolve_dnssec_validation(
    instance_config: Option<&Bind9Config>,
    global_config: Option<&Bind9Config>,
) -> Option<DnssecValidation> {
    instance_config
        .and_then(|c| c.dnssec.as_ref())
        .or_else(|| global_config.and_then(|g| g.dnssec.as_ref()))
        .map(|dnssec| dnssec_validation(dnssec.validation.unwrap_or(false)))
}

/// An ACL directive's address match list (`allow-query`), or none.
///
/// `None` when `acls` is `None` or empty, so no directive is emitted.
/// `source` names the originating CRD field for error context.
///
/// # Errors
///
/// Returns an error if any entry fails address-match-list validation; see
/// [`crate::bind9_acl`] for the accepted syntax.
fn acl_list(acls: Option<&Vec<String>>, source: &str) -> anyhow::Result<Option<AddressMatchList>> {
    let Some(acls) = acls else {
        return Ok(None);
    };
    if acls.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        parse_acl_list(acls).with_context(|| format!("invalid entry in {source}"))?,
    ))
}

/// The `allow-transfer` list, where an explicitly **empty** list means `none`
/// (deny) rather than "not configured".
///
/// # Errors
///
/// Returns an error if any entry fails address-match-list validation; see
/// [`crate::bind9_acl`] for the accepted syntax.
fn transfer_acl(acls: &[String], source: &str) -> anyhow::Result<AddressMatchList> {
    if acls.is_empty() {
        return Ok(deny_all_transfers());
    }
    parse_acl_list(acls).with_context(|| format!("invalid entry in {source}"))
}

/// Build the named.conf.options configuration for a cluster
///
/// Generates the BIND9 options configuration file from the cluster's `spec.global` config.
/// Includes settings for recursion, ACLs (allow-query, allow-transfer), DNSSEC,
/// forwarders, and listen addresses (listen-on / listen-on-v6).
///
/// # Arguments
///
/// * `cluster` - `Bind9Cluster` containing global configuration
///
/// # Returns
///
/// A string containing the complete named.conf.options configuration
///
/// # Errors
///
/// Returns an error if an ACL, forwarder, listen address or DNSSEC signing
/// value fails validation.
fn build_cluster_options_conf(cluster: &Bind9Cluster) -> anyhow::Result<String> {
    let global = cluster.spec.common.global.as_ref();

    // allow-transfer: same deny-by-default as the instance-level builder
    // (ADR-0007, closes the #466 gap): an explicit ACL renders it, an
    // explicitly empty list renders `none`, and no ACL at all denies AXFR
    // (BIND 9.18's own default is to allow transfers to ANY host).
    let allow_transfer = match global.and_then(|g| g.allow_transfer.as_ref()) {
        Some(acls) => transfer_acl(acls, SOURCE_GLOBAL_ALLOW_TRANSFER)?,
        None => deny_all_transfers(),
    };

    let options = OptionsBlock {
        listen_on: vec![listen_on(
            LISTEN_ON_DIRECTIVE,
            global.and_then(|g| g.listen_on.as_ref()),
        )?],
        listen_on_v6: vec![listen_on(
            LISTEN_ON_V6_DIRECTIVE,
            global.and_then(|g| g.listen_on_v6.as_ref()),
        )?],
        // Recursion is off unless the global config enables it.
        recursion: Some(global.and_then(|g| g.recursion).unwrap_or(false)),
        forwarders: forwarders(global.and_then(|g| g.forwarders.as_ref()))?,
        allow_query: acl_list(
            global.and_then(|g| g.allow_query.as_ref()),
            SOURCE_GLOBAL_ALLOW_QUERY,
        )?,
        allow_transfer: Some(allow_transfer),
        rate_limit: rate_limit(global.and_then(|g| g.rate_limit.as_ref())),
        // Emitted only when the global config sets `dnssec`.
        dnssec_validation: resolve_dnssec_validation(None, global),
        key_directory: key_directory(global, None),
        ..base_options()
    };

    Ok(render_options_conf(
        options,
        dnssec_policy_statement(global, None)?,
    ))
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
/// * `cluster_provider` - Optional `ClusterBind9Provider` containing shared configuration
/// * `rndc_secret_name` - Resolved RNDC `Secret` name (from `rndcKey.secretRef`,
///   an inline secret spec, or the auto-generated `{name}-rndc-key` default)
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
    bindcar_config: Option<crate::crd::BindcarConfig>,
    configmap_name: String,
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

    // Merged per field, so an instance setting one field does not discard the
    // rest of the cluster's sidecar configuration. See resolve_bindcar_config.
    let bindcar_config = resolve_bindcar_config(instance, cluster, cluster_provider);

    // Determine ConfigMap name: use cluster ConfigMap if instance belongs to a cluster
    let configmap_name = if instance.spec.cluster_ref.is_empty() {
        // Use instance-specific ConfigMap
        format!("{name}-config")
    } else {
        // Use cluster-level shared ConfigMap
        format!("{}-config", instance.spec.cluster_ref)
    };

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
/// a cluster-managed instance always has `replicas: 1` — the cluster
/// controller creates one single-Pod Deployment per nameserver — so replica
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

    let primaries = common
        .primary
        .as_ref()
        .and_then(|p| p.replicas)
        .unwrap_or(0);
    let secondaries = common
        .secondary
        .as_ref()
        .and_then(|s| s.replicas)
        .unwrap_or(0);

    let role_count = match instance.spec.role {
        crate::crd::ServerRole::Primary => primaries,
        crate::crd::ServerRole::Secondary => secondaries,
    };

    (role_count.max(1), (primaries + secondaries).max(1))
}

pub fn build_deployment(
    name: &str,
    namespace: &str,
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    cluster_provider: Option<&crate::crd::ClusterBind9Provider>,
    rndc_secret_name: &str,
) -> Deployment {
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

    let owner_refs = build_owner_references(instance);

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

    Deployment {
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
    }
}

/// The BIND9 image and its pull policy: the configured image, else the ISC
/// image at `version`, pulled `IfNotPresent` unless configured otherwise.
fn resolve_bind9_image(image_config: Option<&ImageConfig>, version: &str) -> (String, String) {
    let image = image_config
        .and_then(|img_cfg| img_cfg.image.clone())
        .unwrap_or_else(|| format!("internetsystemsconsortium/bind9:{version}"));
    let image_pull_policy = image_config
        .and_then(|cfg| cfg.image_pull_policy.clone())
        .unwrap_or_else(|| "IfNotPresent".into());
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
        security_context: Some(SecurityContext {
            run_as_non_root: Some(true),
            run_as_user: Some(BIND9_NONROOT_UID),
            run_as_group: Some(BIND9_NONROOT_UID),
            allow_privilege_escalation: Some(false),
            capabilities: Some(Capabilities {
                // Drop ALL capabilities and add none back. `named` binds the
                // unprivileged DNS port 5353 (DNS_CONTAINER_PORT), so it no
                // longer needs NET_BIND_SERVICE. This is the strictest posture
                // under Pod Security Admission `restricted`.
                drop: Some(vec!["ALL".to_string()]),
                add: None,
            }),
            // PSA `restricted` requires a RuntimeDefault (or Localhost) seccomp
            // profile on every container. Set it explicitly at the container
            // level in addition to the pod-level default.
            seccomp_profile: Some(SeccompProfile {
                type_: "RuntimeDefault".to_string(),
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
            seccomp_profile: Some(SeccompProfile {
                type_: "RuntimeDefault".to_string(),
                ..Default::default()
            }),
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

/// Volume carrying the sidecar's TLS key pair.
pub(crate) const VOLUME_BINDCAR_TLS: &str = "bindcar-tls";

/// Mount path for that volume inside the bindcar container.
pub(crate) const BINDCAR_TLS_PATH: &str = "/etc/bindcar/tls";

/// Environment variables the operator owns and a tenant may not set.
///
/// User-supplied `bindcarConfig.envVars` are appended after the
/// operator-managed ones, and the kubelet resolves a duplicate name to the
/// **last** occurrence — so without this guard a tenant entry silently wins.
/// The `BIND_TLS_` prefix matters as much as the rest: repointing
/// `BIND_TLS_CERT` substitutes the server's identity, and
/// `BIND_TLS_RELOAD_INTERVAL=0` pins a certificate that is being rotated away
/// from. See ADR-0004 and bindy guide 57 section 25.
pub(crate) const RESERVED_BINDCAR_ENV_PREFIXES: &[&str] = &["BIND_TLS_", "KUBE_"];

/// Exact environment variable names the operator owns.
pub(crate) const RESERVED_BINDCAR_ENV_NAMES: &[&str] = &[
    "BIND_API_TOKEN",
    "DISABLE_AUTH",
    "BIND_ALLOW_ANY_SERVICEACCOUNT",
    "BIND_ALLOWED_SERVICE_ACCOUNTS",
    "BIND_ALLOWED_NAMESPACES",
    "BIND_TOKEN_AUDIENCES",
    "BIND_ZONE_DIR",
    "RNDC_SECRET",
    "RNDC_ALGORITHM",
    "RNDC_KEY_NAME",
];

/// Whether `name` is an operator-reserved environment variable.
#[must_use]
pub(crate) fn is_reserved_bindcar_env(name: &str) -> bool {
    RESERVED_BINDCAR_ENV_NAMES.contains(&name)
        || RESERVED_BINDCAR_ENV_PREFIXES
            .iter()
            .any(|p| name.starts_with(p))
}

/// Build the Bindcar API sidecar container
///
/// # Arguments
///
/// * `bindcar_config` - Optional Bindcar container configuration from the instance spec
/// * `rndc_secret_name` - Name of the Secret containing the RNDC key
///
/// # Returns
///
/// A `Container` configured to run the Bindcar RNDC API sidecar
#[allow(clippy::too_many_lines)]
pub(crate) fn build_api_sidecar_container(
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

    // bindcar 0.7.0 (Mode B / TokenReview) validates the *caller's* SA token
    // against BIND_ALLOWED_SERVICE_ACCOUNTS. The caller is the bindy operator,
    // so the allow-list must name the operator SA in the operator's own
    // namespace — NOT the operand `bind9` SA. The operator namespace is taken
    // from POD_NAMESPACE (set on the operator Deployment) with a sane fallback.
    let operator_namespace = std::env::var("POD_NAMESPACE")
        .unwrap_or_else(|_| crate::constants::DEFAULT_OPERATOR_NAMESPACE.to_string());
    let allowed_service_account = format!(
        "system:serviceaccount:{operator_namespace}:{}",
        crate::constants::OPERATOR_SERVICE_ACCOUNT
    );

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
            value: Some(allowed_service_account),
            ..Default::default()
        },
        // bindcar 0.7.0 enforces the token audience from the TokenReview
        // response. The operator projects a token with the `bindcar` audience
        // (deploy/operator/deployment.yaml); this must match here.
        EnvVar {
            name: "BIND_TOKEN_AUDIENCES".into(),
            value: Some(crate::constants::BINDCAR_TOKEN_AUDIENCE.into()),
            ..Default::default()
        },
        // Writable scratch dir for bindcar's 0600 TSIG key file (nsupdate -k),
        // required because the sidecar runs with a read-only root filesystem.
        EnvVar {
            name: "TMPDIR".into(),
            value: Some(crate::constants::BINDCAR_TMP_PATH.into()),
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
        // The co-located `named` listens on the unprivileged DNS_CONTAINER_PORT
        // (5353), not 53. Point bindcar's dynamic-update (nsupdate) traffic at
        // that port; without this it would default to 53 and every update would
        // fail with connection refused.
        EnvVar {
            name: "NSUPDATE_PORT".into(),
            value: Some(DNS_CONTAINER_PORT.to_string()),
            ..Default::default()
        },
    ];

    // TLS transport (ADR-0004). Opt-in: with no `tls` block the sidecar is
    // configured exactly as in earlier releases.
    let tls = bindcar_config
        .and_then(|c| c.tls.as_ref())
        .filter(|t| t.is_enabled());

    if tls.is_some() {
        env_vars.push(EnvVar {
            name: "BIND_TLS_CERT".into(),
            value: Some(format!("{BINDCAR_TLS_PATH}/tls.crt")),
            ..Default::default()
        });
        env_vars.push(EnvVar {
            name: "BIND_TLS_KEY".into(),
            value: Some(format!("{BINDCAR_TLS_PATH}/tls.key")),
            ..Default::default()
        });
    }
    if let Some(interval) = tls.and_then(|t| t.reload_interval_seconds) {
        // Only set when configured; otherwise bindcar's own default applies.
        env_vars.push(EnvVar {
            name: "BIND_TLS_RELOAD_INTERVAL".into(),
            value: Some(interval.to_string()),
            ..Default::default()
        });
    }

    // Add user-provided environment variables, dropping any that would override
    // an operator-managed one. Appending them unfiltered would hand a tenant
    // control of the sidecar's auth and TLS configuration, because the kubelet
    // honours the last duplicate.
    let user_env_vars = bindcar_config.and_then(|config| config.env_vars.as_ref());
    for var in user_env_vars.into_iter().flatten() {
        if is_reserved_bindcar_env(&var.name) {
            warn!(
                env_var = %var.name,
                "Ignoring operator-reserved environment variable supplied via bindcarConfig.envVars"
            );
            continue;
        }
        env_vars.push(var.clone());
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
        volume_mounts: Some(
            vec![
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
                VolumeMount {
                    name: VOLUME_CONFIG.into(),
                    mount_path: BIND_RNDC_CONF_PATH.into(),
                    sub_path: Some(RNDC_CONF_FILENAME.into()),
                    ..Default::default()
                },
                // Writable /tmp (TMPDIR) for the bindcar TSIG key file, required
                // because readOnlyRootFilesystem is enabled below.
                VolumeMount {
                    name: VOLUME_TMP.into(),
                    mount_path: crate::constants::BINDCAR_TMP_PATH.into(),
                    ..Default::default()
                },
            ]
            .into_iter()
            .chain(tls.map(|_| VolumeMount {
                name: VOLUME_BINDCAR_TLS.into(),
                mount_path: BINDCAR_TLS_PATH.into(),
                read_only: Some(true),
                ..Default::default()
            }))
            .collect(),
        ),
        resources,
        // The sidecar had no probes at all, so a wedged bindcar still counted as
        // Ready and the operator would push zones into it. /api/v1/ready checks
        // that the zone directory is usable and that rndc answers, which means
        // `named` is alive — strictly more than the bind9 container's TCP probe.
        //
        // This deliberately does NOT require any zone to be loaded: a probe
        // that did would deadlock, since a pod whose containers are not ready
        // is never given zones. Holding the pod out of its Service until its
        // zones are loaded is the zones-loaded readiness gate's job (ADR-0017),
        // which the operator opens after writing the zones to the pod's
        // container-ready (but not yet Ready) endpoint.
        //
        // The scheme must follow the sidecar: with TLS on it serves HTTPS only,
        // and an HTTP probe would be refused, leaving the Pod permanently
        // unready.
        readiness_probe: Some(Probe {
            http_get: Some(HTTPGetAction {
                path: Some(BINDCAR_READY_PATH.into()),
                port: IntOrString::Int(port),
                scheme: Some(if tls.is_some() { "HTTPS" } else { "HTTP" }.to_string()),
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
            // The sidecar never binds a privileged port, so it keeps ALL
            // capabilities dropped and a read-only root filesystem — the
            // strictest posture under Pod Security Admission `restricted`.
            read_only_root_filesystem: Some(true),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                ..Default::default()
            }),
            seccomp_profile: Some(SeccompProfile {
                type_: "RuntimeDefault".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
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

    // The sidecar's TLS key pair (ADR-0004). `defaultMode` 0400 keeps the
    // private key unreadable by anything but the container's own user.
    if let Some(tls) = bindcar_tls.filter(|t| t.is_enabled()) {
        if let Some(secret_name) = tls.secret_name.as_ref() {
            volumes.push(Volume {
                name: VOLUME_BINDCAR_TLS.into(),
                secret: Some(k8s_openapi::api::core::v1::SecretVolumeSource {
                    secret_name: Some(secret_name.clone()),
                    default_mode: Some(0o400),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
    }

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
    }

    // ALWAYS add the generated config volume. The generated ConfigMap always
    // exists (it always carries at least rndc.conf, which is not overridable),
    // and rndc.conf is mounted from this volume unconditionally in both the
    // bind9 and bindcar containers. Omitting it when custom refs are set would
    // leave those mounts dangling and make the API server reject the Deployment.
    volumes.push(Volume {
        name: VOLUME_CONFIG.into(),
        config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
            name: configmap_name.to_string(),
            ..Default::default()
        }),
        ..Default::default()
    });

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
/// use bindy_bind9::bind9_resources::build_service;
/// use bindy_api::crd::{Bind9Instance, ServiceConfig};
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
/// let service = build_service("dns-primary", "bindy-system", &instance, Some(&config));
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
/// use bindy_bind9::bind9_resources::build_service_account;
/// use bindy_api::crd::Bind9Instance;
///
/// # fn example(instance: Bind9Instance) {
/// let service_account = build_service_account("bindy-system", &instance);
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

#[cfg(test)]
#[path = "rendered_config_tests.rs"]
mod rendered_config_tests;
