// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! BIND9 management via HTTP API sidecar.
//!
//! This module provides functionality for managing BIND9 servers using an
//! HTTP API sidecar container that executes rndc commands locally. It handles:
//!
//! - Creating and managing DNS zones via the HTTP API
//! - Adding and updating DNS zones via dynamic updates (nsupdate protocol)
//! - Reloading zones after changes
//! - Managing zone transfers
//! - RNDC key generation and management
//!
//! # Architecture
//!
//! The `Bind9Manager` communicates with BIND9 instances via an HTTP API sidecar
//! running in the same pod. The sidecar executes rndc commands locally and manages
//! zone files. Authentication uses Kubernetes `ServiceAccount` tokens.
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy::bind9::Bind9Manager;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let manager = Bind9Manager::new();
//!
//! // Manage zones via HTTP API
//! manager.reload_zone(
//!     "example.com",
//!     "bind9-primary-api.dns-system.svc.cluster.local:8080"
//! ).await?;
//! # Ok(())
//! # }
//! ```

// Module declarations
pub mod records;
pub mod rndc;
pub mod types;
pub mod zone_ops;

// Re-export public types and functions for backwards compatibility
pub use rndc::{
    create_rndc_secret_data, create_tsig_signer, generate_rndc_key, parse_rndc_secret_data,
};
pub use types::{RndcError, RndcKeyData, SRVRecordData, SERVICE_ACCOUNT_TOKEN_PATH};

use anyhow::{Context, Result};
use bindcar::ZoneConfig;
use reqwest::Client as HttpClient;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

/// Manager for BIND9 servers via HTTP API sidecar.
///
/// The `Bind9Manager` provides methods for managing BIND9 servers running in Kubernetes
/// pods via an HTTP API sidecar. The API sidecar executes rndc commands locally and
/// manages zone files. Authentication uses Kubernetes `ServiceAccount` tokens.
///
/// # Examples
///
/// ```rust,no_run
/// use bindy::bind9::Bind9Manager;
///
/// let manager = Bind9Manager::new();
/// ```
#[derive(Debug, Clone)]
pub struct Bind9Manager {
    /// HTTP client for API requests
    client: Arc<HttpClient>,
    /// `ServiceAccount` token for authentication
    token: Arc<String>,
}

impl Bind9Manager {
    /// Create a new `Bind9Manager`.
    ///
    /// Reads the `ServiceAccount` token from the default location and creates
    /// an HTTP client for API requests.
    #[must_use]
    pub fn new() -> Self {
        let token = Self::read_service_account_token().unwrap_or_else(|e| {
            warn!(
                "Failed to read ServiceAccount token: {}. Using empty token.",
                e
            );
            String::new()
        });

        Self {
            client: Arc::new(HttpClient::new()),
            token: Arc::new(token),
        }
    }

    /// Read the `ServiceAccount` token from the mounted secret
    fn read_service_account_token() -> Result<String> {
        std::fs::read_to_string(SERVICE_ACCOUNT_TOKEN_PATH)
            .context("Failed to read ServiceAccount token file")
    }

    /// Build the API base URL from a server address
    ///
    /// Converts "service-name.namespace.svc.cluster.local:8080" or "service-name:8080"
    /// to `<http://service-name.namespace.svc.cluster.local:8080>` or `<http://service-name:8080>`
    ///
    /// This is a public method for testing purposes.
    #[must_use]
    pub fn build_api_url(server: &str) -> String {
        zone_ops::build_api_url(server)
    }

    // ===== Zone management methods =====

