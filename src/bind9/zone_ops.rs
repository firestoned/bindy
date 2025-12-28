// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Zone HTTP API operations for BIND9 management.
//!
//! This module contains all zone management functions that interact with the bindcar HTTP API sidecar.

use super::types::RndcKeyData;
use anyhow::{Context, Result};
use bindcar::{CreateZoneRequest, SoaRecord, ZoneConfig, ZoneResponse};
use reqwest::Client as HttpClient;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

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

/// Execute a request to the bindcar API.
///
/// Low-level helper that handles authentication, logging, and error handling
/// for all communication with the bindcar HTTP API sidecar.
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Authentication token
/// * `method` - HTTP method (GET, POST, DELETE)
/// * `url` - Full URL to the bindcar API endpoint
/// * `body` - Optional JSON body for POST requests
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the API returns an error.
pub(crate) async fn bindcar_request<T: Serialize + std::fmt::Debug>(
    client: &HttpClient,
    token: &str,
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
        "GET" => client.get(url),
        "POST" => {
            let mut req = client.post(url);
            if let Some(body_data) = body {
                req = req.json(body_data);
            }
            req
        }
        "PATCH" => {
            let mut req = client.patch(url);
            if let Some(body_data) = body {
                req = req.json(body_data);
            }
            req
        }
        "DELETE" => client.delete(url),
        _ => anyhow::bail!("Unsupported HTTP method: {method}"),
    };

    // Execute the request
    let response = request
        .header("Authorization", format!("Bearer {token}"))
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
        anyhow::bail!("HTTP request '{method} {url}' failed with status {status}: {error_text}");
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
/// * `client` - HTTP client
/// * `token` - Authentication token
/// * `zone_name` - Name of the zone to reload
/// * `server` - API server address (e.g., "bind9-primary-api:8080")
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone cannot be reloaded.
pub async fn reload_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> Result<()> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}/reload");

    let result = bindcar_request(client, token, "POST", &url, None::<&()>).await;

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
pub async fn reload_all_zones(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    server: &str,
) -> Result<()> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/server/reload");

    bindcar_request(client, token, "POST", &url, None::<&()>)
        .await
        .context("Failed to reload all zones")?;

    Ok(())
}

/// Trigger zone transfer via HTTP API.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone transfer cannot be initiated.
pub async fn retransfer_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> Result<()> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}/retransfer");

    bindcar_request(client, token, "POST", &url, None::<&()>)
        .await
        .context("Failed to retransfer zone")?;

    Ok(())
}

/// Freeze a zone to prevent dynamic updates via HTTP API.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone cannot be frozen.
pub async fn freeze_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> Result<()> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}/freeze");

    bindcar_request(client, token, "POST", &url, None::<&()>)
        .await
        .context("Failed to freeze zone")?;

    Ok(())
}

/// Thaw a frozen zone to allow dynamic updates via HTTP API.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone cannot be thawed.
pub async fn thaw_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> Result<()> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}/thaw");

    bindcar_request(client, token, "POST", &url, None::<&()>)
        .await
        .context("Failed to thaw zone")?;

    Ok(())
}

/// Get zone status via HTTP API.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone status cannot be retrieved.
pub async fn zone_status(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> Result<String> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}/status");

    let status = bindcar_request(client, token, "GET", &url, None::<&()>)
        .await
        .context("Failed to get zone status")?;

    Ok(status)
}

/// Check if a zone exists by trying to get its status.
///
/// Returns `true` if the zone exists and can be queried, `false` otherwise.
pub async fn zone_exists(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> bool {
    match zone_status(client, token, zone_name, server).await {
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
pub async fn server_status(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    server: &str,
) -> Result<String> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/server/status");

    let status = bindcar_request(client, token, "GET", &url, None::<&()>)
        .await
        .context("Failed to get server status")?;

    Ok(status)
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
/// * `client` - HTTP client
/// * `token` - Authentication token
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
#[allow(clippy::implicit_hasher)]
pub async fn add_primary_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
    key_data: &RndcKeyData,
    soa_record: &crate::crd::SOARecord,
    name_server_ips: Option<&HashMap<String, String>>,
    secondary_ips: Option<&[String]>,
) -> Result<bool> {
    use bindcar::ZONE_TYPE_PRIMARY;

    // Check if zone already exists
    if zone_exists(client, token, zone_name, server).await {
        // Zone exists - update its configuration if we have secondary IPs to configure
        if let Some(ips) = secondary_ips {
            if !ips.is_empty() {
                info!(
                    "Zone {zone_name} already exists on {server}, updating also-notify and allow-transfer with {} secondary server(s)",
                    ips.len()
                );
                // Update the zone's also-notify and allow-transfer configuration
                // This is critical when secondary pods restart and get new IPs
                return update_primary_zone(client, token, zone_name, server, ips).await;
            }
        }

        // Zone exists but no secondary IPs to configure - skip
        info!(
            "Zone {zone_name} already exists on {server} with no secondary servers, skipping add"
        );
        return Ok(false);
    }

    // Use the HTTP API to create a minimal zone
    // The bindcar API will handle zone file generation and allow-update configuration
    let base_url = build_api_url(server);
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
        // Primary zones don't have primaries field (only secondary zones do)
        primaries: None,
    };

    let request = CreateZoneRequest {
        zone_name: zone_name.to_string(),
        zone_type: ZONE_TYPE_PRIMARY.to_string(),
        zone_config,
        update_key_name: Some(key_data.name.clone()),
    };

    match bindcar_request(client, token, "POST", &url, Some(&request)).await {
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
            Ok(true)
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
                Ok(false)
            } else {
                Err(e).context("Failed to add zone")
            }
        }
    }
}

