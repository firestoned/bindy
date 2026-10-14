// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Strict allow-list validators for user-supplied `Volume` and `VolumeMount`
//! entries on `Bind9Instance` / `Bind9Cluster` CRDs.
//!
//! # Why
//!
//! `Bind9Instance.spec.volumes`, `Bind9Instance.spec.volumeMounts`, and the
//! same fields on `Bind9ClusterCommonSpec` are typed as the full
//! `k8s_openapi::api::core::v1::Volume` / `VolumeMount`. Without filtering, a
//! namespace-tenant who can create a `Bind9Instance` could mount the host
//! filesystem (`hostPath`), an arbitrary Secret in the target namespace
//! (`secret`), or other dangerous volume sources into a Pod the operator
//! stamps with cluster-wide RBAC. This module enforces an allow-list so the
//! reconciler can refuse the CR with a clear status condition before any
//! Pod is built.
//!
//! Closes audit finding F-001.
//!
//! # Allow-list
//!
//! - **Volume sources:** `emptyDir`, `configMap` (name must start with
//!   [`crate::constants::ALLOWED_USER_CONFIGMAP_PREFIX`]), `secret` (name
//!   must start with [`crate::constants::ALLOWED_USER_SECRET_PREFIX`]),
//!   `persistentVolumeClaim` (name must start with
//!   [`crate::constants::ALLOWED_USER_PVC_PREFIX`]).
//! - **VolumeMount.mountPath:** must begin with one of
//!   [`crate::constants::ALLOWED_USER_MOUNT_PREFIXES`] and must not contain
//!   `..` (a `..` segment can escape the prefix onto an operator-owned path).
//! - **VolumeMount.subPath / subPathExpr:** must not contain `..`.
//!
//! Everything else is rejected. This is an allow-list, not a block-list, so
//! future Volume variants added by Kubernetes are rejected by default.

use crate::constants::{
    ALLOWED_USER_CONFIGMAP_PREFIX, ALLOWED_USER_MOUNT_PREFIXES, ALLOWED_USER_PVC_PREFIX,
    ALLOWED_USER_SECRET_PREFIX,
};
use k8s_openapi::api::core::v1::{Volume, VolumeMount};
use thiserror::Error;

/// Rejection reasons returned by [`validate_user_volumes`] and
/// [`validate_user_volume_mounts`].
///
/// Each variant carries enough context for the reconciler to render a clear
/// status condition on the offending CR.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VolumeRejection {
    #[error(
        "volume {name:?} uses forbidden source kind {kind}: only emptyDir, configMap, secret \
         (with name prefix {ALLOWED_USER_SECRET_PREFIX:?}), or persistentVolumeClaim (with name \
         prefix {ALLOWED_USER_PVC_PREFIX:?}) are permitted"
    )]
    ForbiddenSource { name: String, kind: String },

    #[error("volume {name:?} sets more than one source ({sources:?}): a volume has exactly one")]
    MultipleSources { name: String, sources: Vec<String> },

    #[error(
        "volume {name:?} secret reference {secret:?} does not start with the required prefix \
         {ALLOWED_USER_SECRET_PREFIX:?}"
    )]
    SecretNamePrefix { name: String, secret: String },

    #[error(
        "volume {name:?} configMap reference {config_map:?} does not start with the required \
         prefix {ALLOWED_USER_CONFIGMAP_PREFIX:?}"
    )]
    ConfigMapNamePrefix { name: String, config_map: String },

    #[error(
        "volume {name:?} persistentVolumeClaim reference {pvc:?} does not start with the \
         required prefix {ALLOWED_USER_PVC_PREFIX:?}"
    )]
    PvcNamePrefix { name: String, pvc: String },

    #[error(
        "volumeMount mountPath {path:?} is outside the allowed prefixes \
         {ALLOWED_USER_MOUNT_PREFIXES:?}"
    )]
    MountPathOutsideAllowList { path: String },

    #[error("volumeMount mountPath {path:?} contains '..' (path traversal not permitted)")]
    MountPathTraversal { path: String },

    #[error("volumeMount {field} {value:?} contains '..' (path traversal not permitted)")]
    SubPathTraversal { field: &'static str, value: String },

    #[error(
        "DNSSEC keysFrom secret reference {secret:?} does not start with the required prefix \
         {ALLOWED_USER_SECRET_PREFIX:?}"
    )]
    DnssecKeySecretPrefix { secret: String },

    #[error(
        "rndcKey.secretRef {secret:?} does not start with the required prefix \
         {ALLOWED_USER_SECRET_PREFIX:?}"
    )]
    RndcKeySecretPrefix { secret: String },
}

