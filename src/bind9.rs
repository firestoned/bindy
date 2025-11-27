use crate::crd::SOARecord;
use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

/// Helper struct for SRV record parameters
pub struct SRVRecordData {
    pub priority: i32,
    pub weight: i32,
    pub port: i32,
    pub target: String,
    pub ttl: Option<i32>,
}

/// Manages BIND9 zone files and configurations
pub struct Bind9Manager {
    zones_dir: String,
}

impl Bind9Manager {
    pub fn new(zones_dir: String) -> Self {
        Self { zones_dir }
    }

    /// Create a primary zone file from zone name and SOA record
    pub fn create_zone_file(
        &self,
        zone_name: &str,
        soa_record: &SOARecord,
        default_ttl: i32,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));

        // Format SOA record
        // Convert admin@example.com to admin.example.com
        let admin_email_formatted = soa_record.admin_email.replace('@', ".");

        let content = format!(
            r#"$TTL {}
@   IN  SOA {} {} (
        {}  ; serial
        {}  ; refresh
        {}  ; retry
        {}  ; expire
        {} ) ; minimum
    IN  NS  {}
"#,
            default_ttl,
            soa_record.primary_ns,
            admin_email_formatted,
            soa_record.serial,
            soa_record.refresh,
            soa_record.retry,
            soa_record.expire,
            soa_record.negative_ttl,
            soa_record.primary_ns
        );

        fs::write(&path, content)?;
        info!("Created zone file: {}", path.display());
        Ok(())
    }

    /// Create a secondary zone configuration
    /// This creates a stub file indicating the zone should be transferred from primary servers
    pub fn create_secondary_zone(&self, zone_name: &str, primary_servers: &[String]) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}.secondary", zone_name));

        // For secondary zones, we create a marker file with primary server info
        // The actual zone data will be transferred from the primary servers
        let primaries_list = primary_servers.join("; ");
        let content = format!(
            r#"; Secondary zone for {}
; Primary servers: {}
; Zone data will be transferred from primary servers
"#,
            zone_name, primaries_list
        );

        fs::write(&path, content)?;
        info!("Created secondary zone marker: {}", path.display());
        Ok(())
    }

    /// Add an A record to a zone file
    pub fn add_a_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv4: &str,
        ttl: Option<i32>,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        let record = format!("{}{}  IN  A  {}\n", name, ttl_part, ipv4);
        content.push_str(&record);

        fs::write(&path, content)?;
        info!("Added A record: {} -> {}", name, ipv4);
        Ok(())
    }

    /// Add a CNAME record to a zone file
    pub fn add_cname_record(
        &self,
        zone_name: &str,
        name: &str,
        target: &str,
        ttl: Option<i32>,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        let record = format!("{}{}  IN  CNAME  {}\n", name, ttl_part, target);
        content.push_str(&record);

        fs::write(&path, content)?;
        info!("Added CNAME record: {} -> {}", name, target);
        Ok(())
    }

    /// Add a TXT record to a zone file
    pub fn add_txt_record(
        &self,
        zone_name: &str,
        name: &str,
        texts: &[String],
        ttl: Option<i32>,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        // TXT records need to be quoted
        let text_parts = texts
            .iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>()
            .join(" ");

        let record = format!("{}{}  IN  TXT  {}\n", name, ttl_part, text_parts);
        content.push_str(&record);

        fs::write(&path, content)?;
        info!("Added TXT record: {} -> {}", name, texts.join(", "));
        Ok(())
    }

    /// Add an AAAA record to a zone file
    pub fn add_aaaa_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv6: &str,
        ttl: Option<i32>,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        let record = format!("{}{}  IN  AAAA  {}\n", name, ttl_part, ipv6);
        content.push_str(&record);

        fs::write(&path, content)?;
        info!("Added AAAA record: {} -> {}", name, ipv6);
        Ok(())
    }

    /// Add an MX record to a zone file
    pub fn add_mx_record(
        &self,
        zone_name: &str,
        name: &str,
        priority: i32,
        mail_server: &str,
        ttl: Option<i32>,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        let record = format!(
            "{}{}  IN  MX  {} {}\n",
            name, ttl_part, priority, mail_server
        );
        content.push_str(&record);

        fs::write(&path, content)?;
        info!(
            "Added MX record: {} -> {} (priority: {})",
            name, mail_server, priority
        );
        Ok(())
    }

    /// Add an NS record to a zone file
    pub fn add_ns_record(
        &self,
        zone_name: &str,
        name: &str,
        nameserver: &str,
        ttl: Option<i32>,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        let record = format!("{}{}  IN  NS  {}\n", name, ttl_part, nameserver);
        content.push_str(&record);

        fs::write(&path, content)?;
        info!("Added NS record: {} -> {}", name, nameserver);
        Ok(())
    }

    /// Add an SRV record to a zone file
    pub fn add_srv_record(
        &self,
        zone_name: &str,
        name: &str,
        srv_data: &SRVRecordData,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = srv_data.ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        let record = format!(
            "{}{}  IN  SRV  {} {} {} {}\n",
            name, ttl_part, srv_data.priority, srv_data.weight, srv_data.port, srv_data.target
        );
        content.push_str(&record);

        fs::write(&path, content)?;
        info!(
            "Added SRV record: {} -> {}:{} (priority: {}, weight: {})",
            name, srv_data.target, srv_data.port, srv_data.priority, srv_data.weight
        );
        Ok(())
    }

    /// Add a CAA record to a zone file
    pub fn add_caa_record(
        &self,
        zone_name: &str,
        name: &str,
        flags: i32,
        tag: &str,
        value: &str,
        ttl: Option<i32>,
    ) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        let mut content = fs::read_to_string(&path)?;

        let ttl_str = ttl.map(|t| t.to_string()).unwrap_or_default();
        let ttl_part = if ttl_str.is_empty() {
            String::new()
        } else {
            format!(" {}", ttl_str)
        };

        let record = format!(
            "{}{}  IN  CAA  {} {} \"{}\"\n",
            name, ttl_part, flags, tag, value
        );
        content.push_str(&record);

        fs::write(&path, content)?;
        info!(
            "Added CAA record: {} -> {} {} \"{}\"",
            name, flags, tag, value
        );
        Ok(())
    }

    /// Delete a zone file
    pub fn delete_zone(&self, zone_name: &str) -> Result<()> {
        let path = Path::new(&self.zones_dir).join(format!("db.{}", zone_name));
        if path.exists() {
            fs::remove_file(&path)?;
            info!("Deleted zone file: {}", path.display());
        }
        Ok(())
    }

    /// Check if a zone file exists
    pub fn zone_exists(&self, zone_name: &str) -> bool {
        Path::new(&self.zones_dir)
            .join(format!("db.{}", zone_name))
            .exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Bind9Manager) {
        let temp_dir = TempDir::new().unwrap();
        let manager = Bind9Manager::new(temp_dir.path().to_string_lossy().to_string());
        (temp_dir, manager)
    }

    fn create_test_zone(manager: &Bind9Manager) {
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };
        manager.create_zone_file("example.com", &soa, 3600).unwrap();
    }

    #[test]
    fn test_create_zone_file() {
        let (_temp_dir, manager) = setup();

        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        assert!(manager.zone_exists("example.com"));

        // Verify content
        let path = Path::new(&manager.zones_dir).join("db.example.com");
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("$TTL 3600"));
        assert!(content.contains("ns1.example.com."));
        assert!(content.contains("admin.example.com"));
        assert!(content.contains("2024010101"));
    }

    #[test]
    fn test_create_secondary_zone() {
        let (_temp_dir, manager) = setup();

        let primary_servers = vec!["10.0.1.1".to_string(), "10.0.1.2".to_string()];
        manager
            .create_secondary_zone("example.com", &primary_servers)
            .unwrap();

        let path = Path::new(&manager.zones_dir).join("db.example.com.secondary");
        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("Secondary zone for example.com"));
        assert!(content.contains("10.0.1.1; 10.0.1.2"));
        assert!(content.contains("Primary servers"));
    }

    #[test]
    fn test_add_a_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("www 300  IN  A  192.0.2.1"));
    }

    #[test]
    fn test_add_a_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_a_record("example.com", "mail", "192.0.2.2", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("mail  IN  A  192.0.2.2"));
    }

    #[test]
    fn test_add_aaaa_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_aaaa_record("example.com", "www", "2001:db8::1", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("www 300  IN  AAAA  2001:db8::1"));
    }

    #[test]
    fn test_add_aaaa_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_aaaa_record("example.com", "mail", "2001:db8::2", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("mail  IN  AAAA  2001:db8::2"));
    }

    #[test]
    fn test_add_cname_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_cname_record("example.com", "blog", "www.example.com.", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("blog 300  IN  CNAME  www.example.com."));
    }

    #[test]
    fn test_add_cname_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_cname_record("example.com", "ftp", "www.example.com.", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("ftp  IN  CNAME  www.example.com."));
    }

    #[test]
    fn test_add_txt_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec!["v=spf1 mx ~all".to_string()];
        manager
            .add_txt_record("example.com", "@", &texts, Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  TXT  \"v=spf1 mx ~all\""));
    }

    #[test]
    fn test_add_txt_record_multiple_values() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec!["part1".to_string(), "part2".to_string()];
        manager
            .add_txt_record("example.com", "_dmarc", &texts, None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("_dmarc  IN  TXT  \"part1\" \"part2\""));
    }

    #[test]
    fn test_add_mx_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  MX  10 mail.example.com."));
    }

    #[test]
    fn test_add_mx_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_mx_record("example.com", "@", 20, "mail2.example.com.", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@  IN  MX  20 mail2.example.com."));
    }

    #[test]
    fn test_add_ns_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_ns_record("example.com", "@", "ns2.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  NS  ns2.example.com."));
    }

    #[test]
    fn test_add_ns_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_ns_record("example.com", "subdomain", "ns3.example.com.", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("subdomain  IN  NS  ns3.example.com."));
    }

    #[test]
    fn test_add_srv_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

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

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("_sip._tcp 3600  IN  SRV  10 60 5060 sipserver.example.com."));
    }

    #[test]
    fn test_add_srv_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

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

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("_ldap._tcp  IN  SRV  0 100 389 ldap.example.com."));
    }

    #[test]
    fn test_add_caa_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

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

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  CAA  0 issue \"letsencrypt.org\""));
    }

    #[test]
    fn test_add_caa_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

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

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@  IN  CAA  128 iodef \"mailto:admin@example.com\""));
    }

    #[test]
    fn test_delete_zone() {
        let (_temp_dir, manager) = setup();
        create_test_zone(&manager);

        assert!(manager.zone_exists("example.com"));

        manager.delete_zone("example.com").unwrap();

        assert!(!manager.zone_exists("example.com"));
    }

    #[test]
    fn test_delete_zone_nonexistent() {
        let (_temp_dir, manager) = setup();

        // Should not error when deleting non-existent zone
        manager.delete_zone("nonexistent.com").unwrap();
    }

    #[test]
    fn test_zone_exists_false() {
        let (_temp_dir, manager) = setup();

        assert!(!manager.zone_exists("nonexistent.com"));
    }

    #[test]
    fn test_multiple_records() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Add multiple different record types
        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_aaaa_record("example.com", "www", "2001:db8::1", Some(300))
            .unwrap();
        manager
            .add_cname_record("example.com", "blog", "www.example.com.", Some(300))
            .unwrap();
        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("www 300  IN  A  192.0.2.1"));
        assert!(content.contains("www 300  IN  AAAA  2001:db8::1"));
        assert!(content.contains("blog 300  IN  CNAME  www.example.com."));
        assert!(content.contains("@ 3600  IN  MX  10 mail.example.com."));
    }

    // =====================================================
    // Additional Comprehensive Tests
    // =====================================================

    #[test]
    fn test_zone_name_with_underscores() {
        let (_temp_dir, manager) = setup();
        let soa = SOARecord {
            primary_ns: "ns1._example.com.".to_string(),
            admin_email: "admin@_example.com".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let result = manager.create_zone_file("_example.com", &soa, 3600);
        assert!(result.is_ok());
        assert!(manager.zone_exists("_example.com"));
    }

    #[test]
    fn test_zone_name_with_hyphens() {
        let (_temp_dir, manager) = setup();
        let soa = SOARecord {
            primary_ns: "ns1.my-example.com.".to_string(),
            admin_email: "admin@my-example.com".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let result = manager.create_zone_file("my-example.com", &soa, 3600);
        assert!(result.is_ok());
        assert!(manager.zone_exists("my-example.com"));
    }

    #[test]
    fn test_add_multiple_a_records() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "mail", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "ftp", "192.0.2.3", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("www 300  IN  A  192.0.2.1"));
        assert!(content.contains("mail 300  IN  A  192.0.2.2"));
        assert!(content.contains("ftp 300  IN  A  192.0.2.3"));
    }

    #[test]
    fn test_special_characters_in_txt_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec!["v=spf1 include:_spf.google.com ~all".to_string()];
        manager
            .add_txt_record("example.com", "@", &texts, Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("v=spf1 include:_spf.google.com ~all"));
    }

    #[test]
    fn test_multiple_txt_record_strings() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec![
            "part1".to_string(),
            "part2".to_string(),
            "part3".to_string(),
        ];
        manager
            .add_txt_record("example.com", "_dmarc", &texts, Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("_dmarc 3600  IN  TXT  \"part1\" \"part2\" \"part3\""));
    }

    #[test]
    fn test_large_serial_number() {
        let (_temp_dir, manager) = setup();
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 9999999999i64,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let content =
            fs::read_to_string(Path::new(&manager.zones_dir).join("db.example.com")).unwrap();

        assert!(content.contains("9999999999"));
    }

    #[test]
    fn test_different_email_formats() {
        let (_temp_dir, manager) = setup();

        let soa1 = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let soa2 = SOARecord {
            primary_ns: "ns1.example.org.".to_string(),
            admin_email: "hostmaster@example.org".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        manager
            .create_zone_file("example1.com", &soa1, 3600)
            .unwrap();
        manager
            .create_zone_file("example2.org", &soa2, 3600)
            .unwrap();

        let content1 =
            fs::read_to_string(Path::new(&manager.zones_dir).join("db.example1.com")).unwrap();
        let content2 =
            fs::read_to_string(Path::new(&manager.zones_dir).join("db.example2.org")).unwrap();

        assert!(content1.contains("admin.example.com"));
        assert!(content2.contains("hostmaster.example.org"));
    }

    #[test]
    fn test_custom_ttl_values() {
        let (temp_dir, manager) = setup();

        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        manager.create_zone_file("example.com", &soa, 7200).unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("$TTL 7200"));
    }

    #[test]
    fn test_mx_record_priority_order() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Add MX records with different priorities
        manager
            .add_mx_record("example.com", "@", 10, "mail1.example.com.", Some(3600))
            .unwrap();
        manager
            .add_mx_record("example.com", "@", 20, "mail2.example.com.", Some(3600))
            .unwrap();
        manager
            .add_mx_record("example.com", "@", 30, "mail3.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("@ 3600  IN  MX  10 mail1.example.com."));
        assert!(content.contains("@ 3600  IN  MX  20 mail2.example.com."));
        assert!(content.contains("@ 3600  IN  MX  30 mail3.example.com."));
    }

    #[test]
    fn test_srv_record_parameters() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

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

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("_sip._tcp 3600  IN  SRV  10 60 5060 sipserver.example.com."));
    }

    #[test]
    fn test_caa_record_flags_and_tag() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Issue CAA record
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
        // Wildcard issue record
        manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issuewild",
                "letsencrypt.org",
                Some(3600),
            )
            .unwrap();
        // IODEF record
        manager
            .add_caa_record(
                "example.com",
                "@",
                128,
                "iodef",
                "mailto:admin@example.com",
                Some(3600),
            )
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("@ 3600  IN  CAA  0 issue \"letsencrypt.org\""));
        assert!(content.contains("@ 3600  IN  CAA  0 issuewild \"letsencrypt.org\""));
        assert!(content.contains("@ 3600  IN  CAA  128 iodef \"mailto:admin@example.com\""));
    }

    #[test]
    fn test_complete_zone_workflow() {
        let (_temp_dir, manager) = setup();

        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        // Step 1: Create zone
        manager.create_zone_file("example.com", &soa, 3600).unwrap();
        assert!(manager.zone_exists("example.com"));

        // Step 2: Add apex records
        manager
            .add_a_record("example.com", "@", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_aaaa_record("example.com", "@", "2001:db8::1", Some(300))
            .unwrap();
        manager
            .add_ns_record("example.com", "@", "ns1.example.com.", Some(3600))
            .unwrap();
        manager
            .add_ns_record("example.com", "@", "ns2.example.com.", Some(3600))
            .unwrap();

        // Step 3: Add subdomain records
        manager
            .add_a_record("example.com", "www", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "mail", "192.0.2.3", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "ftp", "192.0.2.4", Some(300))
            .unwrap();

        // Step 4: Add service records
        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();
        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sip.example.com.".to_string(),
            ttl: Some(3600),
        };
        manager
            .add_srv_record("example.com", "_sip._tcp", &srv_data)
            .unwrap();

        // Step 5: Add TXT records
        let spf = vec!["v=spf1 mx ~all".to_string()];
        manager
            .add_txt_record("example.com", "@", &spf, Some(3600))
            .unwrap();

        // Step 6: Verify zone still exists
        assert!(manager.zone_exists("example.com"));

        // Step 7: Delete zone
        manager.delete_zone("example.com").unwrap();
        assert!(!manager.zone_exists("example.com"));
    }

    #[test]
    fn test_primary_and_secondary_zones_together() {
        let (_temp_dir, manager) = setup();

        // Create primary zone
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin@example.com".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };
        manager.create_zone_file("primary.com", &soa, 3600).unwrap();

        // Create secondary zone
        let primary_servers = vec!["10.0.1.1".to_string(), "10.0.1.2".to_string()];
        manager
            .create_secondary_zone("secondary.com", &primary_servers)
            .unwrap();

        assert!(manager.zone_exists("primary.com"));
        // Note: Secondary zones are stored with .secondary extension, so they don't match zone_exists()
        // which looks for "db.{zone_name}", but we can verify the file exists separately
        let secondary_path = Path::new(&manager.zones_dir).join("db.secondary.com.secondary");
        assert!(secondary_path.exists());
    }

    #[test]
    fn test_secondary_zone_with_multiple_primaries() {
        let (_temp_dir, manager) = setup();

        let primary_servers = vec![
            "10.0.1.1".to_string(),
            "10.0.1.2".to_string(),
            "10.0.1.3".to_string(),
            "10.0.1.4".to_string(),
        ];
        manager
            .create_secondary_zone("secondary.com", &primary_servers)
            .unwrap();

        let content =
            fs::read_to_string(Path::new(&manager.zones_dir).join("db.secondary.com.secondary"))
                .unwrap();

        for server in primary_servers {
            assert!(content.contains(&server));
        }
    }

    #[test]
    fn test_nameserver_records_at_subdomain() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_ns_record(
                "example.com",
                "subdomain",
                "ns1.subdomain.example.com.",
                Some(3600),
            )
            .unwrap();
        manager
            .add_ns_record(
                "example.com",
                "subdomain",
                "ns2.subdomain.example.com.",
                Some(3600),
            )
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("subdomain 3600  IN  NS  ns1.subdomain.example.com."));
        assert!(content.contains("subdomain 3600  IN  NS  ns2.subdomain.example.com."));
    }

    #[test]
    fn test_zone_apex_vs_subdomain() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Apex records
        manager
            .add_a_record("example.com", "@", "192.0.2.1", Some(300))
            .unwrap();
        // Subdomain records
        manager
            .add_a_record("example.com", "www", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "api", "192.0.2.3", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("@ 300  IN  A  192.0.2.1"));
        assert!(content.contains("www 300  IN  A  192.0.2.2"));
        assert!(content.contains("api 300  IN  A  192.0.2.3"));
    }
}