/// Update an existing primary zone's configuration via HTTP API.
///
/// Updates a zone's `also-notify` and `allow-transfer` configuration without
/// deleting and re-adding the zone. This is used when secondary pod IPs change
/// (e.g., after pod restart) to keep zone transfer ACLs up to date.
///
/// **Implementation:** Uses bindcar's PATCH endpoint introduced in v0.4.0.
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Authentication token
/// * `zone_name` - Name of the zone (e.g., "example.com")
/// * `server` - API endpoint (e.g., "bind9-primary-api:8080")
/// * `secondary_ips` - Updated list of secondary server IPs for also-notify and allow-transfer
///
/// # Returns
///
/// Returns `Ok(true)` if the zone was updated, `Ok(false)` if no update was needed.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone cannot be updated.
pub async fn update_primary_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
    secondary_ips: &[String],
) -> Result<bool> {
    // Define the update request structure
    // IMPORTANT: Must match bindcar's ModifyZoneRequest which uses camelCase
    #[derive(Serialize, Debug)]
    #[serde(rename_all = "camelCase")]
    struct ZoneUpdateRequest {
        also_notify: Option<Vec<String>>,
        allow_transfer: Option<Vec<String>>,
    }

    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}");

    let update_request = ZoneUpdateRequest {
        also_notify: Some(secondary_ips.to_vec()),
        allow_transfer: Some(secondary_ips.to_vec()),
    };

    info!(
        "Updating zone {zone_name} on {server} with {} secondary server(s): {:?}",
        secondary_ips.len(),
        secondary_ips
    );

    // Use PATCH to update only the specified fields
    match bindcar_request(client, token, "PATCH", &url, Some(&update_request)).await {
        Ok(_) => {
            info!(
                "Successfully updated zone {zone_name} on {server} with also-notify and allow-transfer for {} secondary server(s)",
                secondary_ips.len()
            );
            Ok(true)
        }
        Err(e) => {
            let error_msg = e.to_string();
            // If the zone doesn't exist, we can't update it
            if error_msg.contains("not found") || error_msg.contains("404") {
                debug!("Zone {zone_name} not found on {server}, cannot update");
                Ok(false)
            } else {
                Err(e).context("Failed to update zone configuration")
            }
        }
    }
}

/// Add a secondary zone via HTTP API.
///
/// Creates a secondary zone configured to transfer from the specified primary servers.
/// This is idempotent - if the zone already exists, it returns success without re-adding.
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Authentication token
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
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
    key_data: &RndcKeyData,
    primary_ips: &[String],
) -> Result<bool> {
    use bindcar::ZONE_TYPE_SECONDARY;

    // Check if zone already exists (idempotent)
    if zone_exists(client, token, zone_name, server).await {
        info!("Zone {zone_name} already exists on {server}, skipping add");
        return Ok(false);
    }

    // Use the HTTP API to create a minimal secondary zone
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones");

    // Create zone configuration for secondary zone with primaries
    // Secondary zones don't need SOA/NS records as they are transferred from primary
    let zone_config = ZoneConfig {
        ttl: DEFAULT_DNS_RECORD_TTL_SECS as u32,
        soa: SoaRecord {
            primary_ns: "placeholder.example.com.".to_string(),
            admin_email: "admin.example.com.".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604_800,
            negative_ttl: 86400,
        },
        name_servers: vec![],
        name_server_ips: std::collections::HashMap::new(),
        records: vec![],
        also_notify: None,
        allow_transfer: None,
        primaries: Some(primary_ips.to_vec()),
    };

    let request = CreateZoneRequest {
        zone_name: zone_name.to_string(),
        zone_type: ZONE_TYPE_SECONDARY.to_string(),
        zone_config,
        update_key_name: Some(key_data.name.clone()),
    };

    match bindcar_request(client, token, "POST", &url, Some(&request)).await {
        Ok(_) => {
            info!(
                "Added secondary zone {zone_name} on {server} with primaries: {:?}",
                primary_ips
            );
            Ok(true)
        }
        Err(e) => {
            let err_msg = e.to_string().to_lowercase();
            // Handle "zone already exists" errors as success (idempotent)
            if err_msg.contains("already exists")
                || err_msg.contains("already serves")
                || err_msg.contains("duplicate zone")
                || err_msg.contains("409")
            {
                info!("Zone {zone_name} already exists on {server} (detected via API error), treating as success");
                Ok(false)
            } else {
                Err(e).context("Failed to add secondary zone")
            }
        }
    }
}

