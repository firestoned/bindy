// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! DNSSEC signing: the signing config an instance resolves to, the zone
//! policy name, the key volumes and the key init container (ADR-0012).

use super::pod::restricted_security_context;
use super::{BIND_DNSSEC_KEYS_PATH, EMPTY_DIR_MEDIUM_MEMORY, VOLUME_DNSSEC_KEYS};
use k8s_openapi::api::core::v1::{Container, Volume, VolumeMount};
use tracing::{debug, warn};

/// Where the init container sees the user's DNSSEC key Secret (ADR-0012).
/// Mounted read-only, and only into that container: `named` never sees it.
const BIND_DNSSEC_KEYS_SOURCE_PATH: &str = "/etc/bind/dnssec-keys-source";

/// The user's DNSSEC key Secret, the source the init container copies from.
const VOLUME_DNSSEC_KEYS_SOURCE: &str = "dnssec-keys-source";
/// Mode of the Secret's files: readable by the bind group (the pod's
/// `fsGroup`), which the init container runs as. Secret volumes are owned by
/// root, so an owner-only mode would leave them unreadable.
const DNSSEC_KEYS_SOURCE_MODE: i32 = 0o440;
/// Init container that copies Secret-supplied DNSSEC keys into the writable
/// key directory (ADR-0012).
const CONTAINER_NAME_DNSSEC_KEYS_INIT: &str = "dnssec-keys-init";

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

/// Name of the policy bindy defines when the cluster names none. Not
/// `"default"`: BIND reserves that for a built-in policy (see
/// `BIND_BUILTIN_DNSSEC_POLICIES` in `named_conf`).
pub(super) const DEFAULT_DNSSEC_POLICY_NAME: &str = "bindy";

/// The Secret a signing config takes its keys from, if any (ADR-0012).
pub(super) fn dnssec_key_secret(
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
        security_context: Some(restricted_security_context(true)),
        ..Default::default()
    })
}
