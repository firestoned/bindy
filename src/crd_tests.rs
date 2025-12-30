// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#[cfg(test)]
#[allow(clippy::unreadable_literal)]
mod tests {
    use crate::crd::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_label_selector_default() {
        let selector = LabelSelector::default();
        assert!(selector.match_labels.is_none());
        assert!(selector.match_expressions.is_none());
    }

    #[test]
    fn test_label_selector_with_match_labels() {
        let mut labels = BTreeMap::new();
        labels.insert("app".into(), "dns".into());
        labels.insert("env".into(), "prod".into());

        let selector = LabelSelector {
            match_labels: Some(labels.clone()),
            match_expressions: None,
        };

        assert!(selector.match_labels.is_some());
        assert_eq!(selector.match_labels.unwrap().len(), 2);
    }

    #[test]
    fn test_label_selector_requirement() {
        let req = LabelSelectorRequirement {
            key: "environment".into(),
            operator: "In".into(),
            values: Some(vec!["prod".into(), "staging".into()]),
        };

        assert_eq!(req.key, "environment");
        assert_eq!(req.operator, "In");
        assert!(req.values.is_some());
        assert_eq!(req.values.unwrap().len(), 2);
    }

    #[test]
    fn test_soa_record() {
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        assert_eq!(soa.primary_ns, "ns1.example.com.");
        assert_eq!(soa.admin_email, "admin@example.com");
        assert_eq!(soa.serial, 2024010101);
    }

