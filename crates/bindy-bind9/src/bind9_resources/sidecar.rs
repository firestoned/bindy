// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The bindcar API sidecar: its configuration merged across instance,
//! cluster and provider, and its container.

use super::pod::{
    env, restricted_security_context, secret_env, timed_probe, DEFAULT_IMAGE_PULL_POLICY,
    READINESS_TIMING,
};
use super::{
    BIND_CACHE_PATH, BIND_KEYS_PATH, BIND_RNDC_CONF_PATH, RNDC_CONF_FILENAME, VOLUME_CACHE,
    VOLUME_CONFIG, VOLUME_RNDC_KEY, VOLUME_TMP,
};
use crate::constants::{CONTAINER_NAME_BINDCAR, DNS_CONTAINER_PORT};
use crate::crd::{Bind9Cluster, Bind9Instance};
use k8s_openapi::api::core::v1::{Container, ContainerPort, HTTPGetAction, Probe, VolumeMount};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use std::collections::BTreeMap;
use tracing::warn;

/// bindcar's unauthenticated readiness endpoint. It reports whether the zone
/// directory is usable and rndc answers; it does not require any zone.
const BINDCAR_READY_PATH: &str = "/api/v1/ready";

/// Key of the RNDC `Secret` holding the base64 key material.
const RNDC_SECRET_KEY_SECRET: &str = "secret";
/// Key of the RNDC `Secret` holding the HMAC algorithm name.
const RNDC_SECRET_KEY_ALGORITHM: &str = "algorithm";

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
        .unwrap_or_else(|| DEFAULT_IMAGE_PULL_POLICY.to_string());

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
        env("BIND_ZONE_DIR", BIND_CACHE_PATH),
        env("API_PORT", port.to_string()),
        env("RUST_LOG", log_level),
        env("BIND_ALLOWED_SERVICE_ACCOUNTS", allowed_service_account),
        // bindcar 0.7.0 enforces the token audience from the TokenReview
        // response. The operator projects a token with the `bindcar` audience
        // (deploy/operator/deployment.yaml); this must match here.
        env(
            "BIND_TOKEN_AUDIENCES",
            crate::constants::BINDCAR_TOKEN_AUDIENCE,
        ),
        // Writable scratch dir for bindcar's 0600 TSIG key file (nsupdate -k),
        // required because the sidecar runs with a read-only root filesystem.
        env("TMPDIR", crate::constants::BINDCAR_TMP_PATH),
        secret_env("RNDC_SECRET", rndc_secret_name, RNDC_SECRET_KEY_SECRET),
        secret_env(
            "RNDC_ALGORITHM",
            rndc_secret_name,
            RNDC_SECRET_KEY_ALGORITHM,
        ),
        // The co-located `named` listens on the unprivileged DNS_CONTAINER_PORT
        // (5353), not 53. Point bindcar's dynamic-update (nsupdate) traffic at
        // that port; without this it would default to 53 and every update would
        // fail with connection refused.
        env("NSUPDATE_PORT", DNS_CONTAINER_PORT.to_string()),
    ];

    // TLS transport (ADR-0004). Opt-in: with no `tls` block the sidecar is
    // configured exactly as in earlier releases.
    let tls = bindcar_config
        .and_then(|c| c.tls.as_ref())
        .filter(|t| t.is_enabled());

    if tls.is_some() {
        env_vars.push(env("BIND_TLS_CERT", format!("{BINDCAR_TLS_PATH}/tls.crt")));
        env_vars.push(env("BIND_TLS_KEY", format!("{BINDCAR_TLS_PATH}/tls.key")));
    }
    if let Some(interval) = tls.and_then(|t| t.reload_interval_seconds) {
        // Only set when configured; otherwise bindcar's own default applies.
        env_vars.push(env("BIND_TLS_RELOAD_INTERVAL", interval.to_string()));
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
        readiness_probe: Some(timed_probe(
            &READINESS_TIMING,
            Probe {
                http_get: Some(HTTPGetAction {
                    path: Some(BINDCAR_READY_PATH.into()),
                    port: IntOrString::Int(port),
                    scheme: Some(if tls.is_some() { "HTTPS" } else { "HTTP" }.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )),
        // The sidecar never binds a privileged port, so it keeps ALL
        // capabilities dropped and a read-only root filesystem.
        security_context: Some(restricted_security_context(true)),
        ..Default::default()
    }
}
