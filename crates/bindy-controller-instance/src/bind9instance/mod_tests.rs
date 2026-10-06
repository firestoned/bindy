// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `Bind9Instance` reconciliation integration.

#[cfg(test)]
mod tests {
    use crate::bind9instance::{
        calculate_requeue_duration, resources, ROTATION_DUE_MARGIN, ROTATION_OVERDUE_RECHECK,
    };
    use crate::crd::{RndcAlgorithm, RndcKeyConfig, RndcKeyRotationStatus};
    use chrono::Utc;
    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    // ========================================================================
    // Scheduled rotation wake (ADR-0016): with no periodic resync the
    // instance is woken exactly when its RNDC key falls due.
    // ========================================================================

    const HOUR_SECS: u64 = 3600;

    fn rotating_config() -> RndcKeyConfig {
        RndcKeyConfig {
            auto_rotate: true,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        }
    }

    #[test]
    fn test_calculate_requeue_duration_rotation_disabled() {
        let config = RndcKeyConfig {
            auto_rotate: false,
            ..rotating_config()
        };
        let now = Utc::now();
        let secret = create_test_secret_with_annotations(now, None, 0);

        assert!(calculate_requeue_duration(&config, &secret, now).is_none());
    }

    #[test]
    fn test_calculate_requeue_duration_no_annotations() {
        let secret = Secret {
            metadata: ObjectMeta {
                name: Some("test-secret".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(calculate_requeue_duration(&rotating_config(), &secret, Utc::now()).is_none());
    }

    #[test]
    fn test_calculate_requeue_duration_no_rotation_scheduled() {
        let now = Utc::now();
        let secret = create_test_secret_with_annotations(now, None, 0);

        assert!(calculate_requeue_duration(&rotating_config(), &secret, now).is_none());
    }

    #[test]
    fn test_calculate_requeue_duration_rotation_overdue() {
        let now = Utc::now();
        let created_at = now - chrono::Duration::hours(2);
        let secret = create_test_secret_with_annotations(
            created_at,
            Some(created_at + chrono::Duration::hours(1)), // due an hour ago
            0,
        );

        let wake = calculate_requeue_duration(&rotating_config(), &secret, now)
            .expect("an overdue rotation is re-checked");

        assert_eq!(wake, ROTATION_OVERDUE_RECHECK);
    }

    #[test]
    fn test_calculate_requeue_duration_wakes_when_the_key_falls_due() {
        let now = Utc::now();
        let created_at = now - chrono::Duration::hours(2);
        let rotate_at = now + chrono::Duration::hours(1);
        let secret = create_test_secret_with_annotations(created_at, Some(rotate_at), 0);

        let wake = calculate_requeue_duration(&rotating_config(), &secret, now)
            .expect("a pending rotation is scheduled");

        assert_eq!(
            wake,
            std::time::Duration::from_secs(HOUR_SECS) + ROTATION_DUE_MARGIN,
            "wake at rotate_at, not minutes early: an early wake finds nothing due"
        );
    }

    #[test]
    fn test_calculate_requeue_duration_honours_the_minimum_rotation_interval() {
        // Due in 2 minutes, but the key was created just now: rotation is held
        // back until MIN_TIME_BETWEEN_ROTATIONS_HOURS has passed.
        let now = Utc::now();
        let rotate_at = now + chrono::Duration::minutes(2);
        let secret = create_test_secret_with_annotations(now, Some(rotate_at), 0);

        let wake = calculate_requeue_duration(&rotating_config(), &secret, now)
            .expect("a pending rotation is scheduled");

        assert_eq!(
            wake,
            std::time::Duration::from_secs(HOUR_SECS) + ROTATION_DUE_MARGIN
        );
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
                placement: None,
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
                placement: None,
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
                namespace: Some("bindy-system".to_string()),
                annotations: Some(annotations),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    // ========================================================================
    // Parent generation tracking (parent_generation_changed)
    // ========================================================================

    use crate::bind9instance::parent_generation_changed;

    #[test]
    fn test_parent_generation_changed_detects_newer_parent() {
        // Parent generation moved from 3 to 5 since last observed
        assert!(parent_generation_changed(Some(5), Some(3)));
    }

    #[test]
    fn test_parent_generation_changed_unchanged_parent() {
        assert!(!parent_generation_changed(Some(3), Some(3)));
    }

    #[test]
    fn test_parent_generation_changed_never_observed() {
        // Parent exists but its generation was never recorded: must reconcile
        assert!(parent_generation_changed(Some(1), None));
    }

    #[test]
    fn test_parent_generation_changed_no_parent() {
        assert!(!parent_generation_changed(None, None));
        assert!(!parent_generation_changed(None, Some(4)));
    }

    #[test]
    fn test_parent_generation_changed_independent_of_instance_generation() {
        // THE BUG (regression guard): the old code compared the PARENT's
        // generation against the INSTANCE's own observedGeneration - two
        // unrelated counters. A parent at generation 2 whose changes were
        // already applied (observed parent generation 2) must NOT re-trigger,
        // regardless of what the instance's own generation is; and a parent
        // that moved to 7 MUST trigger even if the instance's own observed
        // generation is far higher (e.g. 100).
        assert!(!parent_generation_changed(Some(2), Some(2)));
        assert!(parent_generation_changed(Some(7), Some(2)));
    }
}