    #[test]
    fn test_condition() {
        let condition = Condition {
            r#type: "Ready".into(),
            status: "True".into(),
            reason: Some("ReconcileSuccess".into()),
            message: Some("Zone created successfully".into()),
            last_transition_time: Some("2024-01-01T00:00:00Z".into()),
        };

        assert_eq!(condition.r#type, "Ready");
        assert_eq!(condition.status, "True");
        assert!(condition.reason.is_some());
        assert!(condition.message.is_some());
    }

    #[test]
    fn test_dnszone_status_default() {
        let status = DNSZoneStatus::default();
        assert!(status.conditions.is_empty());
        assert!(status.observed_generation.is_none());
        assert!(status.record_count.is_none());
    }

    #[test]
    fn test_tsig_key() {
        let tsig = TSIGKey {
            name: "transfer-key".into(),
            algorithm: RndcAlgorithm::HmacSha256,
            secret: "base64secret==".into(),
        };

        assert_eq!(tsig.name, "transfer-key");
        assert_eq!(tsig.algorithm, RndcAlgorithm::HmacSha256);
        assert_eq!(tsig.secret, "base64secret==");
    }

    #[test]
    fn test_server_role() {
        let primary = ServerRole::Primary;
        let secondary = ServerRole::Secondary;

        // Test that the enum variants exist
        match primary {
            ServerRole::Primary => {}
            ServerRole::Secondary => panic!("Wrong variant"),
        }

        match secondary {
            ServerRole::Primary => panic!("Wrong variant"),
            ServerRole::Secondary => {}
        }
    }

    #[test]
    fn test_dnszone_spec() {
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let spec = DNSZoneSpec {
            zone_name: "example.com".into(),
            cluster_ref: Some("my-cluster".into()),
            cluster_provider_ref: None,
            soa_record: soa,
            ttl: Some(3600),
            name_server_ips: None,
            records_from: None,
        };

        assert_eq!(spec.zone_name, "example.com");
        assert_eq!(spec.cluster_ref, Some("my-cluster".into()));
        assert_eq!(spec.soa_record.primary_ns, "ns1.example.com.");
        assert_eq!(spec.ttl.unwrap(), 3600);
    }

    #[test]
    fn test_bind9cluster_spec() {
        let mut acls = BTreeMap::new();
        acls.insert("trusted".into(), vec!["10.0.0.0/8".into()]);

        let spec = Bind9ClusterSpec {
            common: Bind9ClusterCommonSpec {
                version: Some("9.18".into()),
                image: None,
                config_map_refs: None,
                global: Some(Bind9Config {
                    recursion: Some(false),
                    allow_query: None,
                    allow_transfer: None,
                    dnssec: None,
                    forwarders: None,
                    listen_on: None,
                    listen_on_v6: None,
                    rndc_secret_ref: None,
                    bindcar_config: None,
                }),
                primary: None,
                secondary: None,
                rndc_secret_refs: Some(vec![RndcSecretRef {
                    name: "rndc-key-secret".into(),
                    algorithm: RndcAlgorithm::HmacSha256,
                    key_name_key: "key-name".into(),
                    secret_key: "secret".into(),
                }]),
                acls: Some(acls.clone()),
                volumes: None,
                volume_mounts: None,
                zones_from: None,
            },
        };

        assert_eq!(spec.common.version.unwrap(), "9.18");
        assert!(spec.common.global.is_some());
        assert_eq!(spec.common.rndc_secret_refs.as_ref().unwrap().len(), 1);
        assert!(spec.common.acls.is_some());
        assert_eq!(spec.common.acls.unwrap().get("trusted").unwrap().len(), 1);
    }

    #[test]
    fn test_arecord_spec() {
        let spec = ARecordSpec {
            name: "www".into(),
            ipv4_address: "192.0.2.1".into(),
            ttl: Some(300),
        };

        assert_eq!(spec.name, "www");
        assert_eq!(spec.ipv4_address, "192.0.2.1");
        assert_eq!(spec.ttl.unwrap(), 300);
    }

    #[test]
    fn test_aaaarecord_spec() {
        let spec = AAAARecordSpec {
            name: "www".into(),
            ipv6_address: "2001:db8::1".into(),
            ttl: Some(300),
        };

        assert_eq!(spec.ipv6_address, "2001:db8::1");
    }

    #[test]
    fn test_txtrecord_spec() {
        let spec = TXTRecordSpec {
            name: "@".into(),
            text: vec!["v=spf1 mx ~all".into()],
            ttl: Some(3600),
        };

        assert_eq!(spec.text.len(), 1);
        assert_eq!(spec.text[0], "v=spf1 mx ~all");
    }

    #[test]
    fn test_cnamerecord_spec() {
        let spec = CNAMERecordSpec {
            name: "blog".into(),
            target: "www.example.com.".into(),
            ttl: Some(300),
        };

        assert_eq!(spec.target, "www.example.com.");
    }

    #[test]
    fn test_mxrecord_spec() {
        let spec = MXRecordSpec {
            name: "@".into(),
            priority: 10,
            mail_server: "mail.example.com.".into(),
            ttl: Some(3600),
        };

        assert_eq!(spec.priority, 10);
        assert_eq!(spec.mail_server, "mail.example.com.");
    }

    #[test]
    fn test_nsrecord_spec() {
        let spec = NSRecordSpec {
            name: "@".into(),
            nameserver: "ns1.example.com.".into(),
            ttl: Some(3600),
        };

        assert_eq!(spec.nameserver, "ns1.example.com.");
    }

    #[test]
    fn test_srvrecord_spec() {
        let spec = SRVRecordSpec {
            name: "_sip._tcp".into(),
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sipserver.example.com.".into(),
            ttl: Some(3600),
        };

        assert_eq!(spec.priority, 10);
        assert_eq!(spec.weight, 60);
        assert_eq!(spec.port, 5060);
        assert_eq!(spec.target, "sipserver.example.com.");
    }

    #[test]
    fn test_caarecord_spec() {
        let spec = CAARecordSpec {
            name: "@".into(),
            flags: 0,
            tag: "issue".into(),
            value: "letsencrypt.org".into(),
            ttl: Some(3600),
        };

        assert_eq!(spec.flags, 0);
        assert_eq!(spec.tag, "issue");
        assert_eq!(spec.value, "letsencrypt.org");
    }

    #[test]
    fn test_record_status_default() {
        let status = RecordStatus::default();
        assert!(status.conditions.is_empty());
        assert!(status.observed_generation.is_none());
    }

    #[test]
    fn test_bind9_config() {
        let config = Bind9Config {
            recursion: Some(false),
            allow_query: Some(vec!["0.0.0.0/0".into()]),
            allow_transfer: Some(vec!["10.0.0.0/8".into()]),
            dnssec: Some(DNSSECConfig {
                validation: Some(true),
            }),
            forwarders: Some(vec!["8.8.8.8".into(), "8.8.4.4".into()]),
            listen_on: Some(vec!["any".into()]),
            listen_on_v6: Some(vec!["any".into()]),
            rndc_secret_ref: None,
            bindcar_config: None,
        };

        assert_eq!(config.recursion, Some(false));
        assert!(config.allow_query.is_some());
        assert!(config.dnssec.is_some());
        assert_eq!(config.forwarders.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_dnssec_config() {
        let config = DNSSECConfig {
            validation: Some(false),
        };

        assert_eq!(config.validation, Some(false));
    }

    #[test]
    fn test_bind9instance_spec() {
        let spec = Bind9InstanceSpec {
            cluster_ref: "my-cluster".into(),
            role: ServerRole::Primary,
            replicas: Some(3),
            version: Some("9.18".into()),
            image: None,
            config_map_refs: None,
            config: Some(Bind9Config {
                recursion: Some(false),
                allow_query: None,
                allow_transfer: None,
                dnssec: None,
                forwarders: None,
                listen_on: None,
                listen_on_v6: None,
                rndc_secret_ref: None,
                bindcar_config: None,
            }),
            primary_servers: None,
            volumes: None,
            volume_mounts: None,
            rndc_secret_ref: None,
            storage: None,
            bindcar_config: None,
            zones_from: None,
        };

        assert_eq!(spec.cluster_ref, "my-cluster");
        assert_eq!(spec.replicas, Some(3));
        assert_eq!(spec.version.unwrap(), "9.18");
        assert!(spec.config.is_some());
        assert!(spec.primary_servers.is_none());
    }

    #[test]
    fn test_bind9instance_status_default() {
        let status = Bind9InstanceStatus::default();
        assert!(status.conditions.is_empty());
        assert!(status.observed_generation.is_none());
        assert!(status.replicas.is_none());
        assert!(status.ready_replicas.is_none());
        assert!(status.service_address.is_none());
    }

    #[test]
    fn test_bind9instance_status_with_values() {
        let condition = Condition {
            r#type: "Ready".into(),
            status: "True".into(),
            reason: None,
            message: None,
            last_transition_time: None,
        };

        let status = Bind9InstanceStatus {
            conditions: vec![condition],
            observed_generation: Some(1),
            replicas: Some(3),
            ready_replicas: Some(2),
            service_address: Some("my-instance.dns-system.svc.cluster.local".into()),
            selected_zones: vec![],
        };

        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.replicas, Some(3));
        assert_eq!(status.ready_replicas, Some(2));
        assert_eq!(
            status.service_address.as_deref(),
            Some("my-instance.dns-system.svc.cluster.local")
        );
    }

    #[test]
    fn test_condition_types() {
        // Test all valid condition types as defined in CRD
        let valid_types = vec!["Ready", "Available", "Progressing", "Degraded", "Failed"];

        for condition_type in valid_types {
            let condition = Condition {
                r#type: condition_type.into(),
                status: "True".into(),
                reason: Some("Test".into()),
                message: Some("Test message".into()),
                last_transition_time: Some("2024-11-26T10:00:00Z".into()),
            };

            assert_eq!(condition.r#type, condition_type);
            assert_eq!(condition.status, "True");
            assert!(condition.reason.is_some());
            assert!(condition.message.is_some());
            assert!(condition.last_transition_time.is_some());
        }
    }

    #[test]
    fn test_condition_status_values() {
        // Test all valid status values
        let valid_statuses = vec!["True", "False", "Unknown"];

        for status_value in valid_statuses {
            let condition = Condition {
                r#type: "Ready".to_string(),
                status: status_value.into(),
                reason: None,
                message: None,
                last_transition_time: None,
            };

            assert_eq!(condition.status, status_value);
        }
    }

    #[test]
    fn test_condition_with_all_fields() {
        let condition = Condition {
            r#type: "Ready".into(),
            status: "True".into(),
            reason: Some("ResourceCreated".into()),
            message: Some("All resources successfully created".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        assert_eq!(condition.r#type, "Ready");
        assert_eq!(condition.status, "True");
        assert_eq!(condition.reason.as_deref(), Some("ResourceCreated"));
        assert_eq!(
            condition.message.as_deref(),
            Some("All resources successfully created")
        );
        assert_eq!(
            condition.last_transition_time.as_deref(),
            Some("2024-11-26T10:00:00Z")
        );
    }

    #[test]
    fn test_multiple_conditions() {
        let conditions = vec![
            Condition {
                r#type: "Ready".into(),
                status: "True".into(),
                reason: Some("Ready".into()),
                message: Some("Resource is ready".into()),
                last_transition_time: Some("2024-11-26T10:00:00Z".into()),
            },
            Condition {
                r#type: "Progressing".into(),
                status: "False".into(),
                reason: Some("Completed".into()),
                message: Some("Deployment complete".into()),
                last_transition_time: Some("2024-11-26T10:00:00Z".into()),
            },
        ];

        let status = Bind9InstanceStatus {
            conditions: conditions.clone(),
            observed_generation: Some(1),
            replicas: Some(3),
            ready_replicas: Some(3),
            service_address: None,
            selected_zones: vec![],
        };

        assert_eq!(status.conditions.len(), 2);
        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.conditions[1].r#type, "Progressing");
    }

    #[test]
    fn test_dnszone_status_with_conditions() {
        let condition = Condition {
            r#type: "Ready".into(),
            status: "True".into(),
            reason: Some("ZoneCreated".into()),
            message: Some("Zone file created for 2 instances".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        let status = DNSZoneStatus {
            conditions: vec![condition],
            observed_generation: Some(1),
            record_count: Some(5),
            secondary_ips: None,
            records: vec![],
            selection_method: None,
            selected_by_instance: None,
        };

        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.record_count, Some(5));
    }

    #[test]
    fn test_record_status_with_condition() {
        let condition = Condition {
            r#type: "Ready".into(),
            status: "True".into(),
            reason: Some("RecordCreated".into()),
            message: Some("DNS record added to zone".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        let status = RecordStatus {
            conditions: vec![condition],
            observed_generation: Some(1),
            zone: None,
            record_hash: None,
            last_updated: None,
        };

        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.conditions[0].r#type, "Ready");
        assert_eq!(status.observed_generation, Some(1));
    }

    #[test]
    fn test_degraded_condition() {
        let condition = Condition {
            r#type: "Degraded".into(),
            status: "True".into(),
            reason: Some("SomeReplicasDown".into()),
            message: Some("1 of 3 replicas is not ready".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        let status = Bind9InstanceStatus {
            conditions: vec![condition],
            observed_generation: Some(1),
            replicas: Some(3),
            ready_replicas: Some(2),
            service_address: None,
            selected_zones: vec![],
        };

        assert_eq!(status.conditions[0].r#type, "Degraded");
        assert_eq!(status.conditions[0].status, "True");
        assert_eq!(status.ready_replicas, Some(2));
    }

    #[test]
    fn test_failed_condition() {
        let condition = Condition {
            r#type: "Failed".into(),
            status: "True".into(),
            reason: Some("ResourceCreationFailed".into()),
            message: Some("Failed to create ConfigMap: permission denied".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        let status = Bind9InstanceStatus {
            conditions: vec![condition],
            observed_generation: Some(1),
            replicas: Some(0),
            ready_replicas: Some(0),
            service_address: None,
            selected_zones: vec![],
        };

        assert_eq!(status.conditions[0].r#type, "Failed");
        assert_eq!(status.conditions[0].status, "True");
        assert!(status.conditions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("permission denied"));
    }

    #[test]
    fn test_available_condition() {
        let condition = Condition {
            r#type: "Available".into(),
            status: "True".into(),
            reason: Some("MinimumReplicasAvailable".into()),
            message: Some("Deployment has minimum availability".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        assert_eq!(condition.r#type, "Available");
        assert_eq!(condition.status, "True");
    }

    #[test]
    fn test_progressing_condition() {
        let condition = Condition {
            r#type: "Progressing".into(),
            status: "True".into(),
            reason: Some("NewReplicaSetCreated".into()),
            message: Some("Deployment is progressing".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        assert_eq!(condition.r#type, "Progressing");
        assert_eq!(condition.status, "True");
    }

    #[test]
    fn test_condition_serialization() {
        use serde_json;

        let condition = Condition {
            r#type: "Ready".into(),
            status: "True".into(),
            reason: Some("AllGood".into()),
            message: Some("Everything is working".into()),
            last_transition_time: Some("2024-11-26T10:00:00Z".into()),
        };

        let json = serde_json::to_string(&condition).unwrap();
        assert!(json.contains("Ready"));
        assert!(json.contains("True"));
        assert!(json.contains("AllGood"));

        let deserialized: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.r#type, condition.r#type);
        assert_eq!(deserialized.status, condition.status);
    }

    #[test]
    fn test_status_with_no_conditions() {
        let status = Bind9InstanceStatus {
            conditions: vec![],
            observed_generation: Some(1),
            replicas: Some(3),
            ready_replicas: Some(3),
            service_address: None,
            selected_zones: vec![],
        };

        assert_eq!(status.conditions.len(), 0);
        assert!(status.observed_generation.is_some());
    }

    #[test]
    fn test_observed_generation_tracking() {
        let status = Bind9InstanceStatus {
            conditions: vec![],
            observed_generation: Some(5),
            replicas: Some(3),
            ready_replicas: Some(3),
            service_address: None,
            selected_zones: vec![],
        };

        // Observed generation tracks which generation of the resource was last reconciled
        assert_eq!(status.observed_generation, Some(5));
    }

    // ========================================================================
    // Algorithm Tests - as_str() and as_rndc_str()
    // ========================================================================

    #[test]
    fn test_rndc_algorithm_as_str_all_variants() {
        // Test that as_str() returns the BIND9 format with "hmac-" prefix
        assert_eq!(RndcAlgorithm::HmacMd5.as_str(), "hmac-md5");
        assert_eq!(RndcAlgorithm::HmacSha1.as_str(), "hmac-sha1");
        assert_eq!(RndcAlgorithm::HmacSha224.as_str(), "hmac-sha224");
        assert_eq!(RndcAlgorithm::HmacSha256.as_str(), "hmac-sha256");
        assert_eq!(RndcAlgorithm::HmacSha384.as_str(), "hmac-sha384");
        assert_eq!(RndcAlgorithm::HmacSha512.as_str(), "hmac-sha512");
    }

    #[test]
    fn test_rndc_algorithm_as_rndc_str_all_variants() {
        // Test that as_rndc_str() returns the format expected by rndc crate (no "hmac-" prefix)
        assert_eq!(RndcAlgorithm::HmacMd5.as_rndc_str(), "md5");
        assert_eq!(RndcAlgorithm::HmacSha1.as_rndc_str(), "sha1");
        assert_eq!(RndcAlgorithm::HmacSha224.as_rndc_str(), "sha224");
        assert_eq!(RndcAlgorithm::HmacSha256.as_rndc_str(), "sha256");
        assert_eq!(RndcAlgorithm::HmacSha384.as_rndc_str(), "sha384");
        assert_eq!(RndcAlgorithm::HmacSha512.as_rndc_str(), "sha512");
    }

    #[test]
    fn test_rndc_algorithm_default() {
        // Test that the default algorithm is HmacSha256
        let default_alg = RndcAlgorithm::default();
        assert_eq!(default_alg, RndcAlgorithm::HmacSha256);
        assert_eq!(default_alg.as_str(), "hmac-sha256");
        assert_eq!(default_alg.as_rndc_str(), "sha256");
    }

    #[test]
    fn test_rndc_algorithm_format_consistency() {
        // Verify that as_str() always has "hmac-" prefix and as_rndc_str() doesn't
        let algorithms = vec![
            RndcAlgorithm::HmacMd5,
            RndcAlgorithm::HmacSha1,
            RndcAlgorithm::HmacSha224,
            RndcAlgorithm::HmacSha256,
            RndcAlgorithm::HmacSha384,
            RndcAlgorithm::HmacSha512,
        ];

        for algo in algorithms {
            let bind9_format = algo.as_str();
            let rndc_format = algo.as_rndc_str();

            // BIND9 format should have "hmac-" prefix
            assert!(
                bind9_format.starts_with("hmac-"),
                "BIND9 format should start with 'hmac-': {bind9_format}"
            );

            // RNDC format should NOT have "hmac-" prefix
            assert!(
                !rndc_format.starts_with("hmac-"),
                "RNDC format should NOT start with 'hmac-': {rndc_format}"
            );

            // RNDC format should be the BIND9 format without the "hmac-" prefix
            assert_eq!(
                bind9_format.strip_prefix("hmac-").unwrap(),
                rndc_format,
                "RNDC format should be BIND9 format without 'hmac-' prefix"
            );
        }
    }

    #[test]
    fn test_rndc_algorithm_clone_and_partial_eq() {
        let algo = RndcAlgorithm::HmacSha256;
        let cloned = algo.clone();

        assert_eq!(algo, cloned);
        assert_eq!(algo.as_str(), cloned.as_str());
        assert_eq!(algo.as_rndc_str(), cloned.as_rndc_str());
    }

    #[test]
    fn test_rndc_algorithm_debug_format() {
        // Test that Debug format is reasonable
        let algo = RndcAlgorithm::HmacSha256;
        let debug_str = format!("{algo:?}");
        assert!(debug_str.contains("HmacSha256"));
    }

    #[test]
    fn test_rndc_algorithm_serialization() {
        use serde_json;

        // RndcAlgorithm uses serde(rename_all = "kebab-case")
        // So it should serialize to "hmac-sha256" format
        let algo = RndcAlgorithm::HmacSha256;
        let json = serde_json::to_string(&algo).unwrap();
        assert_eq!(json, "\"hmac-sha256\"");

        let algo = RndcAlgorithm::HmacMd5;
        let json = serde_json::to_string(&algo).unwrap();
        assert_eq!(json, "\"hmac-md5\"");
    }

    #[test]
    fn test_rndc_algorithm_deserialization_success() {
        use serde_json;

        // Test successful deserialization with kebab-case
        let json = "\"hmac-sha256\"";
        let algo: RndcAlgorithm = serde_json::from_str(json).unwrap();
        assert_eq!(algo, RndcAlgorithm::HmacSha256);

        let json = "\"hmac-sha512\"";
        let algo: RndcAlgorithm = serde_json::from_str(json).unwrap();
        assert_eq!(algo, RndcAlgorithm::HmacSha512);

        let json = "\"hmac-md5\"";
        let algo: RndcAlgorithm = serde_json::from_str(json).unwrap();
        assert_eq!(algo, RndcAlgorithm::HmacMd5);
    }

    #[test]
    fn test_rndc_algorithm_deserialization_failure_camel_case() {
        use serde_json;

        // Test that camelCase fails (we expect kebab-case)
        let json = "\"hmacSha256\"";
        let result: Result<RndcAlgorithm, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "Deserialization should fail for camelCase: hmacSha256"
        );
    }

    #[test]
    fn test_rndc_algorithm_deserialization_failure_uppercase() {
        use serde_json;

        // Test that uppercase fails
        let json = "\"HMAC-SHA256\"";
        let result: Result<RndcAlgorithm, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "Deserialization should fail for uppercase: HMAC-SHA256"
        );
    }

    #[test]
    fn test_rndc_algorithm_deserialization_failure_no_prefix() {
        use serde_json;

        // Test that algorithm strings without "hmac-" prefix fail during deserialization
        let json = "\"sha256\"";
        let result: Result<RndcAlgorithm, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "Deserialization should fail for algorithm without 'hmac-' prefix: sha256"
        );

        let json = "\"md5\"";
        let result: Result<RndcAlgorithm, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "Deserialization should fail for algorithm without 'hmac-' prefix: md5"
        );
    }

    #[test]
    fn test_rndc_algorithm_deserialization_failure_misspelled() {
        use serde_json;

        // Test various misspellings
        let invalid_values = vec![
            "\"hmac-sha-256\"", // Extra hyphen
            "\"hmac_sha256\"",  // Underscore instead of hyphen
            "\"hmac-sha 256\"", // Space instead of hyphen
            "\"hmac-sha25\"",   // Truncated
            "\"hmac-sha2566\"", // Extra digit
            "\"hmac-shaa256\"", // Extra 'a'
            "\"mac-sha256\"",   // Missing 'h'
            "\"sha256-hmac\"",  // Reversed
        ];

        for invalid_json in invalid_values {
            let result: Result<RndcAlgorithm, _> = serde_json::from_str(invalid_json);
            assert!(
                result.is_err(),
                "Deserialization should fail for invalid value: {invalid_json}"
            );
        }
    }

    #[test]
    fn test_rndc_algorithm_deserialization_failure_unknown_algorithm() {
        use serde_json;

        // Test unknown algorithm names
        let invalid_algorithms = vec![
            "\"hmac-sha3\"",
            "\"hmac-blake2\"",
            "\"hmac-sha128\"",
            "\"hmac-md4\"",
            "\"hmac-ripemd160\"",
        ];

        for invalid_json in invalid_algorithms {
            let result: Result<RndcAlgorithm, _> = serde_json::from_str(invalid_json);
            assert!(
                result.is_err(),
                "Deserialization should fail for unknown algorithm: {invalid_json}"
            );
        }
    }

    #[test]
    fn test_rndc_algorithm_roundtrip() {
        use serde_json;

        // Test that serialization and deserialization roundtrip correctly
        let algorithms = vec![
            RndcAlgorithm::HmacMd5,
            RndcAlgorithm::HmacSha1,
            RndcAlgorithm::HmacSha224,
            RndcAlgorithm::HmacSha256,
            RndcAlgorithm::HmacSha384,
            RndcAlgorithm::HmacSha512,
        ];

        for original in algorithms {
            let json = serde_json::to_string(&original).unwrap();
            let deserialized: RndcAlgorithm = serde_json::from_str(&json).unwrap();
            assert_eq!(original, deserialized);
            assert_eq!(original.as_str(), deserialized.as_str());
            assert_eq!(original.as_rndc_str(), deserialized.as_rndc_str());
        }
    }

    #[test]
    fn test_rndc_secret_ref_with_algorithm() {
        // Test that RndcSecretRef correctly uses the algorithm
        let secret_ref = RndcSecretRef {
            name: "my-rndc-key".into(),
            algorithm: RndcAlgorithm::HmacSha256,
            key_name_key: "key-name".into(),
            secret_key: "secret".into(),
        };

        assert_eq!(secret_ref.algorithm, RndcAlgorithm::HmacSha256);
        assert_eq!(secret_ref.algorithm.as_str(), "hmac-sha256");
        assert_eq!(secret_ref.algorithm.as_rndc_str(), "sha256");
    }

    #[test]
    fn test_tsig_key_with_different_algorithms() {
        // Test TSIGKey with different algorithms
        let algorithms = vec![
            RndcAlgorithm::HmacSha256,
            RndcAlgorithm::HmacSha512,
            RndcAlgorithm::HmacMd5,
        ];

        for algo in algorithms {
            let tsig = TSIGKey {
                name: "transfer-key".into(),
                algorithm: algo.clone(),
                secret: "base64secret==".into(),
            };

            assert_eq!(tsig.algorithm, algo);
            assert_eq!(tsig.algorithm.as_str(), algo.as_str());
            assert_eq!(tsig.algorithm.as_rndc_str(), algo.as_rndc_str());
        }
    }
}