/// Validate that a `rndcKey.secretRef` (or deprecated `rndcSecretRef`) names
/// a Secret with the user-secret name prefix (ADR-0023, Decision 6).
///
/// The operator reads two entries of that Secret and places them in the
/// `{instance}-rndc-key` mirror the BIND9 pods mount, so without this check
/// anyone who can create a `Bind9Instance`, `Bind9Cluster` or
/// `ClusterBind9Provider` could have the operator copy entries out of any
/// Secret in the namespace. The `bindy-pod-shape-validation` admission
/// policy enforces the same rule.
///
/// # Errors
///
/// Returns [`VolumeRejection::RndcKeySecretPrefix`] if `name` does not start
/// with [`crate::constants::ALLOWED_USER_SECRET_PREFIX`].
pub fn validate_rndc_key_secret_name(name: &str) -> Result<(), VolumeRejection> {
    if name.starts_with(ALLOWED_USER_SECRET_PREFIX) {
        return Ok(());
    }
    Err(VolumeRejection::RndcKeySecretPrefix {
        secret: name.to_string(),
    })
}

/// Validate that a user-supplied DNSSEC key Secret name obeys the same
/// name-prefix allow-list as user `secret:` volumes.
///
/// `spec.dnssec.signing.keysFrom.secretRef` is mounted into the operand Pod
/// outside the normal `spec.volumes` path, so without this check a tenant can
/// mount an arbitrary Secret in the namespace (e.g. another tenant's RNDC/TSIG
/// key), bypassing both [`validate_user_volumes`] and the pod-shape admission
/// policy. Closes audit finding H2.
///
/// # Errors
///
/// Returns [`VolumeRejection::DnssecKeySecretPrefix`] if `name` does not start
/// with [`crate::constants::ALLOWED_USER_SECRET_PREFIX`].
pub fn validate_dnssec_key_secret_name(name: &str) -> Result<(), VolumeRejection> {
    if name.starts_with(ALLOWED_USER_SECRET_PREFIX) {
        return Ok(());
    }
    Err(VolumeRejection::DnssecKeySecretPrefix {
        secret: name.to_string(),
    })
}

/// Validate a slice of user-supplied [`Volume`] entries against the
/// allow-list. Returns the first rejection encountered.
///
/// # Errors
///
/// Returns [`VolumeRejection`] for the first volume that fails any check.
pub fn validate_user_volumes(vols: &[Volume]) -> Result<(), VolumeRejection> {
    for v in vols {
        validate_one_volume(v)?;
    }
    Ok(())
}

/// Validate the `Option<&Vec<Volume>>` that the resource builder passes
/// around. Convenience wrapper so callers can skip the `if let Some` dance.
///
/// # Errors
///
/// Same as [`validate_user_volumes`].
pub fn validate_optional_user_volumes(vols: Option<&Vec<Volume>>) -> Result<(), VolumeRejection> {
    match vols {
        Some(vs) => validate_user_volumes(vs),
        None => Ok(()),
    }
}

/// Validate a slice of user-supplied [`VolumeMount`] entries against the
/// allow-list. Returns the first rejection encountered.
///
/// # Errors
///
/// Returns [`VolumeRejection`] for the first mount that fails any check.
pub fn validate_user_volume_mounts(mounts: &[VolumeMount]) -> Result<(), VolumeRejection> {
    for m in mounts {
        validate_one_volume_mount(m)?;
    }
    Ok(())
}

/// Validate the `Option<&Vec<VolumeMount>>` that the resource builder passes
/// around. Convenience wrapper.
///
/// # Errors
///
/// Same as [`validate_user_volume_mounts`].
pub fn validate_optional_user_volume_mounts(
    mounts: Option<&Vec<VolumeMount>>,
) -> Result<(), VolumeRejection> {
    match mounts {
        Some(ms) => validate_user_volume_mounts(ms),
        None => Ok(()),
    }
}

