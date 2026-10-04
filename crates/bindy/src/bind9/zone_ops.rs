// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

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
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::constants::{DEFAULT_DNS_RECORD_TTL_SECS, DNS_CONTAINER_PORT};
use bindy_controller_sdk::retry::{http_backoff, is_retryable_http_status, ExponentialBackoff};

/// Append the operand's DNS container port to each transfer endpoint, in the
/// compact `<ip>:<port>` form bindcar accepts (IPv6 addresses are bracketed:
/// `[2001:db8::1]:5353`).
///
/// `named` binds the unprivileged [`DNS_CONTAINER_PORT`] (5353), not 53, so a
/// secondary's `primaries` and a primary's `also-notify` endpoints must target
/// that port. bindcar (`0.7.2`+) parses this form and renders BIND's
/// `<ip> port <n>` syntax. `allow-transfer` is a port-agnostic ACL and must stay
/// bare IPs, so it deliberately does **not** use this helper.
fn with_transfer_port(ips: &[String]) -> Vec<String> {
    ips.iter()
        .map(|ip| {
            if ip.parse::<std::net::Ipv6Addr>().is_ok() {
                format!("[{ip}]:{DNS_CONTAINER_PORT}")
            } else {
                format!("{ip}:{DNS_CONTAINER_PORT}")
            }
        })
        .collect()
}

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

/// Extract the HTTP status code from an error, if it originated from a
/// bindcar [`HttpError`].
///
/// Works even when the error has been wrapped in additional `anyhow` context
/// (e.g. by `zone_status`), because `anyhow::Error::downcast_ref` sees
/// through context layers. Never string-match on `e.to_string()` for status
/// codes: it only prints the outermost context.
fn bindcar_http_status(err: &anyhow::Error) -> Option<StatusCode> {
    err.downcast_ref::<HttpError>()
        .map(|http_err| http_err.status)
}

/// Returns `true` if the error is a bindcar HTTP 404 Not Found response.
pub(crate) fn is_http_not_found(err: &anyhow::Error) -> bool {
    bindcar_http_status(err) == Some(StatusCode::NOT_FOUND)
}

/// Returns `true` if the error is a bindcar HTTP 409 Conflict response.
pub(crate) fn is_http_conflict(err: &anyhow::Error) -> bool {
    bindcar_http_status(err) == Some(StatusCode::CONFLICT)
}

/// Returns `true` if a bindcar/BIND9 message indicates the zone already exists.
///
/// BIND9 can return various messages for duplicate zones:
/// - "already exists" (including the BIND9 `zone X/IN: already exists` form)
/// - "already serves the given zone"
/// - "duplicate zone"
fn is_zone_already_exists_message(message: &str) -> bool {
    let msg = message.to_lowercase();
    msg.contains("already exists")
        || msg.contains("already serves")
        || msg.contains("duplicate zone")
}

/// Returns `true` if the error indicates the zone already exists — either an
/// HTTP 409 Conflict or a BIND9 "already exists"-style message.
fn is_zone_already_exists_error(err: &anyhow::Error) -> bool {
    is_http_conflict(err) || is_zone_already_exists_message(&err.to_string())
}

/// Build the API base URL from a server address
///
/// Converts "service-name.namespace.svc.cluster.local:8080" or "service-name:8080"
/// to `<http://service-name.namespace.svc.cluster.local:8080>` or `<http://service-name:8080>`
pub(crate) fn build_api_url(server: &str) -> String {
    build_api_url_with_scheme(server, false)
}

/// Build the base URL for a bindcar endpoint, choosing the scheme.
///
/// Call sites pass a bare `<pod-ip>:<port>`, so `tls` is what actually moves
/// the operator onto the encrypted transport (ADR-0004).
///
/// An explicit scheme already present on `server` is always preserved — in
/// both directions. Silently upgrading a configured `http://` would be
/// surprising, and silently downgrading an `https://` would be dangerous.
///
/// # Arguments
/// * `server` - endpoint, with or without a scheme
/// * `tls` - whether TLS is enabled for this instance
#[must_use]
pub(crate) fn build_api_url_with_scheme(server: &str, tls: bool) -> String {
    if server.starts_with("http://") || server.starts_with("https://") {
        return server.trim_end_matches('/').to_string();
    }

    let scheme = if tls { "https" } else { "http" };
    format!("{}://{}", scheme, server.trim_end_matches('/'))
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
    bindcar_request_with_backoff(client, token, method, url, body, http_backoff()).await
}

