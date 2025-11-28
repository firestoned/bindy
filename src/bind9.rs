// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 zone file generation and management.
//!
//! This module provides functionality for generating and managing BIND9 zone files
//! from Kubernetes Custom Resources. It handles:
//!
//! - Creating zone files with SOA records
//! - Adding DNS records (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)
//! - Formatting records according to BIND9 syntax
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy::bind9::Bind9Manager;
//! use bindy::crd::SOARecord;
//!
//! let manager = Bind9Manager::new("/etc/bind/zones".to_string());
//!
//! let soa = SOARecord {
//!     primary_ns: "ns1.example.com.".to_string(),
//!     admin_email: "admin@example.com".to_string(),
//!     serial: 2024010101,
//!     refresh: 3600,
//!     retry: 600,
//!     expire: 604800,
//!     negative_ttl: 86400,
//! };
//!
//! // Create a zone file
//! manager.create_zone_file("example.com", &soa, 3600).unwrap();
//!
//! // Add an A record
//! manager.add_a_record("example.com", "www", "192.0.2.1", Some(300)).unwrap();
//! ```

use crate::crd::SOARecord;
use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

/// Parameters for creating SRV records.
///
/// Contains the priority, weight, port, and target required for SRV records.
pub struct SRVRecordData {
    /// Priority of the target host (lower is higher priority)
    pub priority: i32,
    /// Relative weight for records with the same priority
    pub weight: i32,
    /// TCP or UDP port on which the service is found
    pub port: i32,
    /// Canonical hostname of the machine providing the service
    pub target: String,
    /// Time to live in seconds
    pub ttl: Option<i32>,
}

/// Manager for BIND9 zone files and DNS records.
///
/// The `Bind9Manager` provides methods for creating and updating BIND9 zone files
/// in a specified directory. All methods are designed to be idempotent.
///
/// # Examples
///
/// ```rust,no_run
/// use bindy::bind9::Bind9Manager;
///
/// let manager = Bind9Manager::new("/var/lib/bind".to_string());
/// ```
pub struct Bind9Manager {
    /// Directory where zone files are stored
    zones_dir: String,
}

impl Bind9Manager {
    pub fn new(zones_dir: String) -> Self {
        Self { zones_dir }
    }

    /// Get the zones directory path (for testing)
    #[cfg(test)]
    pub(crate) fn zones_dir(&self) -> &str {
        &self.zones_dir
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
