// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 management via rndc (Remote Name Daemon Control).
//!
//! This module provides functionality for managing BIND9 servers using the
//! native `rndc` protocol. It handles:
//!
//! - Connecting to BIND9 instances via rndc protocol over Kubernetes networking
//! - Adding and updating DNS zones via dynamic updates (nsupdate protocol)
//! - Reloading zones after changes
//! - Managing zone transfers
//! - RNDC key generation and management
//!
//! # Architecture
//!
//! The `Bind9Manager` communicates with BIND9 instances running in Kubernetes pods
//! using the RNDC protocol (port 953). Each `Bind9Instance` has an associated Secret
//! containing the RNDC key, allowing secure remote management.
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy::bind9::Bind9Manager;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let manager = Bind9Manager::new();
//!
//! // Generate an RNDC key for a new instance
//! let mut key_data = Bind9Manager::generate_rndc_key();
//! key_data.name = "bind9-primary".to_string();
//!
//! // Manage zones via rndc
//! manager.reload_zone(
//!     "example.com",
//!     "bind9-primary.dns-system.svc.cluster.local:953",
//!     &key_data
//! ).await?;
//! # Ok(())
//! # }
//! ```

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::Rng;
use std::collections::BTreeMap;
use tracing::info;

/// RNDC key data for authentication.
#[derive(Debug, Clone)]
pub struct RndcKeyData {
    /// Key name (typically the instance name)
    pub name: String,
    /// HMAC algorithm (e.g., "hmac-sha256", "hmac-sha512")
    pub algorithm: String,
    /// Base64-encoded secret key
    pub secret: String,
}

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

/// Manager for BIND9 servers via rndc protocol.
///
/// The `Bind9Manager` provides methods for managing BIND9 servers running in Kubernetes
/// pods using the native RNDC protocol. All connections use TSIG authentication with
/// HMAC-SHA256 or HMAC-SHA512 keys stored in Kubernetes Secrets.
///
/// # Examples
///
/// ```rust,no_run
/// use bindy::bind9::Bind9Manager;
///
/// let manager = Bind9Manager::new();
/// ```
#[derive(Debug)]
pub struct Bind9Manager;

impl Bind9Manager {
    /// Create a new `Bind9Manager`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Generate a new RNDC key with HMAC-SHA256.
    ///
    /// Returns a base64-encoded 256-bit (32-byte) key suitable for rndc authentication.
    #[must_use]
    pub fn generate_rndc_key() -> RndcKeyData {
        let mut rng = rand::thread_rng();
        let mut key_bytes = [0u8; 32]; // 256 bits for HMAC-SHA256
        rng.fill(&mut key_bytes);

        RndcKeyData {
            name: String::new(), // Will be set by caller
            algorithm: "hmac-sha256".to_string(),
            secret: BASE64.encode(key_bytes),
        }
    }

