// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for RNDC configuration precedence resolution.

#[cfg(test)]
mod tests {
    use crate::crd::{RndcAlgorithm, RndcKeyConfig, RndcSecretRef, ServerRole};
    use crate::reconcilers::bind9instance::config::*;

    // ========================================================================
    // Precedence Resolution Tests
    // ========================================================================

    #[test]
    fn test_resolve_rndc_config_instance_level_wins() {
        // Instance-level config should override all others
        let instance_config = Some(RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "30d".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha512,
        });

        let role_config = Some(RndcKeyConfig {
            auto_rotate: false,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        });

        let global_config = Some(RndcKeyConfig {
            auto_rotate: false,
            rotate_after: "90d".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha1,
        });

        let result = resolve_rndc_config(
            instance_config.as_ref(),
            role_config.as_ref(),
            global_config.as_ref(),
        );

        // Should use instance-level config
        assert!(result.auto_rotate);
        assert_eq!(result.rotate_after, "30d");
        assert_eq!(result.algorithm, RndcAlgorithm::HmacSha512);
    }

    #[test]
    fn test_resolve_rndc_config_role_level_wins_when_no_instance() {
        // Role-level config should override global when no instance config
        let role_config = Some(RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "60d".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha384,
        });

        let global_config = Some(RndcKeyConfig {
            auto_rotate: false,
            rotate_after: "90d".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha1,
        });

        let result = resolve_rndc_config(None, role_config.as_ref(), global_config.as_ref());

        // Should use role-level config
        assert!(result.auto_rotate);
        assert_eq!(result.rotate_after, "60d");
        assert_eq!(result.algorithm, RndcAlgorithm::HmacSha384);
    }

    #[test]
    fn test_resolve_rndc_config_global_level_wins_when_no_role_or_instance() {
        // Global config should be used when no instance or role config
        let global_config = Some(RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "90d".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        });

        let result = resolve_rndc_config(None, None, global_config.as_ref());

        // Should use global-level config
        assert!(result.auto_rotate);
        assert_eq!(result.rotate_after, "90d");
        assert_eq!(result.algorithm, RndcAlgorithm::HmacSha256);
    }

    #[test]
    fn test_resolve_rndc_config_default_when_all_none() {
        // Should return default config when all are None
        let result = resolve_rndc_config(None, None, None);

        // Should use default values
        assert!(!result.auto_rotate); // Default is false
        assert_eq!(result.rotate_after, "720h"); // Default from constant
        assert_eq!(result.algorithm, RndcAlgorithm::HmacSha256); // Default
        assert!(result.secret_ref.is_none());
        assert!(result.secret.is_none());
    }

    // ========================================================================
    // Backward Compatibility Tests
    // ========================================================================

    #[test]
    fn test_resolve_rndc_config_from_deprecated_new_field_wins() {
        // new rndc_keys should take precedence over deprecated rndc_secret_ref
        let rndc_keys = Some(RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "30d".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha512,
        });

        #[allow(deprecated)]
        let rndc_secret_ref = Some(RndcSecretRef {
            name: "old-secret".to_string(),
            algorithm: RndcAlgorithm::HmacSha256,
            key_name_key: "key-name".to_string(),
            secret_key: "secret".to_string(),
        });

        let result = resolve_rndc_config_from_deprecated(
            rndc_keys.as_ref(),
            rndc_secret_ref.as_ref(),
            ServerRole::Primary,
        );

        // Should use new rndc_keys config
        assert!(result.auto_rotate);
        assert_eq!(result.rotate_after, "30d");
        assert_eq!(result.algorithm, RndcAlgorithm::HmacSha512);
    }

    #[test]
    fn test_resolve_rndc_config_from_deprecated_fallback_to_old() {
        // Should fall back to deprecated rndc_secret_ref when rndc_keys is None
        #[allow(deprecated)]
        let rndc_secret_ref = Some(RndcSecretRef {
            name: "old-secret".to_string(),
            algorithm: RndcAlgorithm::HmacSha256,
            key_name_key: "key-name".to_string(),
            secret_key: "secret".to_string(),
        });

        let result = resolve_rndc_config_from_deprecated(
            None,
            rndc_secret_ref.as_ref(),
            ServerRole::Primary,
        );

        // Should convert deprecated field to new format
        assert!(!result.auto_rotate); // No rotation for deprecated format
        let secret_ref = result.secret_ref.as_ref().unwrap();
        assert_eq!(secret_ref.name, "old-secret");
        assert_eq!(secret_ref.algorithm, RndcAlgorithm::HmacSha256);
        assert!(result.secret.is_none());
    }

    #[test]
    fn test_resolve_rndc_config_from_deprecated_default_when_both_none() {
        // Should return default config when both fields are None
        let result = resolve_rndc_config_from_deprecated(None, None, ServerRole::Secondary);

        // Should use default values
        assert!(!result.auto_rotate);
        assert_eq!(result.rotate_after, "720h");
        assert_eq!(result.algorithm, RndcAlgorithm::HmacSha256);
        assert!(result.secret_ref.is_none());
        assert!(result.secret.is_none());
    }

    // ========================================================================
    // Edge Case Tests
    // ========================================================================

    #[test]
    fn test_resolve_rndc_config_partial_instance_config() {
        // Instance config with some fields set should merge with defaults
        let instance_config = Some(RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "2160h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::default(),
        });

        let result = resolve_rndc_config(instance_config.as_ref(), None, None);

        // Should use instance values where set
        assert!(result.auto_rotate);
        assert_eq!(result.rotate_after, "2160h");
        assert_eq!(result.algorithm, RndcAlgorithm::default());
    }

    #[test]
    fn test_resolve_rndc_config_with_secret_ref() {
        // Config with secret_ref should preserve it
        #[allow(deprecated)]
        let instance_config = Some(RndcKeyConfig {
            auto_rotate: false, // Rotation doesn't apply to secret_ref
            rotate_after: "720h".to_string(),
            secret_ref: Some(RndcSecretRef {
                name: "my-existing-secret".to_string(),
                algorithm: RndcAlgorithm::HmacSha256,
                key_name_key: "key-name".to_string(),
                secret_key: "secret".to_string(),
            }),
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        });

        let result = resolve_rndc_config(instance_config.as_ref(), None, None);

        // Should preserve secret_ref
        assert!(result.secret_ref.is_some());
        assert_eq!(result.secret_ref.unwrap().name, "my-existing-secret");
        assert!(!result.auto_rotate); // No rotation for external secrets
    }
}
