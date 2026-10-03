// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `resources.rs`
//!
//! These tests document expected behavior for Kubernetes resource management.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_reconcile_resources_creates_all() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A new `Bind9Instance` with no existing resources
        // When: reconcile_resources is called
        // Then: Should create `ConfigMap`, RNDC Secret, Service, and Deployment
        //       AND return Ok(())
        //       AND log "Created all resources for instance"
    }

    #[tokio::test]
    async fn test_reconcile_resources_updates_existing() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance with existing resources
        //        AND spec.version changed from "9.18.20" to "9.18.24"
        // When: reconcile_resources is called
        // Then: Should update Deployment with new image version
        //       AND update `ConfigMap` if configuration changed
        //       AND log "Updated resources for instance"
    }

    #[tokio::test]
    async fn test_reconcile_resources_skips_unchanged() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance with existing resources
        //        AND no spec changes
        // When: reconcile_resources is called
        // Then: Should NOT update any resources
        //       AND log "Resources unchanged, skipping update"
    }

    #[tokio::test]
    async fn test_create_configmap_with_cluster_config() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance referencing a cluster with global configuration
        // When: create_configmap is called
        // Then: Should create `ConfigMap` with cluster-level config
        //       AND include named.conf with cluster settings
        //       AND include named.conf.options
    }

    #[tokio::test]
    async fn test_create_configmap_with_custom_refs() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance with spec.configMapRefs set
        // When: create_configmap is called
        // Then: Should skip `ConfigMap` creation
        //       AND log "Instance uses custom `ConfigMaps`, skipping creation"
    }

    #[tokio::test]
    async fn test_create_deployment_with_replicas() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance with spec.replicas = 3
        // When: create_deployment is called
        // Then: Should create Deployment with 3 replicas
        //       AND set anti-affinity rules for pod distribution
    }

    #[tokio::test]
    async fn test_create_service_primary_with_loadbalancer() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A primary instance
        // When: create_service is called
        // Then: Should create Service with type=LoadBalancer
        //       AND expose DNS ports (53/TCP, 53/UDP, 9530/TCP)
    }

    #[tokio::test]
    async fn test_create_service_secondary_cluster_ip() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A secondary instance
        // When: create_service is called
        // Then: Should create Service with type=ClusterIP
        //       AND expose DNS ports (53/TCP, 53/UDP, 9530/TCP)
    }

    #[tokio::test]
    async fn test_delete_resources_cleanup() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance being deleted (has deletionTimestamp)
        // When: delete_resources is called
        // Then: Should delete Deployment first (graceful shutdown)
        //       AND delete Service
        //       AND delete `ConfigMap`
        //       AND delete RNDC Secret
        //       AND log "Successfully deleted all resources"
    }

    // ========================================================================
    // RNDC Secret Creation and Rotation Tests
    // ========================================================================

    #[tokio::test]
    async fn test_create_rndc_secret_auto_generated_mode() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance with auto-generated RNDC config (no secret_ref or secret)
        //        AND config.auto_rotate = true, config.rotate_after = "720h"
        // When: create_or_update_rndc_secret is called
        // Then: Should generate a new RNDC key
        //       AND create Secret with annotations:
        //           - bindy.firestoned.io/rndc-created-at = current timestamp
        //           - bindy.firestoned.io/rndc-rotate-at = created_at + 720h
        //           - bindy.firestoned.io/rndc-rotation-count = "0"
        //       AND log "Created RNDC Secret with rotation enabled"
    }

    #[tokio::test]
    async fn test_create_rndc_secret_with_secret_ref() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance with config.secret_ref = Some(RndcSecretRef{name: "my-secret"})
        // When: create_or_update_rndc_secret is called
        // Then: Should NOT create a new Secret
        //       AND log "Using existing Secret reference: my-secret"
        //       AND return the secret name for deployment configuration
    }

    #[tokio::test]
    async fn test_create_rndc_secret_with_inline_spec() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An instance with config.secret = Some(SecretSpec{...})
        //        AND config.auto_rotate = true
        // When: create_or_update_rndc_secret is called
        // Then: Should create Secret from inline spec
        //       AND add rotation annotations
        //       AND log "Created RNDC Secret from inline spec"
    }

    #[tokio::test]
    async fn test_should_rotate_secret_rotation_due() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A Secret with annotations:
        //        - rndc-created-at = "2025-01-01T00:00:00Z"
        //        - rndc-rotate-at = "2025-01-31T00:00:00Z"
        //        AND current time is 2025-02-01T00:00:00Z (past rotate_at)
        // When: should_rotate_secret is called
        // Then: Should return true (rotation is due)
    }

    #[tokio::test]
    async fn test_should_rotate_secret_not_due() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A Secret with annotations:
        //        - rndc-created-at = "2025-01-01T00:00:00Z"
        //        - rndc-rotate-at = "2025-12-31T00:00:00Z"
        //        AND current time is 2025-01-15T00:00:00Z (before rotate_at)
        // When: should_rotate_secret is called
        // Then: Should return false (not yet due)
    }

    #[tokio::test]
    async fn test_should_rotate_secret_rate_limit() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A Secret rotated 30 minutes ago
        //        AND rotate_at is in the past
        // When: should_rotate_secret is called
        // Then: Should return false (within 1-hour rate limit)
        //       AND log "Skipping rotation - min 1 hour between rotations"
    }

    #[tokio::test]
    async fn test_rotate_rndc_secret_updates_annotations() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: An existing Secret with rotation_count = "5"
        // When: rotate_rndc_secret is called
        // Then: Should generate a new RNDC key
        //       AND update Secret annotations:
        //           - rndc-created-at = current timestamp
        //           - rndc-rotate-at = current timestamp + rotate_after
        //           - rndc-rotation-count = "6" (incremented)
        //       AND replace Secret data with new key
        //       AND log "Rotated RNDC Secret (rotation #6)"
    }

    #[tokio::test]
    async fn test_rotate_rndc_secret_no_rotation_if_disabled() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A Secret with auto_rotate = false in config
        // When: create_or_update_rndc_secret is called
        // Then: Should NOT check rotation eligibility
        //       AND NOT call rotate_rndc_secret
        //       AND log "Auto-rotation disabled for this instance"
    }

    // ========================================================================
    // Pod Restart After Rotation Tests (Phase 5)
    // ========================================================================

    #[tokio::test]
    async fn test_trigger_deployment_rollout_after_rotation() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A successful RNDC Secret rotation
        //        AND a Deployment exists for the instance
        // When: trigger_deployment_rollout is called
        // Then: Should patch Deployment pod template annotation:
        //           - bindy.firestoned.io/rndc-rotated-at = current timestamp
        //       AND trigger rolling restart of all pods
        //       AND log "Triggered Deployment rollout after RNDC rotation"
    }

    #[tokio::test]
    async fn test_trigger_deployment_rollout_updates_annotation() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A Deployment with existing rndc-rotated-at annotation = "2025-01-01T00:00:00Z"
        // When: trigger_deployment_rollout is called after rotation
        // Then: Should update annotation to current timestamp
        //       AND Kubernetes will detect annotation change
        //       AND trigger rolling restart of pods
    }

    #[tokio::test]
    async fn test_rotate_rndc_secret_triggers_pod_restart() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A Secret rotation is due
        //        AND auto_rotate = true
        // When: rotate_rndc_secret is called
        // Then: Should generate new RNDC key
        //       AND replace Secret with new key
        //       AND call trigger_deployment_rollout
        //       AND pods will restart with new RNDC key
        //       AND log "Successfully rotated RNDC Secret (rotation #N)"
        //       AND log "Triggered Deployment rollout after RNDC rotation"
    }

    #[tokio::test]
    async fn test_trigger_deployment_rollout_fails_gracefully() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: RNDC Secret rotation succeeded
        //        BUT Deployment patch fails (e.g., Deployment not found)
        // When: trigger_deployment_rollout is called
        // Then: Should return error
        //       AND Secret rotation is ALREADY COMPLETE (not rolled back)
        //       AND operator will retry on next reconciliation
        //       AND log error message with details
    }

    // ----------------------------------------------------------------
    // evaluate_existing_rndc_secret — pure decision-logic tests
    //
    // Regression coverage for the malformed-Secret bug: after deleting a
    // malformed Secret, control previously continued into the rotation /
    // algorithm checks on the stale in-memory copy and could early-return
    // WITHOUT recreating the Secret, leaving the Deployment mounting a
    // Secret that no longer exists.
    // ----------------------------------------------------------------

    use crate::crd::{RndcAlgorithm, RndcKeyConfig};
    use crate::reconcilers::bind9instance::resources::{
        evaluate_existing_rndc_secret, RndcSecretAction,
    };
    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::ByteString;
    use std::collections::BTreeMap;

    fn rndc_config(auto_rotate: bool) -> RndcKeyConfig {
        RndcKeyConfig {
            auto_rotate,
            rotate_after: "720h".to_string(),
            secret_ref: None,
            secret: None,
            algorithm: RndcAlgorithm::HmacSha256,
        }
    }

    fn valid_secret_data() -> BTreeMap<String, ByteString> {
        let mut data = BTreeMap::new();
        data.insert(
            "key-name".to_string(),
            ByteString(b"bindy-operator".to_vec()),
        );
        data.insert("algorithm".to_string(), ByteString(b"hmac-sha256".to_vec()));
        data.insert("secret".to_string(), ByteString(b"c2VjcmV0".to_vec()));
        data
    }

    fn secret_with_data(data: Option<BTreeMap<String, ByteString>>) -> Secret {
        Secret {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("test-rndc-key".to_string()),
                ..Default::default()
            },
            data,
            ..Default::default()
        }
    }

    #[test]
    fn evaluate_rndc_secret_recreates_when_data_missing() {
        let secret = secret_with_data(None);
        let action = evaluate_existing_rndc_secret(&secret, &rndc_config(false)).unwrap();
        assert!(
            matches!(action, RndcSecretAction::Recreate(_)),
            "Secret without data must be recreated, got {action:?}"
        );
    }

    #[test]
    fn evaluate_rndc_secret_recreates_when_required_keys_missing() {
        let mut data = valid_secret_data();
        data.remove("secret");
        let secret = secret_with_data(Some(data));
        let action = evaluate_existing_rndc_secret(&secret, &rndc_config(false)).unwrap();
        assert!(
            matches!(action, RndcSecretAction::Recreate(_)),
            "Secret missing required keys must be recreated, got {action:?}"
        );
    }

    #[test]
    fn evaluate_rndc_secret_recreates_malformed_even_with_auto_rotate() {
        // The original bug: a malformed Secret with auto_rotate enabled fell
        // into the rotation-annotation branch after deletion and returned
        // early without recreating. Malformedness must win over rotation.
        let secret = secret_with_data(None);
        let action = evaluate_existing_rndc_secret(&secret, &rndc_config(true)).unwrap();
        assert!(
            matches!(action, RndcSecretAction::Recreate(_)),
            "malformed Secret must be recreated regardless of rotation config, got {action:?}"
        );
    }

    #[test]
    fn evaluate_rndc_secret_keeps_valid_secret() {
        let secret = secret_with_data(Some(valid_secret_data()));
        let action = evaluate_existing_rndc_secret(&secret, &rndc_config(false)).unwrap();
        assert_eq!(action, RndcSecretAction::Keep);
    }

    #[test]
    fn evaluate_rndc_secret_recreates_on_algorithm_mismatch() {
        let secret = secret_with_data(Some(valid_secret_data()));
        let mut config = rndc_config(false);
        config.algorithm = RndcAlgorithm::HmacSha512;
        let action = evaluate_existing_rndc_secret(&secret, &config).unwrap();
        assert!(
            matches!(action, RndcSecretAction::Recreate(_)),
            "algorithm drift must recreate the Secret, got {action:?}"
        );
    }

    #[test]
    fn evaluate_rndc_secret_adds_annotations_when_rotation_enabled() {
        let secret = secret_with_data(Some(valid_secret_data()));
        let action = evaluate_existing_rndc_secret(&secret, &rndc_config(true)).unwrap();
        assert_eq!(action, RndcSecretAction::AddRotationAnnotations);
    }

    #[test]
    fn evaluate_rndc_secret_rotates_when_due() {
        use chrono::{Duration, Utc};

        let mut secret = secret_with_data(Some(valid_secret_data()));
        let mut annotations = std::collections::BTreeMap::new();
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
            (Utc::now() - Duration::hours(2)).to_rfc3339(),
        );
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATE_AT.to_string(),
            (Utc::now() - Duration::minutes(5)).to_rfc3339(),
        );
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATION_COUNT.to_string(),
            "0".to_string(),
        );
        secret.metadata.annotations = Some(annotations);

        let action = evaluate_existing_rndc_secret(&secret, &rndc_config(true)).unwrap();
        assert_eq!(action, RndcSecretAction::Rotate);
    }

    // ----------------------------------------------------------------
    // F-001: validate_user_pod_shape — pure-function tests
    // ----------------------------------------------------------------

    use crate::crd::{
        Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, Bind9Instance, Bind9InstanceSpec,
        ServerRole,
    };
    use crate::reconcilers::bind9instance::resources::validate_user_pod_shape_for_test;
    use k8s_openapi::api::core::v1::{HostPathVolumeSource, Volume, VolumeMount};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn instance_with_volumes(volumes: Vec<Volume>, mounts: Vec<VolumeMount>) -> Bind9Instance {
        #[allow(deprecated)]
        Bind9Instance {
            metadata: ObjectMeta {
                name: Some("test-instance".to_string()),
                namespace: Some("tenant-a".to_string()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                placement: None,
                cluster_ref: String::new(),
                role: ServerRole::Primary,
                replicas: Some(1),
                version: None,
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: if volumes.is_empty() {
                    None
                } else {
                    Some(volumes)
                },
                volume_mounts: if mounts.is_empty() {
                    None
                } else {
                    Some(mounts)
                },
                rndc_secret_ref: None,
                rndc_key: None,
                storage: None,
                bindcar_config: None,
            },
            status: None,
        }
    }

    #[test]
    fn validate_pod_shape_accepts_default_instance() {
        let inst = instance_with_volumes(vec![], vec![]);
        validate_user_pod_shape_for_test(&inst, None, None).expect("default instance must pass");
    }

    #[test]
    fn validate_pod_shape_rejects_hostpath_on_instance() {
        let inst = instance_with_volumes(
            vec![Volume {
                name: "host-root".into(),
                host_path: Some(HostPathVolumeSource {
                    path: "/".into(),
                    type_: Some("Directory".into()),
                }),
                ..Default::default()
            }],
            vec![],
        );
        let err = validate_user_pod_shape_for_test(&inst, None, None)
            .expect_err("hostPath on instance must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("Bind9Instance test-instance spec.volumes"),
            "{msg}"
        );
    }

    #[test]
    fn validate_pod_shape_rejects_mount_outside_data() {
        let inst = instance_with_volumes(
            vec![],
            vec![VolumeMount {
                name: "evil".into(),
                mount_path: "/etc/passwd".into(),
                ..Default::default()
            }],
        );
        let err = validate_user_pod_shape_for_test(&inst, None, None)
            .expect_err("/etc/passwd mount must be rejected");
        assert!(err.to_string().contains("spec.volumeMounts"));
    }

    #[test]
    fn validate_pod_shape_rejects_foreign_dnssec_keys_secret() {
        // H2: spec.dnssec.signing.keysFrom.secretRef mounts an arbitrary
        // in-namespace Secret, bypassing the volume allow-list and the
        // pod-shape admission policy. A secret name without the required
        // `bindy-` prefix (e.g. another tenant's RNDC key) must be rejected.
        let mut inst = instance_with_volumes(vec![], vec![]);
        inst.spec.config = Some(
            serde_json::from_value(serde_json::json!({
                "dnssec": {
                    "signing": {
                        "enabled": true,
                        "keysFrom": { "secretRef": { "name": "production-primary-rndc-key" } }
                    }
                }
            }))
            .expect("valid Bind9Config json"),
        );
        let err = validate_user_pod_shape_for_test(&inst, None, None)
            .expect_err("foreign DNSSEC keys secret must be rejected");
        // `{:#}` renders the full anyhow context chain (outer context + source).
        let msg = format!("{err:#}");
        assert!(msg.contains("keysFrom"), "{msg}");
        assert!(msg.contains("production-primary-rndc-key"), "{msg}");
    }

    #[test]
    fn validate_pod_shape_accepts_bindy_prefixed_dnssec_keys_secret() {
        // A tenant's own DNSSEC key Secret named with the `bindy-` prefix is
        // permitted, matching the user-secret volume allow-list.
        let mut inst = instance_with_volumes(vec![], vec![]);
        inst.spec.config = Some(
            serde_json::from_value(serde_json::json!({
                "dnssec": {
                    "signing": {
                        "enabled": true,
                        "keysFrom": { "secretRef": { "name": "bindy-my-dnssec-keys" } }
                    }
                }
            }))
            .expect("valid Bind9Config json"),
        );
        validate_user_pod_shape_for_test(&inst, None, None)
            .expect("bindy- prefixed DNSSEC keys secret must be accepted");
    }

    #[test]
    fn validate_pod_shape_rejects_hostpath_inherited_from_cluster() {
        let inst = instance_with_volumes(vec![], vec![]);
        let cluster = Bind9Cluster {
            metadata: ObjectMeta {
                name: Some("malicious-cluster".to_string()),
                namespace: Some("tenant-a".to_string()),
                ..Default::default()
            },
            spec: Bind9ClusterSpec {
                common: Bind9ClusterCommonSpec {
                    version: None,
                    primary: None,
                    secondary: None,
                    image: None,
                    config_map_refs: None,
                    global: None,
                    rndc_secret_refs: None,
                    acls: None,
                    volumes: Some(vec![Volume {
                        name: "host".into(),
                        host_path: Some(HostPathVolumeSource {
                            path: "/var/lib/kubelet".into(),
                            type_: None,
                        }),
                        ..Default::default()
                    }]),
                    volume_mounts: None,
                },
            },
            status: None,
        };
        let err = validate_user_pod_shape_for_test(&inst, Some(&cluster), None)
            .expect_err("inherited hostPath must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("Bind9Cluster tenant-a/malicious-cluster spec.volumes"),
            "{msg}"
        );
    }

    // ------------------------------------------------------------------
    // validate_user_pod_shape — placement
    // ------------------------------------------------------------------
    //
    // Placement is validated at EVERY level, not just the winning one, so a
    // user is told about a bad block even while a more specific block shadows
    // it. Without these, a bad role-level block on a cluster whose instance
    // overrides placement would reconcile silently and only surface later.

    mod placement_validation {
        use super::{Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, Bind9Instance};
        use crate::crd::{PlacementConfig, PrimaryConfig, SecondaryConfig, SpreadRule};
        use crate::reconcilers::bind9instance::resources::validate_user_pod_shape_for_test;

        fn rule(topology_key: &str) -> SpreadRule {
            SpreadRule {
                topology_key: topology_key.into(),
                max_skew: None,
                when_unsatisfiable: None,
                scope: None,
                min_domains: None,
                node_affinity_policy: None,
                node_taints_policy: None,
            }
        }

        fn bad_placement() -> PlacementConfig {
            // Duplicate (topologyKey, whenUnsatisfiable): Kubernetes requires
            // the pair to be unique and rejects the Pod outright.
            PlacementConfig {
                spread: Some(vec![
                    rule("topology.kubernetes.io/zone"),
                    rule("topology.kubernetes.io/zone"),
                ]),
            }
        }

        fn instance(placement: Option<PlacementConfig>) -> Bind9Instance {
            let mut i = super::instance_with_volumes(vec![], vec![]);
            i.spec.placement = placement;
            i
        }

        fn cluster(common: Bind9ClusterCommonSpec) -> Bind9Cluster {
            Bind9Cluster::new("my-dns", Bind9ClusterSpec { common })
        }

        fn empty_common() -> Bind9ClusterCommonSpec {
            Bind9ClusterCommonSpec {
                version: Some("9.18".into()),
                primary: None,
                secondary: None,
                image: None,
                config_map_refs: None,
                global: None,
                rndc_secret_refs: None,
                acls: None,
                volumes: None,
                volume_mounts: None,
            }
        }

        #[test]
        fn accepts_a_valid_placement_block() {
            let inst = instance(Some(PlacementConfig {
                spread: Some(vec![rule("topology.kubernetes.io/zone")]),
            }));
            assert!(validate_user_pod_shape_for_test(&inst, None, None).is_ok());
        }

        #[test]
        fn accepts_an_absent_placement_block() {
            let inst = instance(None);
            assert!(validate_user_pod_shape_for_test(&inst, None, None).is_ok());
        }

        #[test]
        fn rejects_a_bad_instance_level_block() {
            let inst = instance(Some(bad_placement()));
            let err = validate_user_pod_shape_for_test(&inst, None, None)
                .expect_err("duplicate spread rules must be rejected");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("spec.placement"),
                "error must name the offending field: {msg}"
            );
        }

        #[test]
        fn rejects_a_bad_role_level_block_even_when_the_instance_overrides_it() {
            // The instance sets its own (valid) placement, so the cluster block
            // never reaches a Pod — but it is still wrong and the user should
            // hear about it now rather than after an unrelated edit.
            let inst = instance(Some(PlacementConfig {
                spread: Some(vec![rule("topology.kubernetes.io/zone")]),
            }));
            let mut common = empty_common();
            common.primary = Some(PrimaryConfig {
                placement: Some(bad_placement()),
                ..Default::default()
            });
            let c = cluster(common);

            let err = validate_user_pod_shape_for_test(&inst, Some(&c), None)
                .expect_err("a shadowed but invalid role block must still be rejected");
            assert!(
                format!("{err:#}").contains("spec.primary.placement"),
                "error must name the role block: {err:#}"
            );
        }

        #[test]
        fn rejects_a_bad_secondary_block() {
            let inst = instance(None);
            let mut common = empty_common();
            common.secondary = Some(SecondaryConfig {
                placement: Some(bad_placement()),
                ..Default::default()
            });
            let c = cluster(common);

            let err = validate_user_pod_shape_for_test(&inst, Some(&c), None)
                .expect_err("an invalid secondary block must be rejected");
            assert!(
                format!("{err:#}").contains("spec.secondary.placement"),
                "error must name the role block: {err:#}"
            );
        }
    }

    // ==================================================================
    // Placement convergence
    // ==================================================================
    //
    // This is the machinery that decides whether a live Deployment gets a
    // scheduling change at all, and how that change is expressed as a patch.
    // It is the part most likely to break silently: a regression here does not
    // fail loudly, it just leaves a running Deployment with stale constraints
    // while every other test stays green.

    mod placement_convergence {
        use crate::bind9_resources::build_deployment;
        use crate::crd::{
            Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, Bind9Instance,
            Bind9InstanceSpec, PlacementConfig, PrimaryConfig, ServerRole, SpreadRule,
            WhenUnsatisfiable,
        };
        use crate::reconcilers::bind9instance::deployment_labels_are_current_for_test as deployment_labels_are_current;
        use crate::reconcilers::bind9instance::resources::{
            build_placement_patch_for_test as build_placement_patch,
            deployment_needs_update_for_test as deployment_needs_update,
        };
        use k8s_openapi::api::apps::v1::Deployment;
        use kube::api::ObjectMeta;

        fn instance(name: &str, cluster_ref: &str) -> Bind9Instance {
            #[allow(deprecated)]
            Bind9Instance {
                metadata: ObjectMeta {
                    name: Some(name.into()),
                    namespace: Some("dns".into()),
                    ..Default::default()
                },
                spec: Bind9InstanceSpec {
                    cluster_ref: cluster_ref.to_string(),
                    role: ServerRole::Primary,
                    replicas: Some(1),
                    version: Some("9.18".into()),
                    image: None,
                    config_map_refs: None,
                    config: None,
                    primary_servers: None,
                    volumes: None,
                    volume_mounts: None,
                    rndc_secret_ref: None,
                    rndc_key: None,
                    storage: None,
                    placement: None,
                    bindcar_config: None,
                },
                status: None,
            }
        }

        fn cluster(primaries: i32, placement: Option<PlacementConfig>) -> Bind9Cluster {
            Bind9Cluster::new(
                "my-dns",
                Bind9ClusterSpec {
                    common: Bind9ClusterCommonSpec {
                        version: Some("9.18".into()),
                        primary: Some(PrimaryConfig {
                            replicas: Some(primaries),
                            placement,
                            ..Default::default()
                        }),
                        secondary: None,
                        image: None,
                        config_map_refs: None,
                        global: None,
                        rndc_secret_refs: None,
                        acls: None,
                        volumes: None,
                        volume_mounts: None,
                    },
                },
            )
        }

        fn deployment_for(cluster_spec: Option<&Bind9Cluster>) -> Deployment {
            let inst = instance("my-dns-primary-0", "my-dns");
            build_deployment(
                "my-dns-primary-0",
                "dns",
                &inst,
                cluster_spec,
                None,
                "rndc-key",
            )
        }

        fn zone_rule(when: WhenUnsatisfiable) -> PlacementConfig {
            PlacementConfig {
                spread: Some(vec![SpreadRule {
                    topology_key: "topology.kubernetes.io/zone".into(),
                    max_skew: Some(1),
                    when_unsatisfiable: Some(when),
                    scope: None,
                    min_domains: None,
                    node_affinity_policy: None,
                    node_taints_policy: None,
                }]),
            }
        }

        // ---------------- deployment_needs_update ----------------

        #[test]
        fn detects_a_constraint_being_added() {
            // The upgrade case: a Deployment created before this feature has no
            // constraints, and must be recognised as stale.
            let current = deployment_for(Some(&cluster(1, None)));
            let desired = deployment_for(Some(&cluster(3, None)));
            assert!(current
                .spec
                .as_ref()
                .unwrap()
                .template
                .spec
                .as_ref()
                .unwrap()
                .topology_spread_constraints
                .is_none());
            assert!(deployment_needs_update(&current, &desired));
        }

        #[test]
        fn detects_a_constraint_being_changed() {
            let current = deployment_for(Some(&cluster(
                3,
                Some(zone_rule(WhenUnsatisfiable::ScheduleAnyway)),
            )));
            let desired = deployment_for(Some(&cluster(
                3,
                Some(zone_rule(WhenUnsatisfiable::DoNotSchedule)),
            )));
            assert!(deployment_needs_update(&current, &desired));
        }

        #[test]
        fn detects_a_constraint_being_removed() {
            // `spread: []` must be seen as a change, or the opt-out never takes.
            let current = deployment_for(Some(&cluster(3, None)));
            let desired = deployment_for(Some(&cluster(
                3,
                Some(PlacementConfig {
                    spread: Some(vec![]),
                }),
            )));
            assert!(deployment_needs_update(&current, &desired));
        }

        #[test]
        fn detects_stale_pod_template_labels() {
            let desired = deployment_for(Some(&cluster(3, None)));
            let mut current = desired.clone();
            current
                .spec
                .as_mut()
                .unwrap()
                .template
                .metadata
                .as_mut()
                .unwrap()
                .labels
                .as_mut()
                .unwrap()
                .remove("bindy.firestoned.io/cluster");
            assert!(
                deployment_needs_update(&current, &desired),
                "a Deployment missing the cluster Pod label must be reconciled"
            );
        }

        #[test]
        fn reports_no_update_when_nothing_changed() {
            // Guards against a hot-patch loop: if this ever returns true for an
            // unchanged pair, the operator rewrites the Deployment on every
            // reconcile and rolls the DNS pods every five minutes.
            let c = cluster(3, Some(zone_rule(WhenUnsatisfiable::DoNotSchedule)));
            let current = deployment_for(Some(&c));
            let desired = deployment_for(Some(&c));
            assert!(!deployment_needs_update(&current, &desired));
        }

        // ---------------- build_placement_patch ----------------

        #[test]
        fn patch_replaces_rather_than_merges_the_constraint_list() {
            // topologySpreadConstraints has patchStrategy=merge with
            // patchMergeKey=topologyKey. Without the `$patch: replace`
            // directive, a strategic merge patch MERGES by key and a removed
            // constraint survives on the live Deployment.
            let desired = deployment_for(Some(&cluster(3, None)));
            let patch = build_placement_patch(&desired);
            let list = patch["topologySpreadConstraints"].as_array().unwrap();

            assert_eq!(
                list[0],
                serde_json::json!({"$patch": "replace"}),
                "the replace directive must be the first element"
            );
            assert_eq!(list.len(), 2, "directive + one generated constraint");
            assert_eq!(
                list[1]["topologyKey"].as_str(),
                Some("topology.kubernetes.io/zone")
            );
        }

        #[test]
        fn patch_nulls_the_field_when_no_constraints_are_desired() {
            // This is the removal path. Sending an empty list would leave the
            // field present-but-empty; only an explicit null deletes it.
            let desired = deployment_for(Some(&cluster(
                3,
                Some(PlacementConfig {
                    spread: Some(vec![]),
                }),
            )));
            let patch = build_placement_patch(&desired);
            assert!(
                patch["topologySpreadConstraints"].is_null(),
                "removal must send null, got {}",
                patch["topologySpreadConstraints"]
            );
        }

        #[test]
        fn patch_carries_every_rule_in_order() {
            let mut config = zone_rule(WhenUnsatisfiable::DoNotSchedule);
            config.spread.as_mut().unwrap().push(SpreadRule {
                topology_key: "kubernetes.io/hostname".into(),
                max_skew: Some(1),
                when_unsatisfiable: Some(WhenUnsatisfiable::ScheduleAnyway),
                scope: None,
                min_domains: None,
                node_affinity_policy: None,
                node_taints_policy: None,
            });

            let desired = deployment_for(Some(&cluster(3, Some(config))));
            let list = build_placement_patch(&desired)["topologySpreadConstraints"]
                .as_array()
                .unwrap()
                .clone();

            assert_eq!(list.len(), 3, "directive + two rules");
            assert_eq!(
                list[1]["topologyKey"].as_str(),
                Some("topology.kubernetes.io/zone")
            );
            assert_eq!(
                list[2]["topologyKey"].as_str(),
                Some("kubernetes.io/hostname")
            );
        }

        // ---------------- reconcile short-circuit ----------------

        #[test]
        fn label_check_passes_for_a_freshly_built_deployment() {
            let inst = instance("my-dns-primary-0", "my-dns");
            let deployment = deployment_for(Some(&cluster(3, None)));
            assert!(deployment_labels_are_current(
                &deployment,
                "my-dns-primary-0",
                &inst
            ));
        }

        #[test]
        fn label_check_fails_when_the_pod_template_predates_the_feature() {
            // THE upgrade regression. Metadata labels are correct (they never
            // changed), only the Pod template lacks the cluster label. Checking
            // metadata alone would skip this Deployment forever.
            let inst = instance("my-dns-primary-0", "my-dns");
            let mut deployment = deployment_for(Some(&cluster(3, None)));
            deployment
                .spec
                .as_mut()
                .unwrap()
                .template
                .metadata
                .as_mut()
                .unwrap()
                .labels
                .as_mut()
                .unwrap()
                .remove("bindy.firestoned.io/cluster");

            assert!(
                !deployment_labels_are_current(&deployment, "my-dns-primary-0", &inst),
                "stale Pod template labels must force reconciliation"
            );
        }

        #[test]
        fn label_check_tolerates_foreign_labels() {
            // Other controllers, kubectl and Helm add labels of their own; the
            // check is a subset test, not equality.
            let inst = instance("my-dns-primary-0", "my-dns");
            let mut deployment = deployment_for(Some(&cluster(3, None)));
            for target in [
                deployment.metadata.labels.as_mut().unwrap(),
                deployment
                    .spec
                    .as_mut()
                    .unwrap()
                    .template
                    .metadata
                    .as_mut()
                    .unwrap()
                    .labels
                    .as_mut()
                    .unwrap(),
            ] {
                target.insert("argocd.argoproj.io/instance".into(), "someapp".into());
            }
            assert!(deployment_labels_are_current(
                &deployment,
                "my-dns-primary-0",
                &inst
            ));
        }

        #[test]
        fn label_check_fails_when_a_deployment_has_no_labels_at_all() {
            let inst = instance("my-dns-primary-0", "my-dns");
            let mut deployment = deployment_for(Some(&cluster(3, None)));
            deployment.metadata.labels = None;
            assert!(!deployment_labels_are_current(
                &deployment,
                "my-dns-primary-0",
                &inst
            ));
        }
    }
}
