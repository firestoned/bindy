// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! RNDC configuration precedence resolution.
//!
//! Resolves RNDC key configuration from multiple sources following the precedence order:
//! 1. Instance level (`Bind9InstanceSpec.rndc_key`)
//! 2. Role level (`PrimaryConfig.rndc_key` or `SecondaryConfig.rndc_key`)
//! 3. Deprecated instance-level `rndc_secret_ref` (backward compatibility)
//! 4. Default (auto-generated with defaults from constants)

use crate::constants::DEFAULT_ROTATION_INTERVAL;
use crate::crd::{RndcAlgorithm, RndcKeyConfig, RndcSecretRef};

/// The RNDC configuration used when nothing is configured: an
/// operator-generated key with no rotation.
#[must_use]
pub fn default_rndc_config() -> RndcKeyConfig {
    RndcKeyConfig {
        auto_rotate: false,
        rotate_after: DEFAULT_ROTATION_INTERVAL.to_string(),
        secret_ref: None,
        secret: None,
        algorithm: RndcAlgorithm::default(),
    }
}

/// Resolve RNDC configuration from every source, following precedence order.
///
/// # Precedence Order
///
/// 1. **Instance level** - Highest priority (overrides all others)
/// 2. **Role level** - Primary or Secondary role configuration
/// 3. **Deprecated `rndcSecretRef`** - converted to a non-rotating
///    `RndcKeyConfig` for the user-managed secret it names
/// 4. **Default** - [`default_rndc_config`]
///
/// # Arguments
///
/// * `instance_config` - Instance-level RNDC configuration (`Bind9InstanceSpec.rndc_key`)
/// * `role_config` - Role-level RNDC configuration (`PrimaryConfig.rndc_key` or `SecondaryConfig.rndc_key`)
/// * `deprecated_secret_ref` - The deprecated `Bind9InstanceSpec.rndc_secret_ref`
///   (backward compatibility); used only when neither new field is set
///
/// # Returns
///
/// Resolved `RndcKeyConfig` with the highest-precedence configuration applied.
///
/// # Examples
///
/// ```text
/// use bindy::crd::{RndcKeyConfig, RndcAlgorithm};
/// use bindy::reconcilers::bind9instance::config::resolve_rndc_config;
///
/// let instance_config = Some(RndcKeyConfig {
///     auto_rotate: true,
///     rotate_after: "30d".to_string(),
///     secret_ref: None,
///     secret: None,
///     algorithm: RndcAlgorithm::HmacSha256,
/// });
///
/// let resolved = resolve_rndc_config(instance_config.as_ref(), None, None);
/// assert!(resolved.auto_rotate);
/// ```
#[must_use]
pub fn resolve_rndc_config(
    instance_config: Option<&RndcKeyConfig>,
    role_config: Option<&RndcKeyConfig>,
    deprecated_secret_ref: Option<&RndcSecretRef>,
) -> RndcKeyConfig {
    if let Some(config) = instance_config.or(role_config) {
        return config.clone();
    }

    // Fall back to the deprecated field: a user-managed secret, never rotated
    if let Some(secret_ref) = deprecated_secret_ref {
        return RndcKeyConfig {
            secret_ref: Some(secret_ref.clone()),
            algorithm: secret_ref.algorithm.clone(),
            ..default_rndc_config()
        };
    }

    default_rndc_config()
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;