// ----------------------------------------------------------------------
// internals
// ----------------------------------------------------------------------

/// The `Volume` field that is not a source: every other field names one.
const VOLUME_NAME_FIELD: &str = "name";

/// The source kinds a user volume may use (the allow-list).
const ALLOWED_VOLUME_SOURCES: [&str; 4] =
    ["emptyDir", "secret", "configMap", "persistentVolumeClaim"];

/// The volume's set source fields, by their Kubernetes (camelCase) names.
///
/// Read from the serialized volume rather than from a hand-kept list of
/// fields, so a source Kubernetes adds later is seen, and rejected, too.
fn volume_sources(v: &Volume) -> Vec<String> {
    let Ok(serde_json::Value::Object(fields)) = serde_json::to_value(v) else {
        return Vec::new();
    };
    fields
        .into_iter()
        .filter(|(key, value)| key != VOLUME_NAME_FIELD && !value.is_null())
        .map(|(key, _)| key)
        .collect()
}

fn validate_one_volume(v: &Volume) -> Result<(), VolumeRejection> {
    let name = v.name.clone();

    // A true allow-list: exactly one source, and that one on the list.
    let sources = volume_sources(v);
    let source = match sources.as_slice() {
        [] => return forbid(name, "none"),
        [one] => one.as_str(),
        _ => return Err(VolumeRejection::MultipleSources { name, sources }),
    };
    if !ALLOWED_VOLUME_SOURCES.contains(&source) {
        return forbid(name, source);
    }

    // Allow-listed sources, with name-prefix checks where applicable.
    if let Some(ref s) = v.secret {
        let secret = s.secret_name.clone().unwrap_or_default();
        if secret.starts_with(ALLOWED_USER_SECRET_PREFIX) && !secret.is_empty() {
            return Ok(());
        }
        return Err(VolumeRejection::SecretNamePrefix { name, secret });
    }
    if let Some(ref cm) = v.config_map {
        let config_map = cm.name.clone();
        if config_map.starts_with(ALLOWED_USER_CONFIGMAP_PREFIX) {
            return Ok(());
        }
        return Err(VolumeRejection::ConfigMapNamePrefix { name, config_map });
    }
    if let Some(ref pvc) = v.persistent_volume_claim {
        let pvc_name = pvc.claim_name.clone();
        if pvc_name.starts_with(ALLOWED_USER_PVC_PREFIX) {
            return Ok(());
        }
        return Err(VolumeRejection::PvcNamePrefix {
            name,
            pvc: pvc_name,
        });
    }
    // emptyDir: no name to check.
    Ok(())
}

fn forbid(name: String, kind: &str) -> Result<(), VolumeRejection> {
    Err(VolumeRejection::ForbiddenSource {
        name,
        kind: kind.to_string(),
    })
}

fn validate_one_volume_mount(m: &VolumeMount) -> Result<(), VolumeRejection> {
    // Reject path traversal FIRST: a `..` segment lets an attacker satisfy the
    // prefix allow-list (e.g. `/data/../etc/bind/named.conf` starts with
    // `/data/`) while the kubelet resolves the mount onto an operator-owned
    // path. This guard must run before the prefix check, which would otherwise
    // accept the escaping path. Closes audit finding C1.
    if m.mount_path.contains("..") {
        return Err(VolumeRejection::MountPathTraversal {
            path: m.mount_path.clone(),
        });
    }

    if !ALLOWED_USER_MOUNT_PREFIXES
        .iter()
        .any(|p| m.mount_path.starts_with(p))
    {
        return Err(VolumeRejection::MountPathOutsideAllowList {
            path: m.mount_path.clone(),
        });
    }

    if let Some(ref sub) = m.sub_path {
        if sub.contains("..") {
            return Err(VolumeRejection::SubPathTraversal {
                field: "subPath",
                value: sub.clone(),
            });
        }
    }
    if let Some(ref sub_expr) = m.sub_path_expr {
        if sub_expr.contains("..") {
            return Err(VolumeRejection::SubPathTraversal {
                field: "subPathExpr",
                value: sub_expr.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "safe_volume_tests.rs"]
mod safe_volume_tests;
