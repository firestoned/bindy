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

use crate::constants::{DEFAULT_DNS_RECORD_TTL_SECS, TSIG_FUDGE_TIME_SECS};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use bindcar::{CreateZoneRequest, SoaRecord, ZoneConfig, ZoneResponse};
use hickory_client::client::{Client, SyncClient};
use hickory_client::op::ResponseCode;
use hickory_client::rr::rdata;
use hickory_client::rr::rdata::tsig::TsigAlgorithm;
use hickory_client::rr::{DNSClass, Name, RData, Record};
use hickory_client::udp::UdpClientConnection;
use hickory_proto::rr::dnssec::tsig::TSigner;
use rand::Rng;
use reqwest::Client as HttpClient;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use url::Url;

/// RNDC key data for authentication.
#[derive(Debug, Clone)]
pub struct RndcKeyData {
    /// Key name (typically the instance name)
    pub name: String,
    /// HMAC algorithm
    pub algorithm: crate::crd::RndcAlgorithm,
    /// Base64-encoded secret key
    pub secret: String,
}

/// RNDC command error with structured information.
///
/// Parses BIND9 RNDC error responses in the format:
/// ```text
/// rndc: 'command' failed: error_type
/// error details
/// ```
#[derive(Debug, Clone, thiserror::Error)]
#[error("RNDC command '{command}' failed: {error}")]
pub struct RndcError {
    /// The RNDC command that failed (e.g., "zonestatus", "addzone")
    pub command: String,
    /// The error type (e.g., "not found", "already exists")
    pub error: String,
    /// Additional error details from BIND9
    pub details: Option<String>,
}

impl RndcError {
    /// Parse an RNDC error response.
    ///
    /// Expected format:
    /// ```text
    /// rndc: 'zonestatus' failed: not found
    /// no matching zone 'example.com' in any view
    /// ```
    #[must_use]
    pub fn parse(response: &str) -> Option<Self> {
        // Parse first line: rndc: 'command' failed: error
        let lines: Vec<&str> = response.lines().collect();
        let first_line = lines.first()?;

        if !first_line.starts_with("rndc:") {
            return None;
        }

        // Extract command from 'command'
        let command_start = first_line.find('\'')?;
        let command_end = first_line[command_start + 1..].find('\'')?;
        let command = first_line[command_start + 1..command_start + 1 + command_end].to_string();

        // Extract error after "failed: "
        let failed_pos = first_line.find("failed:")?;
        let error = first_line[failed_pos + 7..].trim().to_string();

        // Remaining lines are details
        let details = if lines.len() > 1 {
            Some(lines[1..].join("\n").trim().to_string())
        } else {
            None
        };

        Some(Self {
            command,
            error,
            details,
        })
    }
}

/// Path to the `ServiceAccount` token file in Kubernetes pods
const SERVICE_ACCOUNT_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";

