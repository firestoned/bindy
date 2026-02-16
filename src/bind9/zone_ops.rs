// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Zone HTTP API operations for BIND9 management.
//!
//! This module contains all zone management functions that interact with the bindcar HTTP API sidecar.

use super::types::RndcKeyData;
use anyhow::{Context, Result};
use bindcar::{CreateZoneRequest, SoaRecord, ZoneConfig, ZoneResponse};
use reqwest::{Client as HttpClient, StatusCode};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;
use crate::reconcilers::retry::{http_backoff, is_retryable_http_status};

/// HTTP error with status code for retry logic.
///
/// This error type preserves the HTTP status code so we can determine
/// if the error is retryable (429, 5xx) without parsing error strings.
#[derive(Debug)]
struct HttpError {
    status: StatusCode,
    message: String,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for HttpError {}

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

/// Execute a request to the bindcar API with automatic retry.
///
/// This is the main entry point for all bindcar HTTP API calls. It wraps the internal
/// `bindcar_request_internal` with exponential backoff retry logic.
///
/// # Retry Behavior
/// - Retries on HTTP 429, 500, 502, 503, 504
/// - Fails immediately on other 4xx errors
/// - Max 2 minutes total retry time
/// - Initial retry after 50ms, exponentially growing to max 10 seconds
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Optional authentication token (None if auth disabled)
/// * `method` - HTTP method (GET, POST, DELETE)
/// * `url` - Full URL to the bindcar API endpoint
/// * `body` - Optional JSON body for POST requests
///
/// # Errors
///
/// Returns an error if the HTTP request fails after all retries or encounters a non-retryable error.
pub(crate) async fn bindcar_request<T: Serialize + std::fmt::Debug>(
    client: &HttpClient,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&T>,
) -> Result<String> {
    let mut backoff = http_backoff();
    let start_time = Instant::now();
    let mut attempt = 0;

    loop {
        attempt += 1;

        let result = bindcar_request_internal(client, token, method, url, body).await;

        match result {
            Ok(response) => {
                if attempt > 1 {
                    debug!(
                        method = %method,
                        url = %url,
                        attempt = attempt,
                        elapsed = ?start_time.elapsed(),
                        "HTTP API call succeeded after retries"
                    );
                }
                return Ok(response);
            }
            Err(e) => {
                // Determine if the error is retryable by checking the error type
                let mut is_retryable = false;

                // Check if this is an HttpError (which contains the actual status code)
                if let Some(http_err) = e.downcast_ref::<HttpError>() {
                    is_retryable = is_retryable_http_status(http_err.status);
                } else {
                    // For non-HTTP errors, check if it's a network error
                    let error_msg = e.to_string();
                    if error_msg.contains("Failed to send") || error_msg.contains("connection") {
                        is_retryable = true;
                    }
                }

                if !is_retryable {
                    error!(
                        method = %method,
                        url = %url,
                        error = %e,
                        "Non-retryable HTTP API error, failing immediately"
                    );
                    return Err(e);
                }

                // Check if we've exceeded max elapsed time
                if let Some(max_elapsed) = backoff.max_elapsed_time {
                    if start_time.elapsed() >= max_elapsed {
                        error!(
                            method = %method,
                            url = %url,
                            attempt = attempt,
                            elapsed = ?start_time.elapsed(),
                            error = %e,
                            "Max retry time exceeded, giving up"
                        );
                        return Err(anyhow::anyhow!(
                            "Max retry time exceeded after {attempt} attempts: {e}"
                        ));
                    }
                }

                // Calculate next backoff interval
                if let Some(duration) = backoff.next_backoff() {
                    warn!(
                        method = %method,
                        url = %url,
                        attempt = attempt,
                        retry_after = ?duration,
                        error = %e,
                        "Retryable HTTP API error, will retry"
                    );
                    tokio::time::sleep(duration).await;
                } else {
                    error!(
                        method = %method,
                        url = %url,
                        attempt = attempt,
                        elapsed = ?start_time.elapsed(),
                        error = %e,
                        "Backoff exhausted, giving up"
                    );
                    return Err(anyhow::anyhow!(
                        "Backoff exhausted after {attempt} attempts: {e}"
                    ));
                }
            }
        }
    }
}

/// Internal implementation of bindcar API requests without retry logic.
///
/// This function handles the actual HTTP communication. It should not be called directly;
/// use `bindcar_request` instead, which wraps this with retry logic.
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Optional authentication token (None if auth disabled)
/// * `method` - HTTP method (GET, POST, DELETE)
/// * `url` - Full URL to the bindcar API endpoint
/// * `body` - Optional JSON body for POST requests
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the API returns an error.
async fn bindcar_request_internal<T: Serialize + std::fmt::Debug>(
    client: &HttpClient,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&T>,
) -> Result<String> {
    // Log the HTTP request
    info!(
        method = %method,
        url = %url,
        body = ?body,
        auth_enabled = token.is_some(),
        "HTTP API request to bindcar"
    );

    // Build the HTTP request
    let mut request = match method {
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

    // Add Authorization header only if token is provided (auth enabled)
    if let Some(token_value) = token {
        request = request.header("Authorization", format!("Bearer {token_value}"));
    }

    // Execute the request
    let response = request
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
        return Err(HttpError {
            status,
            message: error_text,
        }
        .into());
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
/// * `token` - Optional authentication token (None if auth disabled)
/// * `zone_name` - Name of the zone to reload
/// * `server` - API server address (e.g., "bind9-primary-api:8080")
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone cannot be reloaded.
pub async fn reload_zone(
    client: &Arc<HttpClient>,
    token: Option<&str>,
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
    token: Option<&str>,
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
    token: Option<&str>,
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
    token: Option<&str>,
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
    token: Option<&str>,
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
    token: Option<&str>,
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
/// Returns `Ok(true)` if the zone exists and can be queried, `Ok(false)` if the zone
/// definitely does not exist (404), or `Err` for transient errors (rate limiting, network
/// errors, server errors, etc.) that should be retried.
///
/// # Errors
///
/// Returns an error if:
/// - The server is rate limiting requests (429 Too Many Requests)
/// - Network connectivity issues occur
/// - The server returns a 5xx error
/// - Any other non-404 error occurs
pub async fn zone_exists(
    client: &Arc<HttpClient>,
    token: Option<&str>,
    zone_name: &str,
    server: &str,
) -> Result<bool> {
    match zone_status(client, token, zone_name, server).await {
        Ok(_) => {
            debug!("Zone {zone_name} exists on {server}");
            Ok(true)
        }
        Err(e) => {
            let err_msg = e.to_string();

            // 404 Not Found - zone definitely doesn't exist
            if err_msg.contains("404") || err_msg.contains("not found") {
                debug!("Zone {zone_name} does not exist on {server}");
                return Ok(false);
            }

            // Rate limiting - should retry
            if err_msg.contains("429") || err_msg.contains("Too Many Requests") {
                error!("Rate limited while checking if zone {zone_name} exists on {server}: {e}");
                return Err(e).context("Rate limited while checking zone existence");
            }

            // Any other error is a transient failure that should be retried
            error!("Error checking if zone {zone_name} exists on {server}: {e}");
            Err(e).context("Failed to check zone existence")
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
    token: Option<&str>,
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
/// * `name_servers` - Optional list of ALL authoritative nameserver hostnames (including primary from SOA)
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
    token: Option<&str>,
    zone_name: &str,
    server: &str,
    key_data: &RndcKeyData,
    soa_record: &crate::crd::SOARecord,
    name_servers: Option<&[String]>,
    name_server_ips: Option<&HashMap<String, String>>,
    secondary_ips: Option<&[String]>,
    dnssec_policy: Option<&str>,
) -> Result<bool> {
    use bindcar::ZONE_TYPE_PRIMARY;

    // Use the HTTP API to create a minimal zone
    // Idempotency is handled in the error path below (lines 434-446)
    // The bindcar API will handle zone file generation and allow-update configuration
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones");

    // Build list of all authoritative nameservers
    // Priority: use provided name_servers list if available, otherwise fall back to primary NS from SOA
    let all_name_servers = if let Some(ns_list) = name_servers {
        ns_list.to_vec()
    } else {
        // Fallback: only primary NS from SOA
        vec![soa_record.primary_ns.clone()]
    };

    // Log DNSSEC configuration if provided
    if let Some(policy) = dnssec_policy {
        info!("DNSSEC policy '{policy}' will be applied to zone {zone_name} on {server}");
    }

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
        name_servers: all_name_servers,
        name_server_ips: name_server_ips.cloned().unwrap_or_default(),
        records: vec![],
        // Configure zone transfers to secondary servers
        also_notify: secondary_ips.map(<[String]>::to_vec),
        allow_transfer: secondary_ips.map(<[String]>::to_vec),
        // Primary zones don't have primaries field (only secondary zones do)
        primaries: None,
        // DNSSEC configuration (bindcar 0.6.0+)
        dnssec_policy: dnssec_policy.map(String::from),
        inline_signing: dnssec_policy.map(|_| true),
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
            // Handle "zone already exists" errors as success (idempotent)
            // Check if this is an HttpError with 409 Conflict status code
            let is_conflict = e
                .downcast_ref::<HttpError>()
                .is_some_and(|http_err| http_err.status == StatusCode::CONFLICT);

            let err_msg = e.to_string().to_lowercase();
            // BIND9 can return various messages for duplicate zones:
            // - "already exists"
            // - "already serves the given zone"
            // - "duplicate zone"
            // - "zone X/IN: already exists" (BIND9 format)
            // HTTP 409 Conflict means the zone already exists
            if is_conflict
                || err_msg.contains("already exists")
                || err_msg.contains("already serves")
                || err_msg.contains("duplicate zone")
            {
                info!("Zone {zone_name} already exists on {server} (HTTP 409 Conflict), treating as success");

                // Zone exists - check if we need to update its configuration with secondary IPs
                if let Some(ips) = secondary_ips {
                    if !ips.is_empty() {
                        info!(
                            "Zone {zone_name} already exists on {server}, updating also-notify and allow-transfer with {} secondary server(s)",
                            ips.len()
                        );
                        // Update the zone's also-notify and allow-transfer configuration
                        // This is critical when secondary pods restart and get new IPs
                        let _updated =
                            update_primary_zone(client, token, zone_name, server, ips).await?;
                        // IMPORTANT: Return Ok(false) because the zone was NOT newly added, it already existed
                        // Returning true here would trigger status updates and cause a reconciliation loop
                        return Ok(false);
                    }
                }

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
    token: Option<&str>,
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
    token: Option<&str>,
    zone_name: &str,
    server: &str,
    key_data: &RndcKeyData,
    primary_ips: &[String],
) -> Result<bool> {
    use bindcar::ZONE_TYPE_SECONDARY;

    // Use the HTTP API to create a minimal secondary zone
    // Idempotency is handled in the error path below (lines 609-616)
    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones");

    // Create zone configuration for secondary zone with primaries
    // Secondary zones don't need SOA/NS records as they are transferred from primary
    // CRITICAL: Append port number to primary IPs for BIND9 zone transfers
    // BIND9 defaults to port 53, but we use DNS_CONTAINER_PORT (5353)
    // Format: "IP port PORT" (e.g., "10.244.1.82 port 5353")
    let primaries_with_port: Vec<String> = primary_ips
        .iter()
        .map(|ip| format!("{} port {}", ip, crate::constants::DNS_CONTAINER_PORT))
        .collect();

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
        primaries: Some(primaries_with_port),
        // Secondary zones don't need DNSSEC policy (they receive signed zones via transfer)
        dnssec_policy: None,
        inline_signing: None,
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
                request.zone_config.primaries
            );
            Ok(true)
        }
        Err(e) => {
            // Handle "zone already exists" errors as success (idempotent)
            // Check if this is an HttpError with 409 Conflict status code
            let is_conflict = e
                .downcast_ref::<HttpError>()
                .is_some_and(|http_err| http_err.status == StatusCode::CONFLICT);

            let err_msg = e.to_string().to_lowercase();
            // BIND9 can return various messages for duplicate zones:
            // - "already exists"
            // - "already serves the given zone"
            // - "duplicate zone"
            // - "zone X/IN: already exists" (BIND9 format)
            // HTTP 409 Conflict means the zone already exists
            if is_conflict
                || err_msg.contains("already exists")
                || err_msg.contains("already serves")
                || err_msg.contains("duplicate zone")
            {
                info!("Zone {zone_name} already exists on {server} (HTTP 409 Conflict), treating as success");
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
/// * `name_servers` - Optional list of ALL authoritative nameserver hostnames (for primary zones)
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
    token: Option<&str>,
    zone_name: &str,
    zone_type: &str,
    server: &str,
    key_data: &RndcKeyData,
    soa_record: Option<&crate::crd::SOARecord>,
    name_servers: Option<&[String]>,
    name_server_ips: Option<&HashMap<String, String>>,
    secondary_ips: Option<&[String]>,
    primary_ips: Option<&[String]>,
    dnssec_policy: Option<&str>,
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
                name_servers,
                name_server_ips,
                secondary_ips,
                dnssec_policy,
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
#[allow(clippy::too_many_lines)]
pub async fn create_zone_http(
    client: &Arc<HttpClient>,
    token: Option<&str>,
    zone_name: &str,
    zone_type: &str,
    zone_config: ZoneConfig,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    // Check if zone already exists (idempotent)
    match zone_exists(client, token, zone_name, server).await {
        Ok(true) => {
            info!("Zone {zone_name} already exists on {server}, skipping creation");
            return Ok(());
        }
        Ok(false) => {
            // Zone doesn't exist, proceed with creation
        }
        Err(e) => {
            // Can't check zone existence (rate limiting, network error, etc.)
            // Propagate the error rather than blindly attempting creation
            return Err(e).context("Failed to check if zone exists before creation");
        }
    }

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

    let mut post_request = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request);

    // Add Authorization header only if token is provided
    if let Some(token_value) = token {
        post_request = post_request.header("Authorization", format!("Bearer {token_value}"));
    }

    let response = post_request
        .send()
        .await
        .context(format!("Failed to send HTTP request to {url}"))?;

    let status = response.status();

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        // Check if it's a "zone already exists" error (idempotent)
        let error_lower = error_text.to_lowercase();
        let zone_check_result = zone_exists(client, token, zone_name, server).await;
        if error_lower.contains("already exists")
            || error_lower.contains("already serves")
            || error_lower.contains("duplicate zone")
            || status.as_u16() == 409
            || matches!(zone_check_result, Ok(true))
        {
            info!("Zone {zone_name} already exists on {server} (detected via API error or existence check), treating as success");
            return Ok(());
        }

        // If we can't check zone existence due to rate limiting or other errors, propagate that
        if let Err(zone_err) = zone_check_result {
            return Err(zone_err).context("Failed to verify zone existence after creation error");
        }

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
        // Check if the error message indicates zone already exists (idempotent)
        let msg_lower = result.message.to_lowercase();
        let zone_check_result = zone_exists(client, token, zone_name, server).await;
        if msg_lower.contains("already exists")
            || msg_lower.contains("already serves")
            || msg_lower.contains("duplicate zone")
            || matches!(zone_check_result, Ok(true))
        {
            info!("Zone {zone_name} already exists on {server} (detected via API response), treating as success");
            return Ok(());
        }

        // If we can't check zone existence due to rate limiting or other errors, propagate that
        if let Err(zone_err) = zone_check_result {
            return Err(zone_err)
                .context("Failed to verify zone existence after API returned error");
        }

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
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Authentication token
/// * `zone_name` - Name of the zone to delete
/// * `server` - API server address
/// * `freeze_before_delete` - Whether to freeze the zone before deletion (true for primary zones, false for secondary zones)
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the zone cannot be deleted.
pub async fn delete_zone(
    client: &Arc<HttpClient>,
    token: Option<&str>,
    zone_name: &str,
    server: &str,
    freeze_before_delete: bool,
) -> Result<()> {
    // Freeze the zone before deletion if requested (only for primary zones)
    // Secondary zones should NOT be frozen as they are read-only
    if freeze_before_delete {
        if let Err(e) = freeze_zone(client, token, zone_name, server).await {
            debug!(
                "Failed to freeze zone {} before deletion (zone may not exist): {}",
                zone_name, e
            );
        }
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
    token: Option<&str>,
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

/// Verify that a zone is signed with DNSSEC by querying for DNSKEY records.
///
/// This function performs a DNS query to check if the zone has been signed
/// with DNSSEC. It queries for DNSKEY records, which are present in signed zones.
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name to verify (e.g., "example.com")
/// * `server` - The DNS server address (e.g., "bind9-primary.dns-system.svc.cluster.local:5353")
///
/// # Returns
///
/// * `Ok(true)` - Zone is signed (DNSKEY records found)
/// * `Ok(false)` - Zone is not signed (no DNSKEY records)
/// * `Err(_)` - Query failed (network error, invalid zone name, etc.)
///
/// # Errors
///
/// Returns an error if:
/// - The DNS server address cannot be parsed
/// - The zone name is invalid
/// - The DNS query fails (network error, timeout, etc.)
///
/// # Example
///
/// ```no_run
/// use bindy::bind9::zone_ops::verify_zone_signed;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let signed = verify_zone_signed(
///     "example.com",
///     "10.0.0.1:5353"
/// ).await?;
///
/// if signed {
///     println!("Zone is signed with DNSSEC");
/// } else {
///     println!("Zone is not signed");
/// }
/// # Ok(())
/// # }
/// ```
pub async fn verify_zone_signed(zone_name: &str, server: &str) -> Result<bool> {
    use hickory_client::client::{AsyncClient, ClientHandle};
    use hickory_client::rr::{DNSClass, Name, RecordType};
    use hickory_client::udp::UdpClientStream;
    use std::net::SocketAddr;
    use std::str::FromStr;

    // Parse server address
    let server_addr: SocketAddr = server
        .parse()
        .with_context(|| format!("Invalid DNS server address: {server}"))?;

    debug!(
        "Verifying DNSSEC signing for zone {} on {}",
        zone_name, server_addr
    );

    // Create UDP client connection
    let stream = UdpClientStream::<tokio::net::UdpSocket>::new(server_addr);
    let (mut client, bg) = AsyncClient::connect(stream).await?;

    // Spawn the background task
    tokio::spawn(bg);

    // Parse zone name
    let name =
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))?;

    // Query for DNSKEY records
    let response = client
        .query(name.clone(), DNSClass::IN, RecordType::DNSKEY)
        .await
        .with_context(|| {
            format!("Failed to query DNSKEY records for zone {zone_name} on {server_addr}")
        })?;

    // If we got DNSKEY records, the zone is signed
    let is_signed = !response.answers().is_empty();

    if is_signed {
        debug!(
            "Zone {} is signed with DNSSEC (found {} DNSKEY record(s))",
            zone_name,
            response.answers().len()
        );
    } else {
        debug!(
            "Zone {} is not signed with DNSSEC (no DNSKEY records found)",
            zone_name
        );
    }

    Ok(is_signed)
}

#[cfg(test)]
#[path = "zone_ops_tests.rs"]
mod zone_ops_tests;
