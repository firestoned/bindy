// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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
//!   [`crate::constants::ALLOWED_USER_MOUNT_PREFIXES`].
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
    ForbiddenSource { name: String, kind: &'static str },

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

    #[error("volumeMount {field} {value:?} contains '..' (path traversal not permitted)")]
    SubPathTraversal { field: &'static str, value: String },
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

fn validate_one_volume(v: &Volume) -> Result<(), VolumeRejection> {
    let name = v.name.clone();

    // Reject every source kind that isn't on our allow-list. This is an
    // explicit allow-list — anything not matched falls through to
    // ForbiddenSource at the bottom.
    if v.host_path.is_some() {
        return forbid(name, "hostPath");
    }
    if v.csi.is_some() {
        return forbid(name, "csi");
    }
    if v.flex_volume.is_some() {
        return forbid(name, "flexVolume");
    }
    if v.nfs.is_some() {
        return forbid(name, "nfs");
    }
    if v.iscsi.is_some() {
        return forbid(name, "iscsi");
    }
    if v.rbd.is_some() {
        return forbid(name, "rbd");
    }
    if v.cephfs.is_some() {
        return forbid(name, "cephfs");
    }
    if v.glusterfs.is_some() {
        return forbid(name, "glusterfs");
    }
    if v.azure_file.is_some() {
        return forbid(name, "azureFile");
    }
    if v.azure_disk.is_some() {
        return forbid(name, "azureDisk");
    }
    if v.gce_persistent_disk.is_some() {
        return forbid(name, "gcePersistentDisk");
    }
    if v.aws_elastic_block_store.is_some() {
        return forbid(name, "awsElasticBlockStore");
    }
    if v.cinder.is_some() {
        return forbid(name, "cinder");
    }
    if v.fc.is_some() {
        return forbid(name, "fc");
    }
    if v.flocker.is_some() {
        return forbid(name, "flocker");
    }
    if v.photon_persistent_disk.is_some() {
        return forbid(name, "photonPersistentDisk");
    }
    if v.portworx_volume.is_some() {
        return forbid(name, "portworxVolume");
    }
    if v.quobyte.is_some() {
        return forbid(name, "quobyte");
    }
    if v.scale_io.is_some() {
        return forbid(name, "scaleIO");
    }
    if v.storageos.is_some() {
        return forbid(name, "storageos");
    }
    if v.vsphere_volume.is_some() {
        return forbid(name, "vsphereVolume");
    }
    if v.projected.is_some() {
        return forbid(name, "projected");
    }
    if v.ephemeral.is_some() {
        return forbid(name, "ephemeral");
    }
    if v.git_repo.is_some() {
        return forbid(name, "gitRepo");
    }
    if v.downward_api.is_some() {
        return forbid(name, "downwardAPI");
    }

    // Allow-listed sources, with name-prefix checks where applicable.
    if v.empty_dir.is_some() {
        return Ok(());
    }
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

    // No recognised source — reject by default. This catches both empty
    // Volumes and any future variant Kubernetes adds.
    forbid(name, "unknown/none")
}

fn forbid(name: String, kind: &'static str) -> Result<(), VolumeRejection> {
    Err(VolumeRejection::ForbiddenSource { name, kind })
}

fn validate_one_volume_mount(m: &VolumeMount) -> Result<(), VolumeRejection> {
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