/// Add a zone via HTTP API (primary or secondary).
///
/// This is the centralized zone addition function that dispatches to either
/// `add_primary_zone` or `add_secondary_zone` based on the zone type.
///
/// This operation is idempotent - if the zone already exists, it returns success
/// without attempting to re-add it.
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Authentication token
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
/// Returns an error if:
/// - The HTTP request fails
/// - The zone cannot be added
/// - For primary zones: SOA record is None
/// - For secondary zones: `primary_ips` is None or empty
#[allow(clippy::too_many_arguments)]
#[allow(clippy::implicit_hasher)]
pub async fn add_zones(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    zone_type: &str,
    server: &str,
    key_data: &RndcKeyData,
    soa_record: Option<&crate::crd::SOARecord>,
    name_server_ips: Option<&HashMap<String, String>>,
    secondary_ips: Option<&[String]>,
    primary_ips: Option<&[String]>,
) -> Result<bool> {
    use bindcar::{ZONE_TYPE_PRIMARY, ZONE_TYPE_SECONDARY};

    match zone_type {
        ZONE_TYPE_PRIMARY => {
            let soa = soa_record
                .ok_or_else(|| anyhow::anyhow!("SOA record is required for primary zones"))?;

            add_primary_zone(
                client,
                token,
                zone_name,
                server,
                key_data,
                soa,
                name_server_ips,
                secondary_ips,
            )
            .await
        }
        ZONE_TYPE_SECONDARY => {
            let primaries = primary_ips
                .ok_or_else(|| anyhow::anyhow!("Primary IPs are required for secondary zones"))?;

            if primaries.is_empty() {
                anyhow::bail!("Primary IPs list cannot be empty for secondary zones");
            }

            add_secondary_zone(client, token, zone_name, server, key_data, primaries).await
        }
        _ => anyhow::bail!("Invalid zone type: {zone_type}. Must be 'primary' or 'secondary'"),
    }
}

/// Create a zone via HTTP API with structured configuration.
///
/// This method sends a POST request to the API sidecar to create a zone using
/// structured zone configuration from the bindcar library.
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Authentication token
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
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    zone_type: &str,
    zone_config: ZoneConfig,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let base_url = build_api_url(server);
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

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
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
        anyhow::bail!("Failed to create zone '{zone_name}' via HTTP API: {status} - {error_text}");
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
pub async fn delete_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> Result<()> {
    // First freeze the zone to prevent updates (ignore errors if zone doesn't exist)
    if let Err(e) = freeze_zone(client, token, zone_name, server).await {
        debug!(
            "Failed to freeze zone {} before deletion (zone may not exist): {}",
            zone_name, e
        );
    }

    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}");

    // Attempt to delete the zone - treat "not found" as success (idempotent)
    match bindcar_request(client, token, "DELETE", &url, None::<&()>).await {
        Ok(_) => {
            info!("Deleted zone {zone_name} from {server}");
            Ok(())
        }
        Err(e) => {
            let error_msg = e.to_string();
            // If the zone doesn't exist, consider it already deleted (idempotent)
            if error_msg.contains("not found") || error_msg.contains("404") {
                debug!("Zone {zone_name} already deleted from {server}");
                Ok(())
            } else {
                Err(e).context("Failed to delete zone")
            }
        }
    }
}

/// Notify secondaries about zone changes via HTTP API.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the notification cannot be sent.
pub async fn notify_zone(
    client: &Arc<HttpClient>,
    token: &Arc<String>,
    zone_name: &str,
    server: &str,
) -> Result<()> {
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}/notify");

    bindcar_request(client, token, "POST", &url, None::<&()>)
        .await
        .context("Failed to notify zone")?;

    info!("Notified secondaries for zone {zone_name} from {server}");
    Ok(())
}

#[cfg(test)]
#[path = "zone_ops_tests.rs"]
mod zone_ops_tests;
