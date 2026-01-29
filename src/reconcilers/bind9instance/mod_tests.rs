// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `Bind9Instance` reconciliation integration.

#[cfg(test)]
mod tests {
    use crate::crd::{RndcAlgorithm, RndcKeyConfig, RndcKeyRotationStatus};
    use crate::reconcilers::bind9instance::{calculate_requeue_duration, resources};
    use chrono::Utc;
    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    // ========================================================================
    // Requeue Duration Calculation Tests
    // ========================================================================

    #[test]
    fn test_calculate_requeue_duration_rotation_disabled() {
        // Given: Config with auto_rotate = false
        let config = RndcKeyConfig {
            auto_rotate: false,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        };

        let secret = create_test_secret_with_annotations(Utc::now(), None, 0);

        // When: Calculate requeue duration
        let result = calculate_requeue_duration(&config, &secret);

        // Then: Should return None (no rotation scheduled)
        assert!(result.is_none());
    }

    #[test]
    fn test_calculate_requeue_duration_no_annotations() {
        // Given: Config with auto_rotate = true but Secret has no annotations
        let config = RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        };

        let secret = Secret {
            metadata: ObjectMeta {
                name: Some("test-secret".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        // When: Calculate requeue duration
        let result = calculate_requeue_duration(&config, &secret);

        // Then: Should return None (can't determine rotation schedule)
        assert!(result.is_none());
    }

    #[test]
    fn test_calculate_requeue_duration_rotation_overdue() {
        // Given: Config with auto_rotate = true and rotation in the past
        let config = RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        };

        let past_time = Utc::now() - chrono::Duration::hours(2);
        let secret = create_test_secret_with_annotations(
            past_time,
            Some(past_time + chrono::Duration::hours(1)), // rotate_at is 1 hour ago
            0,
        );

        // When: Calculate requeue duration
        let result = calculate_requeue_duration(&config, &secret);

        // Then: Should return 30 seconds (immediate reconciliation)
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_secs(), 30);
    }

    #[test]
    fn test_calculate_requeue_duration_rotation_in_future() {
        // Given: Config with auto_rotate = true and rotation in 1 hour
        let config = RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        };

        let created_at = Utc::now();
        let rotate_at = created_at + chrono::Duration::hours(1);
        let secret = create_test_secret_with_annotations(created_at, Some(rotate_at), 0);

        // When: Calculate requeue duration
        let result = calculate_requeue_duration(&config, &secret);

        // Then: Should return duration slightly less than 1 hour (5 minutes early)
        assert!(result.is_some());
        let duration_secs = result.unwrap().as_secs();
        // Should be around 55 minutes (3600 - 300 seconds)
        assert!((3200..=3400).contains(&duration_secs));
    }

    #[test]
    fn test_calculate_requeue_duration_rotation_very_soon() {
        // Given: Config with auto_rotate = true and rotation in 2 minutes
        let config = RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        };

        let created_at = Utc::now();
        let rotate_at = created_at + chrono::Duration::minutes(2);
        let secret = create_test_secret_with_annotations(created_at, Some(rotate_at), 0);

        // When: Calculate requeue duration
        let result = calculate_requeue_duration(&config, &secret);

        // Then: Should return at least 30 seconds (minimum requeue)
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_secs(), 30);
    }

    // ========================================================================
    // Rotation Status Tests
    // ========================================================================

    #[test]
    fn test_rotation_status_struct_creation() {
        // Given: Rotation metadata
        let created_at = Utc::now();
        let rotate_at = created_at + chrono::Duration::days(30);
        let rotation_count = 5;

        // When: Create RndcKeyRotationStatus
        let status = RndcKeyRotationStatus {
            created_at: created_at.to_rfc3339(),
            rotate_at: Some(rotate_at.to_rfc3339()),
            last_rotated_at: Some(created_at.to_rfc3339()),
            rotation_count,
        };

        // Then: Status contains correct values
        assert_eq!(status.rotation_count, 5);
        assert!(status.rotate_at.is_some());
        assert!(status.last_rotated_at.is_some());
    }

    #[test]
    fn test_rotation_status_no_rotation() {
        // Given: Newly created Secret (no rotations yet)
        let created_at = Utc::now();

        // When: Create RndcKeyRotationStatus for new Secret
        let status = RndcKeyRotationStatus {
            created_at: created_at.to_rfc3339(),
            rotate_at: None,       // No rotation scheduled
            last_rotated_at: None, // Never rotated
            rotation_count: 0,
        };

        // Then: Status reflects no rotation history
        assert_eq!(status.rotation_count, 0);
        assert!(status.rotate_at.is_none());
        assert!(status.last_rotated_at.is_none());
    }

    // ========================================================================
    // Configuration Resolution Tests
    // ========================================================================

    #[test]
    fn test_resolve_full_rndc_config_instance_level() {
        use crate::crd::{Bind9Instance, Bind9InstanceSpec, ServerRole};

        // Given: Instance with rndc_key config
        let instance = Bind9Instance {
            metadata: ObjectMeta::default(),
            spec: Bind9InstanceSpec {
                cluster_ref: String::new(),
                role: ServerRole::Primary,
                rndc_key: Some(RndcKeyConfig {
                    auto_rotate: true,
                    rotate_after: "24h".to_string(),
                    secret_ref: None,
                    secret: None,
                    algorithm: RndcAlgorithm::HmacSha512,
                }),
                replicas: None,
                version: None,
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
                #[allow(deprecated)]
                rndc_secret_ref: None,
                storage: None,
                bindcar_config: None,
            },
            status: None,
        };

        // When: Resolve full RNDC config
        let resolved = resources::resolve_full_rndc_config(&instance, None, None);

        // Then: Should use instance-level config
        assert!(resolved.auto_rotate);
        assert_eq!(resolved.rotate_after, "24h");
        assert_eq!(resolved.algorithm, RndcAlgorithm::HmacSha512);
    }

    #[test]
    fn test_resolve_full_rndc_config_default() {
        use crate::crd::{Bind9Instance, Bind9InstanceSpec, ServerRole};

        // Given: Instance with no rndc_key config
        let instance = Bind9Instance {
            metadata: ObjectMeta::default(),
            spec: Bind9InstanceSpec {
                cluster_ref: String::new(),
                role: ServerRole::Primary,
                rndc_key: None,
                replicas: None,
                version: None,
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
                #[allow(deprecated)]
                rndc_secret_ref: None,
                storage: None,
                bindcar_config: None,
            },
            status: None,
        };

        // When: Resolve full RNDC config
        let resolved = resources::resolve_full_rndc_config(&instance, None, None);

        // Then: Should use default config
        assert!(!resolved.auto_rotate); // Default is false
        assert_eq!(resolved.rotate_after, "720h"); // Default interval
        assert_eq!(resolved.algorithm, RndcAlgorithm::HmacSha256); // Default algorithm
    }

    // ========================================================================
    // Helper Functions
    // ========================================================================

    fn create_test_secret_with_annotations(
        created_at: chrono::DateTime<Utc>,
        rotate_at: Option<chrono::DateTime<Utc>>,
        rotation_count: u32,
    ) -> Secret {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
            created_at.to_rfc3339(),
        );
        if let Some(rt) = rotate_at {
            annotations.insert(
                crate::constants::ANNOTATION_RNDC_ROTATE_AT.to_string(),
                rt.to_rfc3339(),
            );
        }
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATION_COUNT.to_string(),
            rotation_count.to_string(),
        );

        Secret {
            metadata: ObjectMeta {
                name: Some("test-rndc-key".to_string()),
                namespace: Some("dns-system".to_string()),
                annotations: Some(annotations),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