/// Parameters for creating SRV records.
///
/// Contains the priority, weight, port, and target required for SRV records.
#[derive(Clone)]
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
    pub(crate) fn build_api_url(server: &str) -> String {
        if server.starts_with("http://") || server.starts_with("https://") {
            server.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", server.trim_end_matches('/'))
        }
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
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
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
        data.insert(
            "algorithm".to_string(),
            key_data.algorithm.as_str().to_string(),
        );
        data.insert("secret".to_string(), key_data.secret.clone());

        // Add rndc.key file content for BIND9 to use
        let rndc_key_content = format!(
            "key \"{}\" {{\n    algorithm {};\n    secret \"{}\";\n}};\n",
            key_data.name,
            key_data.algorithm.as_str(),
            key_data.secret
        );
        data.insert("rndc.key".to_string(), rndc_key_content);

        data
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
    pub fn parse_rndc_secret_data(data: &BTreeMap<String, Vec<u8>>) -> Result<RndcKeyData> {
        // Try the operator-generated format first (has all metadata fields)
        if let (Some(name_bytes), Some(algo_bytes), Some(secret_bytes)) = (
            data.get("key-name"),
            data.get("algorithm"),
            data.get("secret"),
        ) {
            let name = std::str::from_utf8(name_bytes)?.to_string();
            let algorithm_str = std::str::from_utf8(algo_bytes)?;
            let secret = std::str::from_utf8(secret_bytes)?.to_string();

            let algorithm = match algorithm_str {
                "hmac-md5" => crate::crd::RndcAlgorithm::HmacMd5,
                "hmac-sha1" => crate::crd::RndcAlgorithm::HmacSha1,
                "hmac-sha224" => crate::crd::RndcAlgorithm::HmacSha224,
                "hmac-sha256" => crate::crd::RndcAlgorithm::HmacSha256,
                "hmac-sha384" => crate::crd::RndcAlgorithm::HmacSha384,
                "hmac-sha512" => crate::crd::RndcAlgorithm::HmacSha512,
                _ => anyhow::bail!("Unsupported RNDC algorithm '{algorithm_str}'. Supported algorithms: hmac-md5, hmac-sha1, hmac-sha224, hmac-sha256, hmac-sha384, hmac-sha512"),
            };

            return Ok(RndcKeyData {
                name,
                algorithm,
                secret,
            });
        }

        // Fall back to parsing the rndc.key file (external Secret format)
        if let Some(rndc_key_bytes) = data.get("rndc.key") {
            let rndc_key_content = std::str::from_utf8(rndc_key_bytes)?;
            return Self::parse_rndc_key_file(rndc_key_content);
        }

        anyhow::bail!(
            "Secret must contain either (key-name, algorithm, secret) or rndc.key field. \
             For external secrets, provide only 'rndc.key' with the BIND9 key file content."
        )
    }

    /// Parse a BIND9 key file (rndc.key format) to extract key metadata.
    ///
    /// Expected format:
    /// ```text
    /// key "key-name" {
    ///     algorithm hmac-sha256;
    ///     secret "base64secret==";
    /// };
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the file format is invalid or required fields are missing.
    fn parse_rndc_key_file(content: &str) -> Result<RndcKeyData> {
        // Simple regex-based parser for BIND9 key file format
        // Format: key "name" { algorithm algo; secret "secret"; };

        // Extract key name
        let name = content
            .lines()
            .find(|line| line.contains("key"))
            .and_then(|line| {
                line.split('"').nth(1) // Get the text between first pair of quotes
            })
            .context("Failed to parse key name from rndc.key file")?
            .to_string();

        // Extract algorithm
        let algorithm_str = content
            .lines()
            .find(|line| line.contains("algorithm"))
            .and_then(|line| {
                line.split_whitespace()
                    .nth(1) // After "algorithm"
                    .map(|s| s.trim_end_matches(';'))
            })
            .context("Failed to parse algorithm from rndc.key file")?;

        let algorithm = match algorithm_str {
            "hmac-md5" => crate::crd::RndcAlgorithm::HmacMd5,
            "hmac-sha1" => crate::crd::RndcAlgorithm::HmacSha1,
            "hmac-sha224" => crate::crd::RndcAlgorithm::HmacSha224,
            "hmac-sha256" => crate::crd::RndcAlgorithm::HmacSha256,
            "hmac-sha384" => crate::crd::RndcAlgorithm::HmacSha384,
            "hmac-sha512" => crate::crd::RndcAlgorithm::HmacSha512,
            _ => anyhow::bail!("Unsupported algorithm '{algorithm_str}' in rndc.key file"),
        };

        // Extract secret
        let secret = content
            .lines()
            .find(|line| line.contains("secret"))
            .and_then(|line| {
                line.split('"').nth(1) // Get the text between first pair of quotes
            })
            .context("Failed to parse secret from rndc.key file")?
            .to_string();

        Ok(RndcKeyData {
            name,
            algorithm,
            secret,
        })
    }

    /// Execute a request to the bindcar API.
    ///
    /// Low-level helper that handles authentication, logging, and error handling
    /// for all communication with the bindcar HTTP API sidecar.
    ///
    /// # Arguments
    /// * `method` - HTTP method (GET, POST, DELETE)
    /// * `url` - Full URL to the bindcar API endpoint
    /// * `body` - Optional JSON body for POST requests
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the API returns an error.
    async fn bindcar_request<T: Serialize + std::fmt::Debug>(
        &self,
        method: &str,
        url: &str,
        body: Option<&T>,
    ) -> Result<String> {
        // Log the HTTP request
        info!(
            method = %method,
            url = %url,
            body = ?body,
            "HTTP API request to bindcar"
        );

        // Build the HTTP request
        let request = match method {
            "GET" => self.client.get(url),
            "POST" => {
                let mut req = self.client.post(url);
                if let Some(body_data) = body {
                    req = req.json(body_data);
                }
                req
            }
            "DELETE" => self.client.delete(url),
            _ => anyhow::bail!("Unsupported HTTP method: {method}"),
        };

        // Execute the request
        let response = request
            .header("Authorization", format!("Bearer {}", &*self.token))
            .send()
            .await
            .context(format!("Failed to send HTTP request to {url}"))?;

        let status = response.status();

        // Handle error responses
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(
                method = %method,
                url = %url,
                status = %status,
                error = %error_text,
                "HTTP API request failed"
            );
            anyhow::bail!(
                "HTTP request '{method} {url}' failed with status {status}: {error_text}"
            );
        }

        // Read response body
        let text = response
            .text()
            .await
            .context("Failed to read response body")?;

        info!(
            method = %method,
            url = %url,
            status = %status,
            response_len = text.len(),
            "HTTP API request successful"
        );

        Ok(text)
    }

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
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones/{zone_name}/reload");

        let result = self.bindcar_request("POST", &url, None::<&()>).await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("not found") || err_msg.contains("does not exist") {
                    Err(anyhow::anyhow!("Zone {zone_name} not found on {server}"))
                } else {
                    Err(e).context("Failed to reload zone")
                }
            }
        }
    }

    /// Reload all zones via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn reload_all_zones(&self, server: &str) -> Result<()> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/server/reload");

        self.bindcar_request("POST", &url, None::<&()>)
            .await
            .context("Failed to reload all zones")?;

        Ok(())
    }

    /// Trigger zone transfer via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone transfer cannot be initiated.
    pub async fn retransfer_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones/{zone_name}/retransfer");

        self.bindcar_request("POST", &url, None::<&()>)
            .await
            .context("Failed to retransfer zone")?;

        Ok(())
    }

    /// Freeze a zone to prevent dynamic updates via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be frozen.
    pub async fn freeze_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones/{zone_name}/freeze");

        self.bindcar_request("POST", &url, None::<&()>)
            .await
            .context("Failed to freeze zone")?;

        Ok(())
    }

    /// Thaw a frozen zone to allow dynamic updates via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be thawed.
    pub async fn thaw_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones/{zone_name}/thaw");

        self.bindcar_request("POST", &url, None::<&()>)
            .await
            .context("Failed to thaw zone")?;

        Ok(())
    }

    /// Get zone status via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone status cannot be retrieved.
    pub async fn zone_status(&self, zone_name: &str, server: &str) -> Result<String> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones/{zone_name}/status");

        let status = self
            .bindcar_request("GET", &url, None::<&()>)
            .await
            .context("Failed to get zone status")?;

        Ok(status)
    }

    /// Check if a zone exists by trying to get its status.
    ///
    /// Returns `true` if the zone exists and can be queried, `false` otherwise.
    pub async fn zone_exists(&self, zone_name: &str, server: &str) -> bool {
        match self.zone_status(zone_name, server).await {
            Ok(_) => {
                debug!("Zone {zone_name} exists on {server}");
                true
            }
            Err(e) => {
                debug!("Zone {zone_name} does not exist on {server}: {e}");
                false
            }
        }
    }

    /// Get server status via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the server status cannot be retrieved.
    pub async fn server_status(&self, server: &str) -> Result<String> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/server/status");

        let status = self
            .bindcar_request("GET", &url, None::<&()>)
            .await
            .context("Failed to get server status")?;

        Ok(status)
    }

    /// Add a new zone via HTTP API.
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
    /// * `zone_type` - Zone type (use `ZONE_TYPE_PRIMARY` or `ZONE_TYPE_SECONDARY` constants)
    /// * `server` - API endpoint (e.g., "bind9-primary-api:8080")
    /// * `key_data` - RNDC key data (used for allow-update configuration)
    /// * `name_server_ips` - Optional map of nameserver hostnames to IP addresses for glue records
    /// * `secondary_ips` - Optional list of secondary server IPs for also-notify and allow-transfer
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be added.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::too_many_arguments
    )]
    pub async fn add_zone(
        &self,
        zone_name: &str,
        zone_type: &str,
        server: &str,
        key_data: &RndcKeyData,
        soa_record: &crate::crd::SOARecord,
        name_server_ips: Option<&HashMap<String, String>>,
        secondary_ips: Option<&[String]>,
    ) -> Result<()> {
        // Check if zone already exists (idempotent)
        if self.zone_exists(zone_name, server).await {
            info!("Zone {zone_name} already exists on {server}, skipping add");
            return Ok(());
        }

        // Use the HTTP API to create a minimal zone
        // The bindcar API will handle zone file generation and allow-update configuration
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones");

        // Create zone configuration using SOA record from DNSZone spec
        let zone_config = ZoneConfig {
            ttl: DEFAULT_DNS_RECORD_TTL_SECS as u32,
            soa: SoaRecord {
                primary_ns: soa_record.primary_ns.clone(),
                admin_email: soa_record.admin_email.clone(),
                serial: soa_record.serial as u32,
                refresh: soa_record.refresh as u32,
                retry: soa_record.retry as u32,
                expire: soa_record.expire as u32,
                negative_ttl: soa_record.negative_ttl as u32,
            },
            name_servers: vec![soa_record.primary_ns.clone()],
            name_server_ips: name_server_ips.cloned().unwrap_or_default(),
            records: vec![],
            // Configure zone transfers to secondary servers
            also_notify: secondary_ips.map(<[String]>::to_vec),
            allow_transfer: secondary_ips.map(<[String]>::to_vec),
        };

        let request = CreateZoneRequest {
            zone_name: zone_name.to_string(),
            zone_type: zone_type.to_string(),
            zone_config,
            update_key_name: Some(key_data.name.clone()),
        };

        match self.bindcar_request("POST", &url, Some(&request)).await {
            Ok(_) => {
                if let Some(ips) = secondary_ips {
                    info!(
                        "Added zone {zone_name} on {server} with allow-update for key {} and zone transfers configured for {} secondary server(s): {:?}",
                        key_data.name, ips.len(), ips
                    );
                } else {
                    info!(
                        "Added zone {zone_name} on {server} with allow-update for key {} (no secondary servers)",
                        key_data.name
                    );
                }
                Ok(())
            }
            Err(e) => {
                let err_msg = e.to_string().to_lowercase();
                // Handle "zone already exists" errors as success (idempotent)
                // BIND9 can return various messages for duplicate zones:
                // - "already exists"
                // - "already serves the given zone"
                // - "duplicate zone"
                // HTTP 409 Conflict is also used for resource conflicts
                if err_msg.contains("already exists")
                    || err_msg.contains("already serves")
                    || err_msg.contains("duplicate zone")
                    || err_msg.contains("409")
                {
                    info!("Zone {zone_name} already exists on {server} (detected via API error), treating as success");
                    Ok(())
                } else {
                    Err(e).context("Failed to add zone")
                }
            }
        }
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
    pub async fn create_zone_http(
        &self,
        zone_name: &str,
        zone_type: &str,
        zone_config: ZoneConfig,
        server: &str,
        key_data: &RndcKeyData,
    ) -> Result<()> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones");

        let request = CreateZoneRequest {
            zone_name: zone_name.to_string(),
            zone_type: zone_type.to_string(),
            zone_config,
            update_key_name: Some(key_data.name.clone()),
        };

        debug!(
            zone_name = %zone_name,
            zone_type = %zone_type,
            server = %server,
            "Creating zone via HTTP API"
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", &*self.token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context(format!("Failed to send HTTP request to {url}"))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(
                zone_name = %zone_name,
                server = %server,
                status = %status,
                error = %error_text,
                "Failed to create zone via HTTP API"
            );
            anyhow::bail!(
                "Failed to create zone '{zone_name}' via HTTP API: {status} - {error_text}"
            );
        }

        let result: ZoneResponse = response
            .json()
            .await
            .context("Failed to parse API response")?;

        if !result.success {
            error!(
                zone_name = %zone_name,
                server = %server,
                message = %result.message,
                details = ?result.details,
                "API returned error when creating zone"
            );
            anyhow::bail!("Failed to create zone '{}': {}", zone_name, result.message);
        }

        info!(
            zone_name = %zone_name,
            server = %server,
            message = %result.message,
            "Zone created successfully via HTTP API"
        );

        Ok(())
    }

    /// Delete a zone via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the zone cannot be deleted.
    pub async fn delete_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        // First freeze the zone to prevent updates
        let _ = self.freeze_zone(zone_name, server).await;

        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones/{zone_name}");

        self.bindcar_request("DELETE", &url, None::<&()>)
            .await
            .context("Failed to delete zone")?;

        info!("Deleted zone {zone_name} from {server}");
        Ok(())
    }

    /// Notify secondaries about zone changes via HTTP API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the notification cannot be sent.
    pub async fn notify_zone(&self, zone_name: &str, server: &str) -> Result<()> {
        let base_url = Self::build_api_url(server);
        let url = format!("{base_url}/api/v1/zones/{zone_name}/notify");

        self.bindcar_request("POST", &url, None::<&()>)
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

    /// Create a TSIG signer from RNDC key data.
    ///
    /// # Errors
    ///
    /// Returns an error if the algorithm is unsupported or key data is invalid.
    fn create_tsig_signer(key_data: &RndcKeyData) -> Result<TSigner> {
        // Map RndcAlgorithm to hickory TsigAlgorithm
        let algorithm = match key_data.algorithm {
            crate::crd::RndcAlgorithm::HmacMd5 => TsigAlgorithm::HmacMd5,
            crate::crd::RndcAlgorithm::HmacSha1 => TsigAlgorithm::HmacSha1,
            crate::crd::RndcAlgorithm::HmacSha224 => TsigAlgorithm::HmacSha224,
            crate::crd::RndcAlgorithm::HmacSha256 => TsigAlgorithm::HmacSha256,
            crate::crd::RndcAlgorithm::HmacSha384 => TsigAlgorithm::HmacSha384,
            crate::crd::RndcAlgorithm::HmacSha512 => TsigAlgorithm::HmacSha512,
        };

        // Decode the base64 key
        let key_bytes = BASE64
            .decode(&key_data.secret)
            .context("Failed to decode TSIG key")?;

        // Create TSIG signer
        let signer = TSigner::new(
            key_bytes,
            algorithm,
            Name::from_str(&key_data.name).context("Invalid TSIG key name")?,
            u16::try_from(TSIG_FUDGE_TIME_SECS).unwrap_or(300),
        )
        .context("Failed to create TSIG signer")?;

        Ok(signer)
    }

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
        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let ipv4_str = ipv4.to_string();
        let server_str = server.to_string();
        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

        // Clone key_data for the blocking task
        let key_data = key_data.clone();

        // Execute DNS update in blocking thread (hickory-client is sync)
        tokio::task::spawn_blocking(move || {
            // Parse server address
            let server_addr = server_str
                .parse::<std::net::SocketAddr>()
                .with_context(|| format!("Invalid server address: {server_str}"))?;

            // Create UDP connection
            let conn =
                UdpClientConnection::new(server_addr).context("Failed to create UDP connection")?;

            // Create TSIG signer
            let signer = Self::create_tsig_signer(&key_data)?;

            // Create client with TSIG
            let client = SyncClient::with_tsigner(conn, signer);

            // Parse zone name
            let zone = Name::from_str(&zone_name_str)
                .with_context(|| format!("Invalid zone name: {zone_name_str}"))?;

            // Build full record name
            let fqdn = if name_str == "@" || name_str.is_empty() {
                zone.clone()
            } else {
                Name::from_str(&format!("{name_str}.{zone_name_str}"))
                    .with_context(|| format!("Invalid record name: {name_str}.{zone_name_str}"))?
            };

            // Parse IPv4 address
            let ipv4_addr = Ipv4Addr::from_str(&ipv4_str)
                .with_context(|| format!("Invalid IPv4 address: {ipv4_str}"))?;

            // Create A record
            let mut record =
                Record::from_rdata(fqdn.clone(), ttl_value, RData::A(ipv4_addr.into()));
            record.set_dns_class(DNSClass::IN);

            // Send update using append for idempotent operation
            // append() adds the record to the RRset, or creates a new RRset if none exists
            // must_exist=false means no prerequisite check - truly idempotent
            info!(
                "Adding A record: {} -> {} (TTL: {})",
                fqdn, ipv4_str, ttl_value
            );
            let response = client
                .append(record, zone.clone(), false)
                .with_context(|| format!("Failed to add A record for {fqdn}"))?;

            // Check response code
            match response.response_code() {
                ResponseCode::NoError => {
                    info!("Successfully added A record: {} -> {}", name_str, ipv4_str);
                    Ok(())
                }
                code => Err(anyhow::anyhow!(
                    "DNS update failed with response code: {code:?}"
                )),
            }
        })
        .await
        .context("DNS update task failed")?
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
        use hickory_client::rr::rdata;
        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let target_str = target.to_string();
        let server_str = server.to_string();
        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
        let key_data = key_data.clone();

        tokio::task::spawn_blocking(move || {
            let server_addr = server_str.parse::<std::net::SocketAddr>()?;
            let conn = UdpClientConnection::new(server_addr)?;
            let signer = Self::create_tsig_signer(&key_data)?;
            let client = SyncClient::with_tsigner(conn, signer);

            let zone = Name::from_str(&zone_name_str)?;
            let fqdn = if name_str == "@" || name_str.is_empty() {
                zone.clone()
            } else {
                Name::from_str(&format!("{name_str}.{zone_name_str}"))?
            };

            let target_name = Name::from_str(&target_str)?;
            let cname_rdata = rdata::CNAME(target_name);
            let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::CNAME(cname_rdata));
            record.set_dns_class(DNSClass::IN);

            info!(
                "Adding CNAME record: {} -> {} (TTL: {})",
                record.name(),
                target_str,
                ttl_value
            );
            // Use append for idempotent operation (must_exist=false for no prerequisites)
            let response = client.append(record, zone, false)?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!(
                        "Successfully added CNAME record: {} -> {}",
                        name_str, target_str
                    );
                    Ok(())
                }
                code => Err(anyhow::anyhow!(
                    "DNS update failed with response code: {code:?}"
                )),
            }
        })
        .await?
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
        use hickory_client::rr::rdata;
        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let texts_vec: Vec<String> = texts.to_vec();
        let server_str = server.to_string();
        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
        let key_data = key_data.clone();

        tokio::task::spawn_blocking(move || {
            let server_addr = server_str.parse::<std::net::SocketAddr>()?;
            let conn = UdpClientConnection::new(server_addr)?;
            let signer = Self::create_tsig_signer(&key_data)?;
            let client = SyncClient::with_tsigner(conn, signer);

            let zone = Name::from_str(&zone_name_str)?;
            let fqdn = if name_str == "@" || name_str.is_empty() {
                zone.clone()
            } else {
                Name::from_str(&format!("{name_str}.{zone_name_str}"))?
            };

            let txt_rdata = rdata::TXT::new(texts_vec.clone());
            let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::TXT(txt_rdata));
            record.set_dns_class(DNSClass::IN);

            info!(
                "Adding TXT record: {} -> {:?} (TTL: {})",
                record.name(),
                texts_vec,
                ttl_value
            );
            // Use append for idempotent operation (must_exist=false for no prerequisites)
            let response = client.append(record, zone, false)?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!("Successfully added TXT record: {}", name_str);
                    Ok(())
                }
                code => Err(anyhow::anyhow!(
                    "DNS update failed with response code: {code:?}"
                )),
            }
        })
        .await?
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
        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let ipv6_str = ipv6.to_string();
        let server_str = server.to_string();
        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
        let key_data = key_data.clone();

        tokio::task::spawn_blocking(move || {
            let server_addr = server_str
                .parse::<std::net::SocketAddr>()
                .with_context(|| format!("Invalid server address: {server_str}"))?;
            let conn =
                UdpClientConnection::new(server_addr).context("Failed to create UDP connection")?;
            let signer = Self::create_tsig_signer(&key_data)?;
            let client = SyncClient::with_tsigner(conn, signer);

            let zone = Name::from_str(&zone_name_str)
                .with_context(|| format!("Invalid zone name: {zone_name_str}"))?;
            let fqdn = if name_str == "@" || name_str.is_empty() {
                zone.clone()
            } else {
                Name::from_str(&format!("{name_str}.{zone_name_str}"))
                    .with_context(|| format!("Invalid record name: {name_str}.{zone_name_str}"))?
            };

            let ipv6_addr = Ipv6Addr::from_str(&ipv6_str)
                .with_context(|| format!("Invalid IPv6 address: {ipv6_str}"))?;
            let mut record =
                Record::from_rdata(fqdn.clone(), ttl_value, RData::AAAA(ipv6_addr.into()));
            record.set_dns_class(DNSClass::IN);

            // Use append for idempotent operation (must_exist=false for no prerequisites)
            info!(
                "Adding AAAA record: {} -> {} (TTL: {})",
                fqdn, ipv6_str, ttl_value
            );
            let response = client
                .append(record, zone.clone(), false)
                .with_context(|| format!("Failed to add AAAA record for {fqdn}"))?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!(
                        "Successfully added AAAA record: {} -> {}",
                        name_str, ipv6_str
                    );
                    Ok(())
                }
                code => Err(anyhow::anyhow!(
                    "DNS update failed with response code: {code:?}"
                )),
            }
        })
        .await
        .context("DNS update task failed")?
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
        use hickory_client::rr::rdata;
        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let mail_server_str = mail_server.to_string();
        let server_str = server.to_string();
        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
        let priority_u16 = u16::try_from(priority).unwrap_or(10);
        let key_data = key_data.clone();

        tokio::task::spawn_blocking(move || {
            let server_addr = server_str.parse::<std::net::SocketAddr>()?;
            let conn = UdpClientConnection::new(server_addr)?;
            let signer = Self::create_tsig_signer(&key_data)?;
            let client = SyncClient::with_tsigner(conn, signer);

            let zone = Name::from_str(&zone_name_str)?;
            let fqdn = if name_str == "@" || name_str.is_empty() {
                zone.clone()
            } else {
                Name::from_str(&format!("{name_str}.{zone_name_str}"))?
            };

            let mx_name = Name::from_str(&mail_server_str)?;
            let mx_rdata = rdata::MX::new(priority_u16, mx_name);
            let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::MX(mx_rdata));
            record.set_dns_class(DNSClass::IN);

            // Use append for idempotent operation (must_exist=false for no prerequisites)
            info!(
                "Adding MX record: {} -> {} (priority: {}, TTL: {})",
                fqdn, mail_server_str, priority_u16, ttl_value
            );
            let response = client.append(record, zone, false)?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!(
                        "Successfully added MX record: {} -> {}",
                        name_str, mail_server_str
                    );
                    Ok(())
                }
                code => Err(anyhow::anyhow!(
                    "DNS update failed with response code: {code:?}"
                )),
            }
        })
        .await?
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
        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let nameserver_str = nameserver.to_string();
        let server_str = server.to_string();
        let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
            .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
        let key_data = key_data.clone();

        tokio::task::spawn_blocking(move || {
            let server_addr = server_str.parse::<std::net::SocketAddr>()?;
            let conn = UdpClientConnection::new(server_addr)?;
            let signer = Self::create_tsig_signer(&key_data)?;
            let client = SyncClient::with_tsigner(conn, signer);

            let zone = Name::from_str(&zone_name_str)?;
            let fqdn = if name_str == "@" || name_str.is_empty() {
                zone.clone()
            } else {
                Name::from_str(&format!("{name_str}.{zone_name_str}"))?
            };

            let ns_name = Name::from_str(&nameserver_str)?;
            let mut record =
                Record::from_rdata(fqdn.clone(), ttl_value, RData::NS(rdata::NS(ns_name)));
            record.set_dns_class(DNSClass::IN);

            // Use append for idempotent operation (must_exist=false for no prerequisites)
            info!(
                "Adding NS record: {} -> {} (TTL: {})",
                fqdn, nameserver_str, ttl_value
            );
            let response = client.append(record, zone, false)?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!(
                        "Successfully added NS record: {} -> {}",
                        name_str, nameserver_str
                    );
                    Ok(())
                }
                code => Err(anyhow::anyhow!(
                    "DNS update failed with response code: {code:?}"
                )),
            }
        })
        .await?
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
        use hickory_client::rr::rdata;
        use std::str::FromStr;

        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let target_str = srv_data.target.clone();
        let port = srv_data.port;
        let priority = srv_data.priority;
        let weight = srv_data.weight;
        let ttl = srv_data.ttl;
        let server_str = server.to_string();
        let key_data = key_data.clone();

        tokio::task::spawn_blocking(move || {
            let server_addr = server_str.parse::<std::net::SocketAddr>().context(format!(
                "Invalid server address for SRV record update: {server_str}"
            ))?;

            let conn = UdpClientConnection::new(server_addr)
                .context("Failed to create UDP connection for SRV record")?;

            let signer = Self::create_tsig_signer(&key_data)
                .context("Failed to create TSIG signer for SRV record")?;

            let client = SyncClient::with_tsigner(conn, signer);

            let fqdn_str = if name_str.is_empty() || name_str == "@" {
                zone_name_str.clone()
            } else {
                format!("{name_str}.{zone_name_str}")
            };

            let fqdn = Name::from_str(&fqdn_str)
                .context(format!("Invalid FQDN for SRV record: {fqdn_str}"))?;

            let zone = Name::from_str(&zone_name_str)
                .context(format!("Invalid zone name for SRV: {zone_name_str}"))?;

            let target_name = Name::from_str(&target_str)
                .context(format!("Invalid target for SRV record: {target_str}"))?;

            let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
                .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

            // Convert i32 to u16 for SRV record parameters
            let priority_u16 =
                u16::try_from(priority).context(format!("Invalid SRV priority: {priority}"))?;
            let weight_u16 =
                u16::try_from(weight).context(format!("Invalid SRV weight: {weight}"))?;
            let port_u16 = u16::try_from(port).context(format!("Invalid SRV port: {port}"))?;

            let record_data = rdata::SRV::new(priority_u16, weight_u16, port_u16, target_name);

            let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::SRV(record_data));
            record.set_dns_class(DNSClass::IN);

            // Use append for idempotent operation
            let response = client
                .append(record, zone.clone(), false)
                .context(format!("Failed to send SRV record update for {fqdn_str}"))?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!(
                        "Successfully added SRV record: {} -> {}:{} (priority: {}, weight: {}, TTL: {})",
                        fqdn_str, target_str, port, priority, weight, ttl_value
                    );
                }
                code => {
                    anyhow::bail!("DNS server rejected SRV record update for {fqdn_str}: {code:?}");
                }
            }

            Ok(())
        })
        .await
        .context("SRV record update task panicked")??;

        Ok(())
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
        use hickory_client::rr::rdata;
        use std::str::FromStr;

        let zone_name_str = zone_name.to_string();
        let name_str = name.to_string();
        let tag_str = tag.to_string();
        let value_str = value.to_string();
        let server_str = server.to_string();
        let key_data = key_data.clone();

        tokio::task::spawn_blocking(move || {
            let server_addr = server_str.parse::<std::net::SocketAddr>().context(format!(
                "Invalid server address for CAA record update: {server_str}"
            ))?;

            let conn = UdpClientConnection::new(server_addr)
                .context("Failed to create UDP connection for CAA record")?;

            let signer = Self::create_tsig_signer(&key_data)
                .context("Failed to create TSIG signer for CAA record")?;

            let client = SyncClient::with_tsigner(conn, signer);

            let fqdn_str = if name_str.is_empty() || name_str == "@" {
                zone_name_str.clone()
            } else {
                format!("{name_str}.{zone_name_str}")
            };

            let fqdn = Name::from_str(&fqdn_str)
                .context(format!("Invalid FQDN for CAA record: {fqdn_str}"))?;

            let zone = Name::from_str(&zone_name_str)
                .context(format!("Invalid zone name for CAA: {zone_name_str}"))?;

            let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
                .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

            // CAA flags: 0 = not critical, 128 = critical
            let issuer_critical = flags != 0;

            // Create CAA record based on tag type
            let record_data = match tag_str.as_str() {
                "issue" => {
                    // Parse value as domain name
                    let ca_name = if value_str.is_empty() {
                        None
                    } else {
                        Some(
                            Name::from_str(&value_str)
                                .context(format!("Invalid CA domain name: {value_str}"))?,
                        )
                    };
                    rdata::CAA::new_issue(issuer_critical, ca_name, Vec::new())
                }
                "issuewild" => {
                    let ca_name = if value_str.is_empty() {
                        None
                    } else {
                        Some(
                            Name::from_str(&value_str)
                                .context(format!("Invalid CA domain name: {value_str}"))?,
                        )
                    };
                    rdata::CAA::new_issuewild(issuer_critical, ca_name, Vec::new())
                }
                "iodef" => {
                    let url = Url::parse(&value_str)
                        .context(format!("Invalid iodef URL: {value_str}"))?;
                    rdata::CAA::new_iodef(issuer_critical, url)
                }
                _ => anyhow::bail!(
                    "Unsupported CAA tag: {tag_str}. Supported tags: issue, issuewild, iodef"
                ),
            };

            let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::CAA(record_data));
            record.set_dns_class(DNSClass::IN);

            // Use append for idempotent operation
            let response = client
                .append(record, zone.clone(), false)
                .context(format!("Failed to send CAA record update for {fqdn_str}"))?;

            match response.response_code() {
                ResponseCode::NoError => {
                    info!(
                        "Successfully added CAA record: {} -> {} {} \"{}\" (TTL: {})",
                        fqdn_str, flags, tag_str, value_str, ttl_value
                    );
                }
                code => {
                    anyhow::bail!("DNS server rejected CAA record update for {fqdn_str}: {code:?}");
                }
            }

            Ok(())
        })
        .await
        .context("CAA record update task panicked")??;

        Ok(())
    }
}

impl Default for Bind9Manager {
    fn default() -> Self {
        Self::new()
    }
}
