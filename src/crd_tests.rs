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
        labels.insert("app".to_string(), "dns".to_string());
        labels.insert("env".to_string(), "prod".to_string());

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
            key: "environment".to_string(),
            operator: "In".to_string(),
            values: Some(vec!["prod".to_string(), "staging".to_string()]),
        };

        assert_eq!(req.key, "environment");
        assert_eq!(req.operator, "In");
        assert!(req.values.is_some());
        assert_eq!(req.values.unwrap().len(), 2);
    }

    #[test]
    fn test_soa_record() {
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
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
            r#type: "Ready".to_string(),
            status: "True".to_string(),
            reason: Some("ReconcileSuccess".to_string()),
            message: Some("Zone created successfully".to_string()),
            last_transition_time: Some("2024-01-01T00:00:00Z".to_string()),
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
            primary_servers: vec!["10.0.1.1".to_string(), "10.0.1.2".to_string()],
            tsig_key: Some("my-key".to_string()),
        };

        assert_eq!(config.primary_servers.len(), 2);
        assert_eq!(config.primary_servers[0], "10.0.1.1");
        assert!(config.tsig_key.is_some());
    }

    #[test]
    fn test_secondary_zone_config_without_tsig() {
        let config = SecondaryZoneConfig {
            primary_servers: vec!["10.0.1.1".to_string()],
            tsig_key: None,
        };

        assert_eq!(config.primary_servers.len(), 1);
        assert!(config.tsig_key.is_none());
    }

    #[test]
    fn test_dnszone_spec_primary() {
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let spec = DNSZoneSpec {
            zone_name: "example.com".to_string(),
            zone_type: Some("primary".to_string()),
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
            primary_servers: vec!["10.0.1.1".to_string()],
            tsig_key: None,
        };

        let spec = DNSZoneSpec {
            zone_name: "example.com".to_string(),
            zone_type: Some("secondary".to_string()),
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
            zone: "example.com".to_string(),
            name: "www".to_string(),
            ipv4_address: "192.0.2.1".to_string(),
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
            zone: "example.com".to_string(),
            name: "www".to_string(),
            ipv6_address: "2001:db8::1".to_string(),
            ttl: Some(300),
        };

        assert_eq!(spec.zone, "example.com");
        assert_eq!(spec.ipv6_address, "2001:db8::1");
    }

    #[test]
    fn test_txtrecord_spec() {
        let spec = TXTRecordSpec {
            zone: "example.com".to_string(),
            name: "@".to_string(),
            text: vec!["v=spf1 mx ~all".to_string()],
            ttl: Some(3600),
        };

        assert_eq!(spec.text.len(), 1);
        assert_eq!(spec.text[0], "v=spf1 mx ~all");
    }

    #[test]
    fn test_cnamerecord_spec() {
        let spec = CNAMERecordSpec {
            zone: "example.com".to_string(),
            name: "blog".to_string(),
            target: "www.example.com.".to_string(),
            ttl: Some(300),
        };

        assert_eq!(spec.target, "www.example.com.");
    }

    #[test]
    fn test_mxrecord_spec() {
        let spec = MXRecordSpec {
            zone: "example.com".to_string(),
            name: "@".to_string(),
            priority: 10,
            mail_server: "mail.example.com.".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(spec.priority, 10);
        assert_eq!(spec.mail_server, "mail.example.com.");
    }

    #[test]
    fn test_nsrecord_spec() {
        let spec = NSRecordSpec {
            zone: "example.com".to_string(),
            name: "@".to_string(),
            nameserver: "ns1.example.com.".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(spec.nameserver, "ns1.example.com.");
    }

    #[test]
    fn test_srvrecord_spec() {
        let spec = SRVRecordSpec {
            zone: "example.com".to_string(),
            name: "_sip._tcp".to_string(),
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sipserver.example.com.".to_string(),
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
            zone: "example.com".to_string(),
            name: "@".to_string(),
            flags: 0,
            tag: "issue".to_string(),
            value: "letsencrypt.org".to_string(),
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
            allow_query: Some(vec!["0.0.0.0/0".to_string()]),
            allow_transfer: Some(vec!["10.0.0.0/8".to_string()]),
            dnssec: Some(DNSSECConfig {
                enabled: Some(true),
                validation: Some(true),
            }),
            forwarders: Some(vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()]),
            listen_on: Some(vec!["any".to_string()]),
            listen_on_v6: Some(vec!["any".to_string()]),
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
            version: Some("9.18".to_string()),
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
            r#type: "Ready".to_string(),
            status: "True".to_string(),
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
}
