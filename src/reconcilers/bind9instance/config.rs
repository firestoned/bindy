// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! RNDC configuration precedence resolution.
//!
//! Resolves RNDC key configuration from multiple sources following the precedence order:
//! 1. Instance level (`Bind9InstanceSpec.rndc_keys`)
//! 2. Role level (`PrimaryConfig.rndc_keys` or `SecondaryConfig.rndc_keys`)
//! 3. Global level (`Bind9Config.rndc_secret_ref` - deprecated)
//! 4. Default (auto-generated with defaults from constants)

use crate::constants::DEFAULT_ROTATION_INTERVAL;
use crate::crd::{RndcAlgorithm, RndcKeyConfig, RndcSecretRef, ServerRole};

/// Resolve RNDC configuration from multiple sources following precedence order.
///
/// # Precedence Order
///
/// 1. **Instance level** - Highest priority (overrides all others)
/// 2. **Role level** - Primary or Secondary role configuration
/// 3. **Global level** - Cluster-wide configuration
/// 4. **Default** - Auto-generated with default settings
///
/// # Arguments
///
/// * `instance_config` - Instance-level RNDC configuration (`Bind9InstanceSpec.rndc_keys`)
/// * `role_config` - Role-level RNDC configuration (`PrimaryConfig.rndc_keys` or `SecondaryConfig.rndc_keys`)
/// * `global_config` - Global RNDC configuration (from `Bind9ClusterCommonSpec`)
///
/// # Returns
///
/// Resolved `RndcKeyConfig` with the highest-precedence configuration applied.
///
/// # Examples
///
/// ```rust,no_run
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
    global_config: Option<&RndcKeyConfig>,
) -> RndcKeyConfig {
    // Precedence order: Instance > Role > Global > Default
    if let Some(config) = instance_config {
        return config.clone();
    }

    if let Some(config) = role_config {
        return config.clone();
    }

    if let Some(config) = global_config {
        return config.clone();
    }

    // Default: auto-generated with no rotation
    RndcKeyConfig {
        auto_rotate: false,
        rotate_after: DEFAULT_ROTATION_INTERVAL.to_string(),
        secret_ref: None,
        secret: None,
        algorithm: RndcAlgorithm::default(),
    }
}

/// Resolve RNDC configuration with backward compatibility for deprecated fields.
///
/// Handles migration from the deprecated `rndc_secret_ref` field to the new `rndc_keys` field.
/// If both are present, `rndc_keys` takes precedence.
///
/// # Arguments
///
/// * `rndc_keys` - New RNDC configuration field (preferred)
/// * `rndc_secret_ref` - Deprecated RNDC secret reference (for backward compatibility)
/// * `role` - Server role (Primary or Secondary)
///
/// # Returns
///
/// Resolved `RndcKeyConfig`. If only the deprecated field is present, converts it to the new format.
///
/// # Examples
///
/// ```rust,no_run
/// use bindy::crd::{RndcKeyConfig, RndcSecretRef, RndcAlgorithm, ServerRole};
/// use bindy::reconcilers::bind9instance::config::resolve_rndc_config_from_deprecated;
///
/// // New field takes precedence
/// let new_config = Some(RndcKeyConfig {
///     auto_rotate: true,
///     rotate_after: "30d".to_string(),
///     secret_ref: None,
///     secret: None,
///     algorithm: RndcAlgorithm::HmacSha256,
/// });
///
/// let resolved = resolve_rndc_config_from_deprecated(new_config.as_ref(), None, ServerRole::Primary);
/// assert!(resolved.auto_rotate);
/// ```
#[must_use]
pub fn resolve_rndc_config_from_deprecated(
    rndc_keys: Option<&RndcKeyConfig>,
    rndc_secret_ref: Option<&RndcSecretRef>,
    _role: ServerRole,
) -> RndcKeyConfig {
    // New field takes precedence
    if let Some(config) = rndc_keys {
        return config.clone();
    }

    // Fall back to deprecated field if present
    if let Some(secret_ref) = rndc_secret_ref {
        // Convert deprecated RndcSecretRef to new RndcKeyConfig
        return RndcKeyConfig {
            auto_rotate: false, // No rotation for user-managed secrets
            rotate_after: DEFAULT_ROTATION_INTERVAL.to_string(),
            secret_ref: Some(secret_ref.clone()),
            secret: None,
            algorithm: secret_ref.algorithm.clone(),
        };
    }

    // Default: auto-generated with no rotation
    RndcKeyConfig {
        auto_rotate: false,
        rotate_after: DEFAULT_ROTATION_INTERVAL.to_string(),
        secret_ref: None,
        secret: None,
        algorithm: RndcAlgorithm::default(),
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;