/// As [`bindcar_request`], giving up once `budget` has elapsed instead of
/// after the default two minutes.
///
/// For calls that must not hold a reconcile for long, such as zone deletion
/// against endpoints that may already be gone.
///
/// # Errors
///
/// Returns an error if the request fails after the budget is spent or hits a
/// non-retryable error.
pub(crate) async fn bindcar_request_within<T: Serialize + std::fmt::Debug>(
    client: &HttpClient,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&T>,
    budget: Duration,
) -> Result<String> {
    let mut backoff = http_backoff();
    backoff.max_elapsed_time = Some(budget);
    bindcar_request_with_backoff(client, token, method, url, body, backoff).await
}

async fn bindcar_request_with_backoff<T: Serialize + std::fmt::Debug>(
    client: &HttpClient,
    token: Option<&str>,
    method: &str,
    url: &str,
    body: Option<&T>,
    mut backoff: ExponentialBackoff,
) -> Result<String> {
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
                    // Never sleep past the retry budget.
                    let remaining = backoff
                        .max_elapsed_time
                        .map_or(duration, |max| max.saturating_sub(start_time.elapsed()));
                    tokio::time::sleep(duration.min(remaining)).await;
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
        // 404 Not Found - zone definitely doesn't exist. Inspect the typed
        // error in the chain: zone_status wraps the HttpError with anyhow
        // context, so string-matching on e.to_string() would never see "404".
        Err(e) if is_http_not_found(&e) => {
            debug!("Zone {zone_name} does not exist on {server}");
            Ok(false)
        }
        // Rate limiting - should retry
        Err(e) if bindcar_http_status(&e) == Some(StatusCode::TOO_MANY_REQUESTS) => {
            error!("Rate limited while checking if zone {zone_name} exists on {server}: {e}");
            Err(e).context("Rate limited while checking zone existence")
        }
        // Any other error is a transient failure that should be retried
        Err(e) => {
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
        // Configure zone transfers to secondary servers. NOTIFY targets the
        // secondaries' operand port (5353); allow-transfer is a bare-IP ACL.
        also_notify: secondary_ips.map(with_transfer_port),
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
            // Handle "zone already exists" errors (HTTP 409 Conflict or a
            // BIND9 duplicate-zone message) as success (idempotent)
            if is_zone_already_exists_error(&e) {
                info!("Zone {zone_name} already exists on {server} (HTTP 409 Conflict), treating as success");

                // Zone exists: bring its transfer ACLs (secondary pods get
                // new IPs when they restart) and its DNSSEC policy (set, or
                // inherited, after the zone was created) up to date.
                let ips = secondary_ips.filter(|ips| !ips.is_empty());
                if ips.is_some() || dnssec_policy.is_some() {
                    info!(
                        "Zone {zone_name} already exists on {server}, updating {} secondary server(s), dnssec-policy {:?}",
                        ips.map_or(0, <[String]>::len),
                        dnssec_policy
                    );
                    let _updated =
                        update_primary_zone(client, token, zone_name, server, ips, dnssec_policy)
                            .await?;
                }
                // IMPORTANT: Ok(false) because the zone was NOT newly added.
                // Returning true would trigger status updates and cause a
                // reconciliation loop.
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
/// * `secondary_ips` - Updated secondary server IPs for also-notify and
///   allow-transfer; `None` leaves both as they are
/// * `dnssec_policy` - The zone's `dnssec-policy`; `None` leaves signing as
///   it is (bindcar's merge semantics, its ADR-0001), never "unsign"
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
    secondary_ips: Option<&[String]>,
    dnssec_policy: Option<&str>,
) -> Result<bool> {
    // Define the update request structure
    // IMPORTANT: Must match bindcar's ModifyZoneRequest which uses camelCase.
    // Absent fields are left unchanged by bindcar, so None is never sent.
    #[derive(Serialize, Debug)]
    #[serde(rename_all = "camelCase")]
    struct ZoneUpdateRequest {
        #[serde(skip_serializing_if = "Option::is_none")]
        also_notify: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        allow_transfer: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        dnssec_policy: Option<String>,
    }

    let base_url = build_api_url(server);
    let url = format!("{base_url}/api/v1/zones/{zone_name}");

    let update_request = ZoneUpdateRequest {
        // NOTIFY targets the secondaries' operand port (5353); allow-transfer is
        // a bare-IP ACL.
        also_notify: secondary_ips.map(with_transfer_port),
        allow_transfer: secondary_ips.map(<[String]>::to_vec),
        dnssec_policy: dnssec_policy.map(String::from),
    };

    let secondary_count = secondary_ips.map_or(0, <[String]>::len);
    info!(
        "Updating zone {zone_name} on {server}: {secondary_count} secondary server(s), dnssec-policy {dnssec_policy:?}"
    );

    // Use PATCH to update only the specified fields
    match bindcar_request(client, token, "PATCH", &url, Some(&update_request)).await {
        Ok(_) => {
            info!(
                "Successfully updated zone {zone_name} on {server} ({secondary_count} secondary server(s), dnssec-policy {dnssec_policy:?})"
            );
            Ok(true)
        }
        // If the zone doesn't exist, we can't update it
        Err(e) if is_http_not_found(&e) => {
            debug!("Zone {zone_name} not found on {server}, cannot update");
            Ok(false)
        }
        Err(e) => Err(e).context("Failed to update zone configuration"),
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

    // Create zone configuration for secondary zone with primaries.
    // Secondary zones don't need SOA/NS records as they are transferred from
    // the primary. The operand `named` listens on the unprivileged
    // DNS_CONTAINER_PORT (5353), so each primary endpoint is port-qualified
    // (`<ip>:5353`); bindcar (0.7.2+) parses this and renders BIND's
    // `<ip> port 5353` primaries syntax.
    let primaries: Vec<String> = with_transfer_port(primary_ips);

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
        primaries: Some(primaries),
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
        // Handle "zone already exists" errors (HTTP 409 Conflict or a BIND9
        // duplicate-zone message) as success (idempotent)
        Err(e) if is_zone_already_exists_error(&e) => {
            info!("Zone {zone_name} already exists on {server} (HTTP 409 Conflict), treating as success");
            Ok(false)
        }
        Err(e) => Err(e).context("Failed to add secondary zone"),
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
/// This method sends a POST request to the API sidecar (via the shared
/// `bindcar_request` retry path, so it gets the same retry/backoff and
/// timeout behavior as all other zone operations) to create a zone using
/// structured zone configuration from the bindcar library.
///
/// This operation is idempotent: an HTTP 409 Conflict (or a BIND9
/// "already exists"-style message) is treated as success.
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
    token: Option<&str>,
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

    let body = match bindcar_request(client, token, "POST", &url, Some(&request)).await {
        Ok(body) => body,
        // HTTP 409 Conflict (or an "already exists" message) means the zone
        // is already present - treat as success (idempotent)
        Err(e) if is_zone_already_exists_error(&e) => {
            info!("Zone {zone_name} already exists on {server}, treating as success");
            return Ok(());
        }
        Err(e) => {
            error!(
                zone_name = %zone_name,
                server = %server,
                error = %e,
                "Failed to create zone via HTTP API"
            );
            return Err(e)
                .with_context(|| format!("Failed to create zone '{zone_name}' via HTTP API"));
        }
    };

    let result: ZoneResponse =
        serde_json::from_str(&body).context("Failed to parse API response")?;

    if !result.success {
        // Check if the error message indicates zone already exists (idempotent)
        if is_zone_already_exists_message(&result.message) {
            info!("Zone {zone_name} already exists on {server} (detected via API response), treating as success");
            return Ok(());
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

/// Retry budget for each HTTP call made while deleting a zone.
///
/// Long enough to ride out a bindcar restart; short enough that an endpoint
/// whose pod is gone cannot hold the zone's reconcile for minutes. With the
/// default two-minute budget a single dead endpoint cost over four minutes,
/// and a zone recreated with the same name waited behind it (bug-192).
pub const DELETE_RETRY_BUDGET: Duration = Duration::from_secs(10);

/// Delete a zone via HTTP API.
///
/// Deleting a zone that is not on the server succeeds: its status is checked
/// first, and bindcar answers that with 404 for a missing zone (whereas it
/// answers `rndc delzone` on a missing zone with a retryable 500). The zone is
/// not frozen first: `rndc delzone` does not need it, and a freeze followed by
/// a failed delete would leave the zone refusing dynamic updates.
///
/// # Arguments
/// * `client` - HTTP client
/// * `token` - Authentication token
/// * `zone_name` - Name of the zone to delete
/// * `server` - API server address
///
/// # Errors
///
/// Returns an error if the zone is present and cannot be deleted within
/// [`DELETE_RETRY_BUDGET`] per call.
pub async fn delete_zone(
    client: &Arc<HttpClient>,
    token: Option<&str>,
    zone_name: &str,
    server: &str,
) -> Result<()> {
    delete_zone_within(client, token, zone_name, server, DELETE_RETRY_BUDGET).await
}

/// [`delete_zone`] with an explicit per-call retry budget.
///
/// # Errors
///
/// Returns an error if the zone is present and cannot be deleted within
/// `budget` per call.
pub(crate) async fn delete_zone_within(
    client: &Arc<HttpClient>,
    token: Option<&str>,
    zone_name: &str,
    server: &str,
    budget: Duration,
) -> Result<()> {
    let base_url = build_api_url(server);

    // Is the zone there at all? A missing zone is already deleted.
    let status_url = format!("{base_url}/api/v1/zones/{zone_name}/status");
    match bindcar_request_within(client, token, "GET", &status_url, None::<&()>, budget).await {
        Ok(_) => {}
        Err(e) if is_http_not_found(&e) => {
            debug!("Zone {zone_name} is not on {server}; nothing to delete");
            return Ok(());
        }
        Err(e) => return Err(e).context("Failed to check zone before deletion"),
    }

    let url = format!("{base_url}/api/v1/zones/{zone_name}");
    match bindcar_request_within(client, token, "DELETE", &url, None::<&()>, budget).await {
        Ok(_) => {
            info!("Deleted zone {zone_name} from {server}");
            Ok(())
        }
        // Gone between the check and the delete (idempotent).
        Err(e) if is_http_not_found(&e) => {
            debug!("Zone {zone_name} already deleted from {server}");
            Ok(())
        }
        Err(e) => Err(e).context("Failed to delete zone"),
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
/// * `server` - The DNS server address (e.g., "bind9-primary.bindy-system.svc.cluster.local:53")
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
///     "10.0.0.1:53"
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
    use hickory_net::client::{Client, ClientHandle};
    use hickory_net::runtime::TokioRuntimeProvider;
    use hickory_net::udp::UdpClientStream;
    use hickory_proto::rr::{DNSClass, Name, RecordType};
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

    // Create UDP client connection (unauthenticated read-only query).
    let stream = UdpClientStream::builder(server_addr, TokioRuntimeProvider::default()).build();
    let (mut client, bg) = Client::<TokioRuntimeProvider>::from_sender(stream);

    // Spawn the background task that drives the connection.
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
    let is_signed = !response.answers.is_empty();

    if is_signed {
        debug!(
            "Zone {} is signed with DNSSEC (found {} DNSKEY record(s))",
            zone_name,
            response.answers.len()
        );
    } else {
        debug!(
            "Zone {} is not signed with DNSSEC (no DNSKEY records found)",
            zone_name
        );
    }

    Ok(is_signed)
}

// ============================================================================
// DNSSEC DS record extraction (ADR-0006, roadmap 07 Phase 5)
// ============================================================================

/// DS digest type published in `DNSZone` status: SHA-256 (digest type 2).
///
/// RFC 8624 makes SHA-256 the mandatory-to-implement DS digest; SHA-1 is
/// deprecated and deliberately not emitted.
const DS_DIGEST_TYPE: hickory_proto::dnssec::DigestType = hickory_proto::dnssec::DigestType::SHA256;

/// One DS (Delegation Signer) record derived from a zone's KSK DNSKEY.
///
/// Carries the fields `DNSZone.status.dnssec` publishes: the key tag, the
/// algorithm mnemonic, and the full presentation-format record the user
/// pastes into the parent zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsRecordInfo {
    /// RFC 4034 key tag of the KSK this DS record refers to.
    pub key_tag: u16,
    /// DNSSEC algorithm mnemonic (e.g., `ECDSAP256SHA256`).
    pub algorithm: String,
    /// Presentation-format DS record:
    /// `<zone>. IN DS <keytag> <algorithm> 2 <sha256-digest-hex>`
    pub presentation: String,
}

/// Rewrite a bindcar API endpoint (`<host>:<port>`) to the operand's DNS
/// endpoint on [`DNS_CONTAINER_PORT`].
///
/// Bracketed IPv6 (`[2001:db8::1]:8080`) keeps its brackets; a bare host
/// without a port gets the DNS port appended.
///
/// # Arguments
/// * `endpoint` - The `<host>:<port>` endpoint the zone was configured through
#[must_use]
pub fn dns_query_endpoint(endpoint: &str) -> String {
    if let Some(bracket_end) = endpoint.rfind(']') {
        let host = &endpoint[..=bracket_end];
        return format!("{host}:{DNS_CONTAINER_PORT}");
    }

    match endpoint.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{host}:{DNS_CONTAINER_PORT}")
        }
        _ => format!("{endpoint}:{DNS_CONTAINER_PORT}"),
    }
}

/// Derive DS records from a zone's DNSKEY RRset.
///
/// Only Key Signing Keys produce DS records: the key must have the zone-key
/// and SEP (secure entry point) flags set and must not be revoked. ZSKs and
/// revoked keys are skipped. An unsigned zone (no DNSKEYs) yields an empty
/// vector, not an error.
///
/// # Arguments
/// * `zone_name` - The zone, with or without a trailing dot
/// * `dnskeys` - DNSKEY RDATA from the zone's apex
///
/// # Errors
/// Returns an error if the zone name is invalid, or a key tag / digest
/// cannot be computed from a DNSKEY.
pub fn ds_records_from_dnskeys(
    zone_name: &str,
    dnskeys: &[hickory_proto::dnssec::rdata::DNSKEY],
) -> Result<Vec<DsRecordInfo>> {
    use hickory_proto::dnssec::PublicKey;
    use hickory_proto::rr::Name;
    use std::str::FromStr;

    let zone = zone_name.trim_end_matches('.');
    let name = Name::from_str(&format!("{zone}."))
        .with_context(|| format!("Invalid zone name: {zone_name}"))?;

    let mut ds_records = Vec::new();
    for key in dnskeys {
        // KSKs only: zone-key + SEP flags, and never a revoked key.
        if !key.zone_key() || !key.secure_entry_point() || key.revoke() {
            continue;
        }

        let key_tag = key
            .calculate_key_tag()
            .with_context(|| format!("Failed to calculate DNSKEY key tag for zone {zone}"))?;
        let digest = key
            .to_digest(&name, DS_DIGEST_TYPE)
            .with_context(|| format!("Failed to compute DS digest for zone {zone}"))?;

        let digest_hex: String = digest
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect();
        let algorithm = key.public_key().algorithm();
        let algorithm_number = u8::from(algorithm);
        let digest_type_number = u8::from(DS_DIGEST_TYPE);

        ds_records.push(DsRecordInfo {
            key_tag,
            algorithm: algorithm.as_str().to_string(),
            presentation: format!(
                "{zone}. IN DS {key_tag} {algorithm_number} {digest_type_number} {digest_hex}"
            ),
        });
    }

    Ok(ds_records)
}

/// Query a zone's DNSKEY RRset and derive its DS records (ADR-0006).
///
/// Queries the operand directly over DNS (read-only, unauthenticated — DNSKEY
/// data is public by design) and derives one DS record per KSK. An unsigned
/// zone returns an empty vector.
///
/// # Arguments
/// * `zone_name` - The zone to query, with or without a trailing dot
/// * `server` - DNS endpoint as `<host>:<port>` (see [`dns_query_endpoint`])
///
/// # Errors
/// Returns an error if the server address or zone name is invalid, the DNS
/// query fails, or DS derivation fails.
pub async fn extract_ds_records(zone_name: &str, server: &str) -> Result<Vec<DsRecordInfo>> {
    use hickory_net::client::{Client, ClientHandle};
    use hickory_net::runtime::TokioRuntimeProvider;
    use hickory_net::udp::UdpClientStream;
    use hickory_proto::dnssec::rdata::DNSSECRData;
    use hickory_proto::rr::{DNSClass, Name, RData, RecordType};
    use std::net::SocketAddr;
    use std::str::FromStr;

    let server_addr: SocketAddr = server
        .parse()
        .with_context(|| format!("Invalid DNS server address: {server}"))?;

    let name =
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))?;

    debug!(
        "Extracting DS records for zone {} from {}",
        zone_name, server_addr
    );

    let stream = UdpClientStream::builder(server_addr, TokioRuntimeProvider::default()).build();
    let (mut client, bg) = Client::<TokioRuntimeProvider>::from_sender(stream);
    tokio::spawn(bg);

    let response = client
        .query(name, DNSClass::IN, RecordType::DNSKEY)
        .await
        .with_context(|| {
            format!("Failed to query DNSKEY records for zone {zone_name} on {server_addr}")
        })?;

    let dnskeys: Vec<_> = response
        .answers
        .iter()
        .filter_map(|record| {
            if let RData::DNSSEC(DNSSECRData::DNSKEY(dnskey)) = &record.data {
                Some(dnskey.clone())
            } else {
                None
            }
        })
        .collect();

    ds_records_from_dnskeys(zone_name, &dnskeys)
}

// ============================================================================
// DNSSEC key timing from the sidecar's zone status (bindcar 0.8.1+, ADR-0006)
// ============================================================================

/// Parse the DNSSEC block out of a bindcar zone-status response body.
///
/// bindcar 0.8.1+ returns `ZoneStatusResponse` JSON whose optional `dnssec`
/// field carries the state parsed from `rndc dnssec -status`. Returns `None`
/// for older sidecars, unsigned zones, or an unparsable body — key timing is
/// best-effort and must never fail a reconcile.
///
/// # Arguments
/// * `body` - The raw zone-status response body
#[must_use]
pub fn parse_zone_status_dnssec(body: &str) -> Option<bindcar::DnssecStatus> {
    serde_json::from_str::<bindcar::zones_types::ZoneStatusResponse>(body)
        .ok()
        .and_then(|response| response.dnssec)
}

/// The next scheduled KSK rollover event for a zone, if BIND reports one.
///
/// Only keys with the key-signing duty (KSK/CSK) are considered — a ZSK
/// rollover does not change the DS at the parent, and
/// `DNSZone.status.dnssec.keyTag` describes the KSK. Removed keys are
/// skipped; with several eligible keys the earliest event wins (the
/// timestamps share one ISO 8601 format, so lexicographic order is
/// chronological).
///
/// # Arguments
/// * `status` - The zone's DNSSEC status from the sidecar
#[must_use]
pub fn next_ksk_rollover(status: &bindcar::DnssecStatus) -> Option<String> {
    status
        .keys
        .iter()
        .filter(|key| key.key_signing && !key.removed)
        .filter_map(|key| key.next_rollover.as_ref())
        .min()
        .cloned()
}

#[cfg(test)]
#[path = "zone_ops_tests.rs"]
mod zone_ops_tests;