    /// Create a Kubernetes Secret manifest for an RNDC key.
    ///
    /// Returns a `BTreeMap` suitable for use as Secret data.
    #[must_use]
    pub fn create_rndc_secret_data(key_data: &RndcKeyData) -> BTreeMap<String, String> {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), key_data.name.clone());
        data.insert("algorithm".to_string(), key_data.algorithm.clone());
        data.insert("secret".to_string(), key_data.secret.clone());
        data
    }

    /// Parse RNDC key data from a Kubernetes Secret.
    ///
    /// # Errors
    ///
    /// Returns an error if the secret data is missing required keys (`key-name`, `algorithm`, `secret`)
    /// or if the values are not valid UTF-8 strings.
    pub fn parse_rndc_secret_data(data: &BTreeMap<String, Vec<u8>>) -> Result<RndcKeyData> {
        let name =
            std::str::from_utf8(data.get("key-name").context("Missing key-name in secret")?)?
                .to_string();

        let algorithm = std::str::from_utf8(
            data.get("algorithm")
                .context("Missing algorithm in secret")?,
        )?
        .to_string();

        let secret = std::str::from_utf8(data.get("secret").context("Missing secret in secret")?)?
            .to_string();

        Ok(RndcKeyData {
            name,
            algorithm,
            secret,
        })
    }

    /// Connect to an rndc server and execute a command.
    ///
    /// # Arguments
    /// * `server` - Server address in format "host:port" (e.g., "bind9-primary.dns-system.svc.cluster.local:953")
    /// * `key_data` - RNDC authentication key
    /// * `command` - RNDC command to execute (e.g., "status", "reload zone")
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC connection fails, authentication fails, or the command execution fails.
    async fn exec_rndc_command(
        &self,
        server: &str,
        key_data: &RndcKeyData,
        command: &str,
    ) -> Result<String> {
        // Create rndc client (API: new(server_url, algorithm, secret_key_b64))
        let client = rndc::RndcClient::new(server, &key_data.algorithm, &key_data.secret);

        // Execute command (synchronous, so use spawn_blocking)
        let command = command.to_string();
        let result = tokio::task::spawn_blocking(move || client.rndc_command(&command))
            .await
            .context("Task join failed")?
            .map_err(|e| anyhow::anyhow!("RNDC command failed: {e}"))?;

        Ok(result.text.unwrap_or_default())
    }

    /// Reload a specific zone using rndc.
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone to reload
    /// * `server` - Server address (e.g., "bind9-primary.dns-system.svc.cluster.local:953")
    /// * `key_data` - RNDC authentication key
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the zone cannot be reloaded.
    pub async fn reload_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let command = format!("reload {zone_name}");
        self.exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to reload zone")?;

        info!("Reloaded zone {zone_name} on {server}");
        Ok(())
    }

    /// Reload all zones using rndc.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails.
    pub async fn reload_all_zones(&self, server: &str, key_data: &RndcKeyData) -> Result<()> {
        self.exec_rndc_command(server, key_data, "reload")
            .await
            .context("Failed to reload all zones")?;

        info!("Reloaded all zones on {server}");
        Ok(())
    }

    /// Trigger zone transfer using rndc.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the zone transfer cannot be initiated.
    pub async fn retransfer_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let command = format!("retransfer {zone_name}");
        self.exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to retransfer zone")?;

        info!("Triggered zone transfer for {zone_name} on {server}");
        Ok(())
    }

    /// Freeze a zone to prevent dynamic updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the zone cannot be frozen.
    pub async fn freeze_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let command = format!("freeze {zone_name}");
        self.exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to freeze zone")?;

        info!("Froze zone {zone_name} on {server}");
        Ok(())
    }

    /// Thaw a frozen zone to allow dynamic updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the zone cannot be thawed.
    pub async fn thaw_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let command = format!("thaw {zone_name}");
        self.exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to thaw zone")?;

        info!("Thawed zone {zone_name} on {server}");
        Ok(())
    }

    /// Get zone status using rndc.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the zone status cannot be retrieved.
    pub async fn zone_status(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<String> {
        let command = format!("zonestatus {zone_name}");
        let status = self
            .exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to get zone status")?;

        Ok(status)
    }

    /// Check if a zone exists by trying to get its status.
    pub async fn zone_exists(&self, zone_name: &str, server: &str, key_data: &RndcKeyData) -> bool {
        self.zone_status(zone_name, server, key_data).await.is_ok()
    }

    /// Get server status using rndc.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the server status cannot be retrieved.
    pub async fn server_status(&self, server: &str, key_data: &RndcKeyData) -> Result<String> {
        let status = self
            .exec_rndc_command(server, key_data, "status")
            .await
            .context("Failed to get server status")?;

        Ok(status)
    }

    /// Add a new zone using rndc addzone.
    ///
    /// Note: This requires dynamic zone configuration to be enabled in named.conf.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the zone cannot be added.
    pub async fn add_zone(
        &self,
        zone_name: &str,
        zone_type: &str,
        zone_file: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let command =
            format!(r#"addzone {zone_name} '{{ type {zone_type}; file "{zone_file}"; }};'"#);

        self.exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to add zone")?;

        info!("Added zone {zone_name} on {server}");
        Ok(())
    }

    /// Delete a zone using rndc delzone.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the zone cannot be deleted.
    pub async fn delete_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        // First freeze the zone to prevent updates
        let _ = self.freeze_zone(zone_name, server, key_data).await;

        let command = format!("delzone {zone_name}");
        self.exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to delete zone")?;

        info!("Deleted zone {zone_name} from {server}");
        Ok(())
    }

    /// Notify secondaries about zone changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the RNDC command fails or the notification cannot be sent.
    pub async fn notify_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let command = format!("notify {zone_name}");
        self.exec_rndc_command(server, key_data, &command)
            .await
            .context("Failed to notify zone")?;

        info!("Notified secondaries for zone {zone_name} from {server}");
        Ok(())
    }

    // Note: For DNS record management (A, AAAA, CNAME, etc.), we'll need to use
    // nsupdate protocol or direct zone file manipulation + reload.
    // The rndc protocol itself doesn't support individual record operations.
    //
    // We have a few options:
    // 1. Use nsupdate protocol (requires separate implementation)
    // 2. Use zone file manipulation via ConfigMap + rndc reload
    // 3. Use dynamic DNS updates if enabled on the zone
    //
    // For now, these methods are placeholders that will need implementation
    // based on the chosen approach.

    /// Placeholder: Add an A record (requires nsupdate or zone file manipulation).
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_a_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv4: &str,
        ttl: Option<i32>,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        // TODO: Implement using nsupdate protocol or zone file + reload
        info!("Would add A record: {name}.{zone_name} -> {ipv4} (TTL: {ttl:?}) on {server}");
        // For now, just return success to allow compilation
        Ok(())
    }

    /// Placeholder: Add a CNAME record.
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_cname_record(
        &self,
        zone_name: &str,
        name: &str,
        target: &str,
        ttl: Option<i32>,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        info!("Would add CNAME record: {name}.{zone_name} -> {target} (TTL: {ttl:?}) on {server}");
        Ok(())
    }

    /// Placeholder: Add a TXT record.
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_txt_record(
        &self,
        zone_name: &str,
        name: &str,
        texts: &[String],
        ttl: Option<i32>,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        info!("Would add TXT record: {name}.{zone_name} -> {texts:?} (TTL: {ttl:?}) on {server}");
        Ok(())
    }

    /// Placeholder: Add an AAAA record.
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_aaaa_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv6: &str,
        ttl: Option<i32>,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        info!("Would add AAAA record: {name}.{zone_name} -> {ipv6} (TTL: {ttl:?}) on {server}");
        Ok(())
    }

    /// Placeholder: Add an MX record.
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_mx_record(
        &self,
        zone_name: &str,
        name: &str,
        priority: i32,
        mail_server: &str,
        ttl: Option<i32>,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        info!(
            "Would add MX record: {name}.{zone_name} -> {mail_server} (priority: {priority}, TTL: {ttl:?}) on {server}"
        );
        Ok(())
    }

    /// Placeholder: Add an NS record.
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_ns_record(
        &self,
        zone_name: &str,
        name: &str,
        nameserver: &str,
        ttl: Option<i32>,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        info!("Would add NS record: {name}.{zone_name} -> {nameserver} (TTL: {ttl:?}) on {server}");
        Ok(())
    }

    /// Placeholder: Add an SRV record.
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_srv_record(
        &self,
        zone_name: &str,
        name: &str,
        srv_data: &SRVRecordData,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        let target = &srv_data.target;
        let port = srv_data.port;
        let priority = srv_data.priority;
        let weight = srv_data.weight;
        let ttl = srv_data.ttl;
        info!(
            "Would add SRV record: {name}.{zone_name} -> {target}:{port} (priority: {priority}, weight: {weight}, TTL: {ttl:?}) on {server}"
        );
        Ok(())
    }

    /// Placeholder: Add a CAA record.
    ///
    /// # Errors
    ///
    /// This is currently a placeholder and always returns `Ok(())`.
    #[allow(clippy::unused_async)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_caa_record(
        &self,
        zone_name: &str,
        name: &str,
        flags: i32,
        tag: &str,
        value: &str,
        ttl: Option<i32>,
        server: &str,
        _key_data: &RndcKeyData,
    ) -> Result<()> {
        info!(
            "Would add CAA record: {name}.{zone_name} -> {flags} {tag} \"{value}\" (TTL: {ttl:?}) on {server}"
        );
        Ok(())
    }
}

impl Default for Bind9Manager {
    fn default() -> Self {
        Self::new()
    }
}
