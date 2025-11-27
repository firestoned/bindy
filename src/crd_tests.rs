#[cfg(test)]
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
    fn test_secondary_zone_config() {
        let config = SecondaryZoneConfig {
            primary_servers: vec!["10.0.1.1".into(), "10.0.1.2".into()],
            tsig_key: Some("my-key".into()),
        };

        assert_eq!(config.primary_servers.len(), 2);
        assert_eq!(config.primary_servers[0], "10.0.1.1");
        assert!(config.tsig_key.is_some());
    }

    #[test]
    fn test_secondary_zone_config_without_tsig() {
        let config = SecondaryZoneConfig {
            primary_servers: vec!["10.0.1.1".into()],
            tsig_key: None,
        };

        assert_eq!(config.primary_servers.len(), 1);
        assert!(config.tsig_key.is_none());
    }

    #[test]
    fn test_dnszone_spec_primary() {
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
            zone_type: Some("primary".into()),
            instance_selector: LabelSelector::default(),
            soa_record: Some(soa),
            secondary_config: None,
            ttl: Some(3600),
        };

        assert_eq!(spec.zone_name, "example.com");
        assert_eq!(spec.zone_type.unwrap(), "primary");
        assert!(spec.soa_record.is_some());
        assert!(spec.secondary_config.is_none());
    }

    #[test]
    fn test_dnszone_spec_secondary() {
        let config = SecondaryZoneConfig {
            primary_servers: vec!["10.0.1.1".into()],
            tsig_key: None,
        };

        let spec = DNSZoneSpec {
            zone_name: "example.com".into(),
            zone_type: Some("secondary".into()),
            instance_selector: LabelSelector::default(),
            soa_record: None,
            secondary_config: Some(config),
            ttl: None,
        };

        assert_eq!(spec.zone_name, "example.com");
        assert_eq!(spec.zone_type.unwrap(), "secondary");
        assert!(spec.soa_record.is_none());
        assert!(spec.secondary_config.is_some());
    }

    #[test]
    fn test_arecord_spec() {
        let spec = ARecordSpec {
            zone: "example.com".into(),
            name: "www".into(),
            ipv4_address: "192.0.2.1".into(),
            ttl: Some(300),
        };

        assert_eq!(spec.zone, "example.com");
        assert_eq!(spec.name, "www");
        assert_eq!(spec.ipv4_address, "192.0.2.1");
        assert_eq!(spec.ttl.unwrap(), 300);
    }

    #[test]
    fn test_aaaarecord_spec() {
        let spec = AAAARecordSpec {
            zone: "example.com".into(),
            name: "www".into(),
            ipv6_address: "2001:db8::1".into(),
            ttl: Some(300),
        };

        assert_eq!(spec.zone, "example.com");
        assert_eq!(spec.ipv6_address, "2001:db8::1");
    }

    #[test]
    fn test_txtrecord_spec() {
        let spec = TXTRecordSpec {
            zone: "example.com".into(),
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
            zone: "example.com".into(),
            name: "blog".into(),
            target: "www.example.com.".into(),
            ttl: Some(300),
        };

        assert_eq!(spec.target, "www.example.com.");
    }

    #[test]
    fn test_mxrecord_spec() {
        let spec = MXRecordSpec {
            zone: "example.com".into(),
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
            zone: "example.com".into(),
            name: "@".into(),
            nameserver: "ns1.example.com.".into(),
            ttl: Some(3600),
        };

        assert_eq!(spec.nameserver, "ns1.example.com.");
    }

    #[test]
    fn test_srvrecord_spec() {
        let spec = SRVRecordSpec {
            zone: "example.com".into(),
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
            zone: "example.com".into(),
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
                enabled: Some(true),
                validation: Some(true),
            }),
            forwarders: Some(vec!["8.8.8.8".into(), "8.8.4.4".into()]),
            listen_on: Some(vec!["any".into()]),
            listen_on_v6: Some(vec!["any".into()]),
        };

        assert_eq!(config.recursion, Some(false));
        assert!(config.allow_query.is_some());
        assert!(config.dnssec.is_some());
        assert_eq!(config.forwarders.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_dnssec_config() {
        let config = DNSSECConfig {
            enabled: Some(true),
            validation: Some(false),
        };

        assert_eq!(config.enabled, Some(true));
        assert_eq!(config.validation, Some(false));
    }

    #[test]
    fn test_bind9instance_spec() {
        let spec = Bind9InstanceSpec {
            replicas: Some(3),
            version: Some("9.18".into()),
            config: Some(Bind9Config {
                recursion: Some(false),
                allow_query: None,
                allow_transfer: None,
                dnssec: None,
                forwarders: None,
                listen_on: None,
                listen_on_v6: None,
            }),
        };

        assert_eq!(spec.replicas, Some(3));
        assert_eq!(spec.version.unwrap(), "9.18");
        assert!(spec.config.is_some());
    }

    #[test]
    fn test_bind9instance_status_default() {
        let status = Bind9InstanceStatus::default();
        assert!(status.conditions.is_empty());
        assert!(status.observed_generation.is_none());
        assert!(status.replicas.is_none());
        assert!(status.ready_replicas.is_none());
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
        };

        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.replicas, Some(3));
        assert_eq!(status.ready_replicas, Some(2));
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
        };

        // Observed generation tracks which generation of the resource was last reconciled
        assert_eq!(status.observed_generation, Some(5));
    }
}
