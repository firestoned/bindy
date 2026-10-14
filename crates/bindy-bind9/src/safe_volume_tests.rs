// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod tests {
    use super::super::*;
    use k8s_openapi::api::core::v1::{
        ConfigMapVolumeSource, EmptyDirVolumeSource, HostPathVolumeSource,
        PersistentVolumeClaimVolumeSource, ProjectedVolumeSource, SecretVolumeSource, Volume,
        VolumeMount,
    };

    // ------------------------------------------------------------------
    // Volume validators — happy paths
    // ------------------------------------------------------------------

    #[test]
    fn empty_dir_volume_is_accepted() {
        let v = Volume {
            name: "scratch".into(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        };
        validate_user_volumes(&[v]).expect("emptyDir must be accepted");
    }

    #[test]
    fn configmap_volume_with_bindy_prefix_is_accepted() {
        let v = Volume {
            name: "extra-conf".into(),
            config_map: Some(ConfigMapVolumeSource {
                name: "bindy-extra-options".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };
        validate_user_volumes(&[v]).expect("bindy-prefixed configMap must be accepted");
    }

    #[test]
    fn secret_volume_with_bindy_prefix_is_accepted() {
        let v = Volume {
            name: "extra-key".into(),
            secret: Some(SecretVolumeSource {
                secret_name: Some("bindy-extra-key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        validate_user_volumes(&[v]).expect("bindy-prefixed secret must be accepted");
    }

    #[test]
    fn pvc_volume_with_bindy_prefix_is_accepted() {
        let v = Volume {
            name: "data".into(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: "bindy-zone-data".to_string(),
                read_only: Some(false),
            }),
            ..Default::default()
        };
        validate_user_volumes(&[v]).expect("bindy-prefixed PVC must be accepted");
    }

    #[test]
    fn empty_volume_list_is_accepted() {
        validate_user_volumes(&[]).expect("empty list is trivially valid");
    }

    // ------------------------------------------------------------------
    // Volume validators — rejections
    // ------------------------------------------------------------------

    #[test]
    fn host_path_volume_is_rejected() {
        let v = Volume {
            name: "host-root".into(),
            host_path: Some(HostPathVolumeSource {
                path: "/".to_string(),
                type_: Some("Directory".to_string()),
            }),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("hostPath must be rejected");
        assert!(matches!(err, VolumeRejection::ForbiddenSource { .. }));
        assert!(err.to_string().contains("hostPath"));
    }

    #[test]
    fn projected_volume_is_rejected() {
        let v = Volume {
            name: "projected".into(),
            projected: Some(ProjectedVolumeSource::default()),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("projected must be rejected");
        assert!(matches!(err, VolumeRejection::ForbiddenSource { .. }));
    }

    #[test]
    fn foreign_secret_volume_is_rejected() {
        let v = Volume {
            name: "stolen".into(),
            secret: Some(SecretVolumeSource {
                secret_name: Some("production-primary-rndc-key".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("foreign secret must be rejected");
        assert!(matches!(err, VolumeRejection::SecretNamePrefix { .. }));
    }

    #[test]
    fn secret_volume_without_secret_name_is_rejected() {
        // SecretVolumeSource with secret_name=None should be rejected
        // (cannot prove it complies with the prefix rule).
        let v = Volume {
            name: "ambiguous".into(),
            secret: Some(SecretVolumeSource {
                secret_name: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v])
            .expect_err("secret volume without secretName must be rejected");
        assert!(matches!(err, VolumeRejection::SecretNamePrefix { .. }));
    }

    #[test]
    fn foreign_configmap_volume_is_rejected() {
        let v = Volume {
            name: "evil-cm".into(),
            config_map: Some(ConfigMapVolumeSource {
                name: "kube-root-ca.crt".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("foreign configMap must be rejected");
        assert!(matches!(err, VolumeRejection::ConfigMapNamePrefix { .. }));
    }

    #[test]
    fn foreign_pvc_volume_is_rejected() {
        let v = Volume {
            name: "evil-pvc".into(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: "production-data".to_string(),
                read_only: Some(false),
            }),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("foreign PVC must be rejected");
        assert!(matches!(err, VolumeRejection::PvcNamePrefix { .. }));
    }

    #[test]
    fn volume_with_no_recognised_source_is_rejected() {
        // Default Volume has every source field as None — reject so we don't
        // silently accept future Volume variants.
        let v = Volume {
            name: "empty".into(),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("source-less volume must be rejected");
        assert!(matches!(err, VolumeRejection::ForbiddenSource { .. }));
    }

    // ------------------------------------------------------------------
    // VolumeMount validators
    // ------------------------------------------------------------------

    #[test]
    fn mount_under_data_is_accepted() {
        let m = VolumeMount {
            name: "scratch".into(),
            mount_path: "/data/zones".into(),
            ..Default::default()
        };
        validate_user_volume_mounts(&[m]).expect("mount under /data/ must be accepted");
    }

    #[test]
    fn mount_under_var_log_bind_is_accepted() {
        let m = VolumeMount {
            name: "logs".into(),
            mount_path: "/var/log/bind/audit".into(),
            ..Default::default()
        };
        validate_user_volume_mounts(&[m]).expect("mount under /var/log/bind/ must be accepted");
    }

    #[test]
    fn mount_at_etc_passwd_is_rejected() {
        let m = VolumeMount {
            name: "evil".into(),
            mount_path: "/etc/passwd".into(),
            ..Default::default()
        };
        let err =
            validate_user_volume_mounts(&[m]).expect_err("/etc/passwd mount must be rejected");
        assert!(matches!(
            err,
            VolumeRejection::MountPathOutsideAllowList { .. }
        ));
    }

    #[test]
    fn mount_overlapping_etc_bind_is_rejected() {
        // Overlapping the operator-managed /etc/bind is the simplest way to
        // shadow the rndc.key Secret.
        let m = VolumeMount {
            name: "shadow".into(),
            mount_path: "/etc/bind/keys".into(),
            ..Default::default()
        };
        let err = validate_user_volume_mounts(&[m]).expect_err("/etc/bind mount must be rejected");
        assert!(matches!(
            err,
            VolumeRejection::MountPathOutsideAllowList { .. }
        ));
    }

    #[test]
    fn mount_path_with_traversal_escaping_allowed_prefix_is_rejected() {
        // C1: `/data/../etc/bind/named.conf` starts with the allowed `/data/`
        // prefix but resolves to the operator-owned named.conf. A bare
        // starts_with() check accepts it; we must reject any `..` in mountPath.
        let m = VolumeMount {
            name: "pwn".into(),
            mount_path: "/data/../etc/bind/named.conf".into(),
            sub_path: Some("named.conf".to_string()),
            ..Default::default()
        };
        let err = validate_user_volume_mounts(&[m])
            .expect_err("mountPath containing '..' must be rejected");
        assert!(matches!(err, VolumeRejection::MountPathTraversal { .. }));
    }

    #[test]
    fn mount_path_with_traversal_before_prefix_is_rejected() {
        // Defense in depth: reject `..` even when it appears deeper in the path.
        let m = VolumeMount {
            name: "pwn2".into(),
            mount_path: "/var/log/bind/../../etc/bind/keys".into(),
            ..Default::default()
        };
        let err = validate_user_volume_mounts(&[m])
            .expect_err("mountPath containing '..' must be rejected");
        assert!(matches!(err, VolumeRejection::MountPathTraversal { .. }));
    }

    #[test]
    fn subpath_with_traversal_is_rejected() {
        let m = VolumeMount {
            name: "tricky".into(),
            mount_path: "/data/zones".into(),
            sub_path: Some("../../etc/passwd".to_string()),
            ..Default::default()
        };
        let err =
            validate_user_volume_mounts(&[m]).expect_err("subPath traversal must be rejected");
        assert!(matches!(err, VolumeRejection::SubPathTraversal { .. }));
    }

    #[test]
    fn subpath_expr_with_traversal_is_rejected() {
        let m = VolumeMount {
            name: "tricky2".into(),
            mount_path: "/data/zones".into(),
            sub_path_expr: Some("$(EVIL)/../../etc/passwd".to_string()),
            ..Default::default()
        };
        let err =
            validate_user_volume_mounts(&[m]).expect_err("subPathExpr traversal must be rejected");
        assert!(matches!(err, VolumeRejection::SubPathTraversal { .. }));
    }

    #[test]
    fn dnssec_key_secret_with_bindy_prefix_is_accepted() {
        validate_dnssec_key_secret_name("bindy-my-keys")
            .expect("bindy- prefixed DNSSEC key secret must be accepted");
    }

    #[test]
    fn dnssec_key_secret_without_prefix_is_rejected() {
        let err = validate_dnssec_key_secret_name("production-primary-rndc-key")
            .expect_err("foreign DNSSEC key secret must be rejected");
        assert!(matches!(err, VolumeRejection::DnssecKeySecretPrefix { .. }));
    }

    // ------------------------------------------------------------------
    // A true allow-list: exactly one source, and that one on the list
    // ------------------------------------------------------------------

    #[test]
    fn an_allowed_source_set_together_with_an_unlisted_one_is_rejected() {
        // `image` (Kubernetes 1.31) is on no deny list: the allow-list used
        // to return on the first allowed source and let it through.
        let v = Volume {
            name: "sneaky".into(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            image: Some(k8s_openapi::api::core::v1::ImageVolumeSource {
                reference: Some("registry.example/tools:1".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("two sources must be rejected");
        assert!(
            matches!(err, VolumeRejection::MultipleSources { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn two_allowed_sources_in_one_volume_are_rejected() {
        let v = Volume {
            name: "both".into(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            secret: Some(SecretVolumeSource {
                secret_name: Some("bindy-extra".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(matches!(
            validate_user_volumes(&[v]),
            Err(VolumeRejection::MultipleSources { .. })
        ));
    }

    #[test]
    fn an_unlisted_source_is_rejected_by_its_name() {
        let v = Volume {
            name: "img".into(),
            image: Some(k8s_openapi::api::core::v1::ImageVolumeSource::default()),
            ..Default::default()
        };
        let err = validate_user_volumes(&[v]).expect_err("image must be rejected");
        assert_eq!(
            err,
            VolumeRejection::ForbiddenSource {
                name: "img".into(),
                kind: "image".into()
            }
        );
    }

    #[test]
    fn a_volume_with_no_source_is_rejected() {
        let v = Volume {
            name: "nothing".into(),
            ..Default::default()
        };
        assert!(matches!(
            validate_user_volumes(&[v]),
            Err(VolumeRejection::ForbiddenSource { .. })
        ));
    }

    #[test]
    fn rndc_key_secret_with_bindy_prefix_is_accepted() {
        validate_rndc_key_secret_name("bindy-corp-rndc")
            .expect("bindy- prefixed RNDC key secret must be accepted");
    }

    #[test]
    fn rndc_key_secret_without_prefix_is_rejected() {
        let err = validate_rndc_key_secret_name("other-tenant-tsig")
            .expect_err("foreign RNDC key secret must be rejected");
        assert!(matches!(err, VolumeRejection::RndcKeySecretPrefix { .. }));
    }

    #[test]
    fn empty_mount_list_is_accepted() {
        validate_user_volume_mounts(&[]).expect("empty list is trivially valid");
    }

    // ------------------------------------------------------------------
    // Optional helpers — exercise the Option<&Vec<...>> wrappers
    // ------------------------------------------------------------------

    #[test]
    fn validate_optional_volumes_none_is_ok() {
        validate_optional_user_volumes(None).expect("None must be accepted");
    }

    #[test]
    fn validate_optional_mounts_none_is_ok() {
        validate_optional_user_volume_mounts(None).expect("None must be accepted");
    }

    #[test]
    fn validate_optional_volumes_some_propagates_error() {
        let bad = vec![Volume {
            name: "x".into(),
            host_path: Some(HostPathVolumeSource {
                path: "/".into(),
                type_: None,
            }),
            ..Default::default()
        }];
        validate_optional_user_volumes(Some(&bad))
            .expect_err("Some(bad) must propagate the rejection");
    }
}
