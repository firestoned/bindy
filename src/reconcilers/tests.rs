#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, SRVRecordData};
    use crate::crd::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    // =====================================================
    // Helper Functions
    // =====================================================

    fn create_test_manager() -> (TempDir, Bind9Manager) {
        let temp_dir = TempDir::new().unwrap();
        let manager = Bind9Manager::new(temp_dir.path().to_string_lossy().to_string());
        (temp_dir, manager)
    }

    fn create_label_selector(key: &str, value: &str) -> LabelSelector {
        let mut labels = BTreeMap::new();
        labels.insert(key.to_string(), value.to_string());
        LabelSelector {
            match_labels: Some(labels),
            match_expressions: None,
        }
    }

    fn create_soa_record(primary_ns: &str, admin_email: &str, serial: i64) -> SOARecord {
        SOARecord {
            primary_ns: primary_ns.to_string(),
            admin_email: admin_email.to_string(),
            serial,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        }
    }

    fn create_dnszone_primary(name: &str, zone_name: &str, soa: SOARecord) -> DNSZone {
        DNSZone {
            metadata: kube::api::ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: DNSZoneSpec {
                zone_name: zone_name.to_string(),
                zone_type: Some("primary".to_string()),
                instance_selector: create_label_selector("bind9", "instance"),
                soa_record: Some(soa),
                secondary_config: None,
                ttl: Some(3600),
            },
            status: None,
        }
    }

    fn create_dnszone_secondary(
        name: &str,
        zone_name: &str,
        primary_servers: Vec<String>,
    ) -> DNSZone {
        DNSZone {
            metadata: kube::api::ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                generation: Some(1),
                ..Default::default()
            },
            spec: DNSZoneSpec {
                zone_name: zone_name.to_string(),
                zone_type: Some("secondary".to_string()),
                instance_selector: create_label_selector("bind9", "instance"),
                soa_record: None,
                secondary_config: Some(SecondaryZoneConfig {
                    primary_servers,
                    tsig_key: None,
                }),
                ttl: None,
            },
            status: None,
        }
    }

    // =====================================================
    // Bind9Manager Basic Tests
    // =====================================================

    #[test]
    fn test_bind9_manager_creation() {
        let (_temp_dir, manager) = create_test_manager();
        assert!(!manager.zones_dir.is_empty());
    }

    #[test]
    fn test_create_zone_file_with_soa() {
        let (_temp_dir, manager) = create_test_manager();

        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 2024010101);
        let result = manager.create_zone_file("example.com", &soa, 3600);

        assert!(result.is_ok());
        assert!(manager.zone_exists("example.com"));
    }

    #[test]
    fn test_create_zone_file_content_verification() {
        let (_temp_dir, manager) = create_test_manager();

        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 2024010101);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("$TTL 3600"));
        assert!(content.contains("ns1.example.com."));
        assert!(content.contains("admin.example.com"));
        assert!(content.contains("2024010101"));
    }

    #[test]
    fn test_email_formatting_in_zone_file() {
        let (_temp_dir, manager) = create_test_manager();

        let soa = create_soa_record("ns1.example.com.", "user@domain.co.uk", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        // Email should be formatted as user.domain.co.uk (@ replaced with .)
        assert!(content.contains("user.domain.co.uk"));
        assert!(!content.contains("user@domain.co.uk"));
    }

    #[test]
    fn test_create_secondary_zone() {
        let (_temp_dir, manager) = create_test_manager();

        let primary_servers = vec!["10.0.1.1".to_string(), "10.0.1.2".to_string()];
        let result = manager.create_secondary_zone("example.com", &primary_servers);

        assert!(result.is_ok());
        let path = std::path::Path::new(&manager.zones_dir).join("db.example.com.secondary");
        assert!(path.exists());
    }

    #[test]
    fn test_create_secondary_zone_content() {
        let (_temp_dir, manager) = create_test_manager();

        let primary_servers = vec!["10.0.1.1".to_string(), "10.0.1.2".to_string()];
        manager
            .create_secondary_zone("example.com", &primary_servers)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com.secondary"),
        )
        .unwrap();

        assert!(content.contains("Secondary zone for example.com"));
        assert!(content.contains("10.0.1.1; 10.0.1.2"));
    }

    // =====================================================
    // Record Management Tests
    // =====================================================

    #[test]
    fn test_add_a_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("www 300  IN  A  192.0.2.1"));
    }

    #[test]
    fn test_add_a_record_without_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_a_record("example.com", "mail", "192.0.2.2", None)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("mail  IN  A  192.0.2.2"));
    }

    #[test]
    fn test_add_aaaa_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_aaaa_record("example.com", "www", "2001:db8::1", Some(300))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("www 300  IN  AAAA  2001:db8::1"));
    }

    #[test]
    fn test_add_aaaa_record_without_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_aaaa_record("example.com", "mail", "2001:db8::2", None)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("mail  IN  AAAA  2001:db8::2"));
    }

    #[test]
    fn test_add_cname_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_cname_record("example.com", "blog", "www.example.com.", Some(300))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("blog 300  IN  CNAME  www.example.com."));
    }

    #[test]
    fn test_add_cname_record_without_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_cname_record("example.com", "ftp", "www.example.com.", None)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("ftp  IN  CNAME  www.example.com."));
    }

    #[test]
    fn test_add_txt_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let texts = vec!["v=spf1 mx ~all".to_string()];
        manager
            .add_txt_record("example.com", "@", &texts, Some(3600))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("@ 3600  IN  TXT  \"v=spf1 mx ~all\""));
    }

    #[test]
    fn test_add_txt_record_multiple_strings() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let texts = vec!["part1".to_string(), "part2".to_string()];
        manager
            .add_txt_record("example.com", "_dmarc", &texts, None)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("_dmarc  IN  TXT  \"part1\" \"part2\""));
    }

    #[test]
    fn test_add_mx_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("@ 3600  IN  MX  10 mail.example.com."));
    }

    #[test]
    fn test_add_mx_record_without_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_mx_record("example.com", "@", 20, "mail2.example.com.", None)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("@  IN  MX  20 mail2.example.com."));
    }

    #[test]
    fn test_add_ns_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_ns_record("example.com", "@", "ns2.example.com.", Some(3600))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("@ 3600  IN  NS  ns2.example.com."));
    }

    #[test]
    fn test_add_ns_record_without_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_ns_record("example.com", "subdomain", "ns3.example.com.", None)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("subdomain  IN  NS  ns3.example.com."));
    }

    #[test]
    fn test_add_srv_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sipserver.example.com.".to_string(),
            ttl: Some(3600),
        };
        manager
            .add_srv_record("example.com", "_sip._tcp", &srv_data)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("_sip._tcp 3600  IN  SRV  10 60 5060 sipserver.example.com."));
    }

    #[test]
    fn test_add_srv_record_without_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let srv_data = SRVRecordData {
            priority: 0,
            weight: 100,
            port: 389,
            target: "ldap.example.com.".to_string(),
            ttl: None,
        };
        manager
            .add_srv_record("example.com", "_ldap._tcp", &srv_data)
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("_ldap._tcp  IN  SRV  0 100 389 ldap.example.com."));
    }

    #[test]
    fn test_add_caa_record_with_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issue",
                "letsencrypt.org",
                Some(3600),
            )
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("@ 3600  IN  CAA  0 issue \"letsencrypt.org\""));
    }

    #[test]
    fn test_add_caa_record_without_ttl() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        manager
            .add_caa_record(
                "example.com",
                "@",
                128,
                "iodef",
                "mailto:admin@example.com",
                None,
            )
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("@  IN  CAA  128 iodef \"mailto:admin@example.com\""));
    }

    // =====================================================
    // Zone Management Tests
    // =====================================================

    #[test]
    fn test_zone_exists() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);

        assert!(!manager.zone_exists("example.com"));
        manager.create_zone_file("example.com", &soa, 3600).unwrap();
        assert!(manager.zone_exists("example.com"));
    }

    #[test]
    fn test_delete_zone() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);

        manager.create_zone_file("example.com", &soa, 3600).unwrap();
        assert!(manager.zone_exists("example.com"));

        manager.delete_zone("example.com").unwrap();
        assert!(!manager.zone_exists("example.com"));
    }

    #[test]
    fn test_delete_nonexistent_zone() {
        let (_temp_dir, manager) = create_test_manager();

        // Should not error when deleting non-existent zone
        let result = manager.delete_zone("nonexistent.com");
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_zones() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);

        manager.create_zone_file("example.com", &soa, 3600).unwrap();
        manager.create_zone_file("example.org", &soa, 3600).unwrap();
        manager.create_zone_file("example.net", &soa, 3600).unwrap();

        assert!(manager.zone_exists("example.com"));
        assert!(manager.zone_exists("example.org"));
        assert!(manager.zone_exists("example.net"));
    }

    // =====================================================
    // DNS Zone CRD Tests
    // =====================================================

    #[test]
    fn test_create_primary_dnszone() {
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        let dnszone = create_dnszone_primary("test-zone", "example.com", soa);

        assert_eq!(dnszone.spec.zone_name, "example.com");
        assert_eq!(dnszone.spec.zone_type.as_ref().unwrap(), "primary");
        assert!(dnszone.spec.soa_record.is_some());
    }

    #[test]
    fn test_create_secondary_dnszone() {
        let primary_servers = vec!["10.0.1.1".to_string()];
        let dnszone = create_dnszone_secondary("test-zone", "example.com", primary_servers);

        assert_eq!(dnszone.spec.zone_name, "example.com");
        assert_eq!(dnszone.spec.zone_type.as_ref().unwrap(), "secondary");
        assert!(dnszone.spec.secondary_config.is_some());
    }

    #[test]
    fn test_dnszone_label_selector() {
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        let dnszone = create_dnszone_primary("test-zone", "example.com", soa);

        assert!(dnszone.spec.instance_selector.match_labels.is_some());
        let labels = dnszone.spec.instance_selector.match_labels.unwrap();
        assert_eq!(labels.get("bind9").unwrap(), "instance");
    }

    // =====================================================
    // SRVRecordData Tests
    // =====================================================

    #[test]
    fn test_srv_record_data_creation() {
        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sipserver.example.com.".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(srv_data.priority, 10);
        assert_eq!(srv_data.weight, 60);
        assert_eq!(srv_data.port, 5060);
        assert_eq!(srv_data.target, "sipserver.example.com.");
        assert_eq!(srv_data.ttl, Some(3600));
    }

    #[test]
    fn test_srv_record_data_without_ttl() {
        let srv_data = SRVRecordData {
            priority: 0,
            weight: 50,
            port: 389,
            target: "ldap.example.com.".to_string(),
            ttl: None,
        };

        assert_eq!(srv_data.priority, 0);
        assert_eq!(srv_data.ttl, None);
    }

    // =====================================================
    // Edge Cases and Error Handling
    // =====================================================

    #[test]
    fn test_zone_name_with_underscores() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);

        let result = manager.create_zone_file("_example.com", &soa, 3600);
        assert!(result.is_ok());
        assert!(manager.zone_exists("_example.com"));
    }

    #[test]
    fn test_zone_name_with_hyphens() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);

        let result = manager.create_zone_file("my-example.com", &soa, 3600);
        assert!(result.is_ok());
        assert!(manager.zone_exists("my-example.com"));
    }

    #[test]
    fn test_add_multiple_records_to_zone() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        // Add multiple records
        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "mail", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_aaaa_record("example.com", "www", "2001:db8::1", Some(300))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("www 300  IN  A  192.0.2.1"));
        assert!(content.contains("mail 300  IN  A  192.0.2.2"));
        assert!(content.contains("www 300  IN  AAAA  2001:db8::1"));
    }

    #[test]
    fn test_special_characters_in_zone_content() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let texts = vec!["v=spf1 include:_spf.google.com ~all".to_string()];
        manager
            .add_txt_record("example.com", "@", &texts, Some(300))
            .unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("v=spf1 include:_spf.google.com ~all"));
    }

    #[test]
    fn test_large_serial_number() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 9999999999i64);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("9999999999"));
    }

    #[test]
    fn test_soa_record_with_different_emails() {
        let (_temp_dir, manager) = create_test_manager();

        let soa1 = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        let soa2 = create_soa_record("ns1.example.com.", "hostmaster@example.org", 1);

        manager
            .create_zone_file("example1.com", &soa1, 3600)
            .unwrap();
        manager
            .create_zone_file("example2.com", &soa2, 3600)
            .unwrap();

        let content1 = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example1.com"),
        )
        .unwrap();
        let content2 = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example2.com"),
        )
        .unwrap();

        assert!(content1.contains("admin.example.com"));
        assert!(content2.contains("hostmaster.example.org"));
    }

    #[test]
    fn test_default_ttl_values() {
        let (_temp_dir, manager) = create_test_manager();
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);

        manager.create_zone_file("example.com", &soa, 7200).unwrap();

        let content = std::fs::read_to_string(
            std::path::Path::new(&manager.zones_dir).join("db.example.com"),
        )
        .unwrap();

        assert!(content.contains("$TTL 7200"));
    }

    // =====================================================
    // Integration Tests
    // =====================================================

    #[test]
    fn test_complete_zone_workflow() {
        let (_temp_dir, manager) = create_test_manager();

        // Create zone
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 2024010101);
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        // Add various records
        manager
            .add_a_record("example.com", "@", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "www", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_aaaa_record("example.com", "www", "2001:db8::1", Some(300))
            .unwrap();
        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();
        manager
            .add_ns_record("example.com", "@", "ns2.example.com.", Some(3600))
            .unwrap();

        // Verify zone exists
        assert!(manager.zone_exists("example.com"));

        // Delete zone
        manager.delete_zone("example.com").unwrap();
        assert!(!manager.zone_exists("example.com"));
    }

    #[test]
    fn test_primary_and_secondary_zones_together() {
        let (_temp_dir, manager) = create_test_manager();

        // Create primary zone
        let soa = create_soa_record("ns1.example.com.", "admin@example.com", 1);
        manager.create_zone_file("primary.com", &soa, 3600).unwrap();

        // Create secondary zone
        let primary_servers = vec!["10.0.1.1".to_string()];
        manager
            .create_secondary_zone("secondary.com", &primary_servers)
            .unwrap();

        assert!(manager.zone_exists("primary.com"));
        assert!(manager.zone_exists("secondary.com"));
    }
}