    /// Reload a specific zone via HTTP API.
    ///
    /// This operation is idempotent - if the zone doesn't exist, it returns an error
    /// with a clear message indicating the zone was not found.
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone to reload
    /// * `server` - API server address (e.g., "bind9-primary-api:8080")
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be reloaded.
    pub async fn reload_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        zone_ops::reload_zone(&self.client, &self.token, zone_name, server).await
    }

    /// Reload all zones via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn reload_all_zones(&self, server: &str) -> Result<()> {
        zone_ops::reload_all_zones(&self.client, &self.token, server).await
    }

    /// Trigger zone transfer via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone transfer cannot be initiated.
    pub async fn retransfer_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        zone_ops::retransfer_zone(&self.client, &self.token, zone_name, server).await
    }

    /// Freeze a zone to prevent dynamic updates via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be frozen.
    pub async fn freeze_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        zone_ops::freeze_zone(&self.client, &self.token, zone_name, server).await
    }

    /// Thaw a frozen zone to allow dynamic updates via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be thawed.
    pub async fn thaw_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        zone_ops::thaw_zone(&self.client, &self.token, zone_name, server).await
    }

    /// Get zone status via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone status cannot be retrieved.
    pub async fn zone_status(&self, zone_name: &str, server: &str) -> Result<String> {
        zone_ops::zone_status(&self.client, &self.token, zone_name, server).await
    }

    /// Check if a zone exists by trying to get its status.
    ///
    /// Returns `true` if the zone exists and can be queried, `false` otherwise.
    pub async fn zone_exists(&self, zone_name: &str, server: &str) -> bool {
        zone_ops::zone_exists(&self.client, &self.token, zone_name, server).await
    }

    /// Get server status via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the server status cannot be retrieved.
    pub async fn server_status(&self, server: &str) -> Result<String> {
        zone_ops::server_status(&self.client, &self.token, server).await
    }

    /// Add a zone via HTTP API (primary or secondary).
    ///
    /// This is the centralized zone addition method that dispatches to either
    /// `add_primary_zone` or `add_secondary_zone` based on the zone type.
    ///
    /// This operation is idempotent - if the zone already exists, it returns success
    /// without attempting to re-add it.
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone (e.g., "example.com")
    /// * `zone_type` - Zone type (use `ZONE_TYPE_PRIMARY` or `ZONE_TYPE_SECONDARY` constants)
    /// * `server` - API endpoint (e.g., "bind9-primary-api:8080" or "bind9-secondary-api:8080")
    /// * `key_data` - RNDC key data
    /// * `soa_record` - Optional SOA record data (required for primary zones, ignored for secondary)
    /// * `name_server_ips` - Optional map of nameserver hostnames to IP addresses (for primary zones)
    /// * `secondary_ips` - Optional list of secondary server IPs for also-notify and allow-transfer (for primary zones)
    /// * `primary_ips` - Optional list of primary server IPs to transfer from (for secondary zones)
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the zone was added, `Ok(false)` if it already existed.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be added.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_zones(
        &self,
        zone_name: &str,
        zone_type: &str,
        server: &str,
        key_data: &RndcKeyData,
        soa_record: Option<&crate::crd::SOARecord>,
        name_server_ips: Option<&HashMap<String, String>>,
        secondary_ips: Option<&[String]>,
        primary_ips: Option<&[String]>,
    ) -> Result<bool> {
        zone_ops::add_zones(
            &self.client,
            &self.token,
            zone_name,
            zone_type,
            server,
            key_data,
            soa_record,
            name_server_ips,
            secondary_ips,
            primary_ips,
        )
        .await
    }

    /// Add a new primary zone via HTTP API.
    ///
    /// This operation is idempotent - if the zone already exists, it returns success
    /// without attempting to re-add it.
    ///
    /// The zone is created with `allow-update` enabled for the TSIG key used by the operator.
    /// This allows dynamic DNS updates (RFC 2136) to add/update/delete records in the zone.
    ///
    /// **Note:** This method creates a zone without initial content. For creating zones with
    /// initial SOA/NS records, use `create_zone_http()` instead.
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone (e.g., "example.com")
    /// * `server` - API endpoint (e.g., "bind9-primary-api:8080")
    /// * `key_data` - RNDC key data (used for allow-update configuration)
    /// * `soa_record` - SOA record data
    /// * `name_server_ips` - Optional map of nameserver hostnames to IP addresses for glue records
    /// * `secondary_ips` - Optional list of secondary server IPs for also-notify and allow-transfer
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the zone was added, `Ok(false)` if it already existed.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be added.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::too_many_arguments
    )]
    pub async fn add_primary_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
        soa_record: &crate::crd::SOARecord,
        name_server_ips: Option<&HashMap<String, String>>,
        secondary_ips: Option<&[String]>,
    ) -> Result<bool> {
        zone_ops::add_primary_zone(
            &self.client,
            &self.token,
            zone_name,
            server,
            key_data,
            soa_record,
            name_server_ips,
            secondary_ips,
        )
        .await
    }

    /// Add a secondary zone via HTTP API.
    ///
    /// Creates a secondary zone that will transfer from the specified primary servers.
    /// This is a convenience method specifically for secondary zones.
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone (e.g., "example.com")
    /// * `server` - API endpoint of the secondary server (e.g., "bind9-secondary-api:8080")
    /// * `key_data` - RNDC key data
    /// * `primary_ips` - List of primary server IP addresses to transfer from
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the zone was added, `Ok(false)` if it already existed.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be added.
    pub async fn add_secondary_zone(
        &self,
        zone_name: &str,
        server: &str,
        key_data: &RndcKeyData,
        primary_ips: &[String],
    ) -> Result<bool> {
        zone_ops::add_secondary_zone(
            &self.client,
            &self.token,
            zone_name,
            server,
            key_data,
            primary_ips,
        )
        .await
    }

    /// Create a zone via HTTP API with structured configuration.
    ///
    /// This method sends a POST request to the API sidecar to create a zone using
    /// structured zone configuration from the bindcar library.
    ///
    /// # Arguments
    /// * `zone_name` - Name of the zone (e.g., "example.com")
    /// * `zone_type` - Zone type (use `ZONE_TYPE_PRIMARY` or `ZONE_TYPE_SECONDARY` constants)
    /// * `zone_config` - Structured zone configuration (converted to zone file by bindcar)
    /// * `server` - API endpoint (e.g., "bind9-primary-api:8080")
    /// * `key_data` - RNDC authentication key (used as updateKeyName)
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_zone_http(
        &self,
        zone_name: &str,
        zone_type: &str,
        zone_config: ZoneConfig,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        zone_ops::create_zone_http(
            &self.client,
            &self.token,
            zone_name,
            zone_type,
            zone_config,
            server,
            key_data,
        )
        .await
    }

    /// Delete a zone via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be deleted.
    pub async fn delete_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        zone_ops::delete_zone(&self.client, &self.token, zone_name, server).await
    }

    /// Notify secondaries about zone changes via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the notification cannot be sent.
    pub async fn notify_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        zone_ops::notify_zone(&self.client, &self.token, zone_name, server).await
    }

    // ===== DNS record management methods =====

    /// Add an A record using dynamic DNS update (RFC 2136).
    ///
    /// # Arguments
    /// * `zone_name` - DNS zone name (e.g., "example.com")
    /// * `name` - Record name (e.g., "www" for www.example.com, or "@" for apex)
    /// * `ipv4` - IPv4 address
    /// * `ttl` - Time to live in seconds (None = use zone default)
    /// * `server` - DNS server address with port (e.g., "10.0.0.1:53")
    /// * `key_data` - TSIG key for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_a_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv4: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::a::add_a_record(zone_name, name, ipv4, ttl, server, key_data).await
    }

    /// Add an AAAA record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_aaaa_record(
        &self,
        zone_name: &str,
        name: &str,
        ipv6: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::a::add_aaaa_record(zone_name, name, ipv6, ttl, server, key_data).await
    }

    /// Add a CNAME record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_cname_record(
        &self,
        zone_name: &str,
        name: &str,
        target: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::cname::add_cname_record(zone_name, name, target, ttl, server, key_data).await
    }

    /// Add a TXT record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_txt_record(
        &self,
        zone_name: &str,
        name: &str,
        texts: &[String],
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::txt::add_txt_record(zone_name, name, texts, ttl, server, key_data).await
    }

    /// Add an MX record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_mx_record(
        &self,
        zone_name: &str,
        name: &str,
        priority: i32,
        mail_server: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::mx::add_mx_record(
            zone_name,
            name,
            priority,
            mail_server,
            ttl,
            server,
            key_data,
        )
        .await
    }

    /// Add an NS record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if the DNS update fails or the server rejects it.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_ns_record(
        &self,
        zone_name: &str,
        name: &str,
        nameserver: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::ns::add_ns_record(zone_name, name, nameserver, ttl, server, key_data).await
    }

    /// Add an SRV record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - DNS server connection fails
    /// - TSIG signer creation fails
    /// - DNS update is rejected by the server
    /// - Invalid domain name or target
    #[allow(clippy::too_many_arguments)]
    pub async fn add_srv_record(
        &self,
        zone_name: &str,
        name: &str,
        srv_data: &SRVRecordData,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::srv::add_srv_record(zone_name, name, srv_data, server, key_data).await
    }

    /// Add a CAA record using dynamic DNS update (RFC 2136).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - DNS server connection fails
    /// - TSIG signer creation fails
    /// - DNS update is rejected by the server
    /// - Invalid domain name, flags, tag, or value
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    pub async fn add_caa_record(
        &self,
        zone_name: &str,
        name: &str,
        flags: i32,
        tag: &str,
        value: &str,
        ttl: Option<i32>,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        records::caa::add_caa_record(zone_name, name, flags, tag, value, ttl, server, key_data)
            .await
    }

    // ===== RNDC static methods (exposed through the struct for backwards compatibility) =====

    /// Generate a new RNDC key with HMAC-SHA256.
    ///
    /// Returns a base64-encoded 256-bit (32-byte) key suitable for rndc authentication.
    #[must_use]
    pub fn generate_rndc_key() -> RndcKeyData {
        rndc::generate_rndc_key()
    }

    /// Create a Kubernetes Secret manifest for an RNDC key.
    ///
    /// Returns a `BTreeMap` suitable for use as Secret data.
    #[must_use]
    pub fn create_rndc_secret_data(
        key_data: &RndcKeyData,
    ) -> std::collections::BTreeMap<String, String> {
        rndc::create_rndc_secret_data(key_data)
    }

    /// Parse RNDC key data from a Kubernetes Secret.
    ///
    /// Supports two Secret formats:
    /// 1. **Operator-generated** (all 4 fields): `key-name`, `algorithm`, `secret`, `rndc.key`
    /// 2. **External/user-managed** (minimal): `rndc.key` only - parses the BIND9 key file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Neither the metadata fields nor `rndc.key` are present
    /// - The `rndc.key` file cannot be parsed
    /// - Values are not valid UTF-8 strings
    pub fn parse_rndc_secret_data(
        data: &std::collections::BTreeMap<String, Vec<u8>>,
    ) -> Result<RndcKeyData> {
        rndc::parse_rndc_secret_data(data)
    }
}

impl Default for Bind9Manager {
    fn default() -> Self {
        Self::new()
    }
}

// Declare test modules
#[cfg(test)]
mod mod_tests;
