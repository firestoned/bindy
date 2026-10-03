// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS operation and HTTP API error types for Bindy.
//!
//! This module provides specialized error types for:
//! - Bindcar HTTP API operations (zone and record management)
//! - Hickory DNS client operations (dynamic updates, zone transfers)
//! - TSIG authentication failures
//! - Network connectivity issues with BIND9 instances
//!
//! These errors provide structured error handling for DNS operations,
//! enabling better error reporting in status conditions and metrics.

use thiserror::Error;

/// Errors that can occur during DNS zone operations via Bindcar HTTP API.
///
/// These errors represent failures when interacting with the Bindcar HTTP API
/// on BIND9 instances for zone management operations.
#[derive(Error, Debug, Clone)]
pub enum ZoneError {
    /// Zone not found (HTTP 404 from bindcar API)
    ///
    /// Returned when attempting to operate on a zone that doesn't exist on the BIND9 server.
    /// This could happen if the zone was deleted externally or was never created.
    #[error("Zone '{zone}' not found on endpoint {endpoint} (HTTP 404)")]
    ZoneNotFound {
        /// The zone name that was not found
        zone: String,
        /// The BIND9 endpoint (IP:port) that returned the error
        endpoint: String,
    },

    /// Failed to create a new zone (generic creation error)
    ///
    /// Returned when zone creation fails for reasons other than the zone already existing.
    /// This could be due to invalid zone configuration, filesystem errors, or BIND9 internal errors.
    #[error("Failed to create zone '{zone}' on endpoint {endpoint}: {reason}")]
    ZoneCreationFailed {
        /// The zone name that failed to create
        zone: String,
        /// The BIND9 endpoint (IP:port) where creation failed
        endpoint: String,
        /// Specific reason for the failure
        reason: String,
    },

    /// Zone already exists when attempting to create a new zone
    ///
    /// Returned when attempting to create a zone that already exists on the BIND9 server.
    /// This is typically a non-fatal error that can be safely ignored.
    #[error("Zone '{zone}' already exists on endpoint {endpoint}")]
    ZoneAlreadyExists {
        /// The zone name that already exists
        zone: String,
        /// The BIND9 endpoint (IP:port) where the zone exists
        endpoint: String,
    },

    /// Failed to delete a zone
    ///
    /// Returned when zone deletion fails. This could be due to the zone being in use,
    /// permissions issues, or BIND9 being unable to clean up zone files.
    #[error("Failed to delete zone '{zone}' on endpoint {endpoint}: {reason}")]
    ZoneDeletionFailed {
        /// The zone name that failed to delete
        zone: String,
        /// The BIND9 endpoint (IP:port) where deletion failed
        endpoint: String,
        /// Specific reason for the failure
        reason: String,
    },

    /// Invalid zone configuration
    ///
    /// Returned when the zone configuration is malformed or contains invalid parameters.
    /// This includes invalid SOA records, bad nameserver IPs, or invalid zone type.
    #[error("Invalid zone configuration for '{zone}': {reason}")]
    InvalidZoneConfiguration {
        /// The zone name with invalid configuration
        zone: String,
        /// Explanation of what is invalid
        reason: String,
    },
}

/// Errors that can occur during DNS record operations via Hickory DNS client.
///
/// These errors represent failures when performing dynamic DNS updates (nsupdate)
/// or querying DNS records using the Hickory DNS client library.
#[derive(Error, Debug, Clone)]
pub enum RecordError {
    /// DNS record not found when querying the primary server (NXDOMAIN or no records)
    ///
    /// Returned when querying for a DNS record that doesn't exist in the zone.
    /// This is typically returned as an NXDOMAIN response or an empty answer section.
    #[error("DNS record '{name}' in zone '{zone}' not found on server {server} (no answer)")]
    RecordNotFound {
        /// The record name (e.g., "www", "@")
        name: String,
        /// The zone name (e.g., "example.com")
        zone: String,
        /// The DNS server that was queried
        server: String,
    },

    /// Failed to add or update a DNS record via dynamic update
    ///
    /// Returned when a dynamic DNS update (nsupdate) fails. This could be due to
    /// TSIG authentication failure, zone not allowing updates, or invalid record data.
    #[error("Failed to update record '{name}.{zone}' on server {server}: {reason}")]
    RecordUpdateFailed {
        /// The record name being updated
        name: String,
        /// The zone containing the record
        zone: String,
        /// The DNS server where the update failed
        server: String,
        /// Specific reason for the failure
        reason: String,
    },

    /// Failed to delete a DNS record via dynamic update
    ///
    /// Returned when attempting to delete a DNS record via nsupdate fails.
    #[error("Failed to delete record '{name}.{zone}' on server {server}: {reason}")]
    RecordDeletionFailed {
        /// The record name being deleted
        name: String,
        /// The zone containing the record
        zone: String,
        /// The DNS server where deletion failed
        server: String,
        /// Specific reason for the failure
        reason: String,
    },

    /// Invalid record data (malformed IP, invalid FQDN, etc.)
    ///
    /// Returned when record data fails validation before attempting to create/update.
    /// This includes invalid IP addresses, malformed FQDNs, or out-of-range TTL values.
    #[error("Invalid record data for '{name}.{zone}': {reason}")]
    InvalidRecordData {
        /// The record name with invalid data
        name: String,
        /// The zone containing the record
        zone: String,
        /// Explanation of what is invalid
        reason: String,
    },
}

/// Errors related to BIND9 instance availability and connectivity.
///
/// These errors occur when the Bindcar HTTP API is unreachable or returns
/// gateway errors, indicating the BIND9 instance or bindcar service is unavailable.
#[derive(Error, Debug, Clone)]
pub enum InstanceError {
    /// BIND9 instance unavailable (HTTP 502 Bad Gateway or 503 Service Unavailable)
    ///
    /// Returned when the bindcar HTTP API returns a gateway error, indicating the
    /// underlying BIND9 instance is not responding or the bindcar process cannot
    /// communicate with BIND9.
    #[error("BIND9 instance at {endpoint} unavailable (HTTP {status_code})")]
    Bind9InstanceUnavailable {
        /// The endpoint (IP:port) that is unavailable
        endpoint: String,
        /// HTTP status code (502 or 503)
        status_code: u16,
    },

    /// HTTP connection failed (network unreachable, connection refused, timeout)
    ///
    /// Returned when the HTTP client cannot establish a connection to the bindcar API.
    /// This indicates network issues, the pod being down, or the service not listening.
    #[error("HTTP connection to {endpoint} failed: {reason}")]
    HttpConnectionFailed {
        /// The endpoint (IP:port) that couldn't be reached
        endpoint: String,
        /// Reason for the connection failure
        reason: String,
    },

    /// HTTP request timeout
    ///
    /// Returned when an HTTP request to bindcar exceeds the configured timeout.
    /// This typically indicates a heavily loaded or unresponsive BIND9 instance.
    #[error("HTTP request to {endpoint} timed out after {timeout_ms}ms")]
    HttpRequestTimeout {
        /// The endpoint (IP:port) that timed out
        endpoint: String,
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },

    /// Unexpected HTTP response from bindcar API
    ///
    /// Returned when bindcar returns an unexpected HTTP status code that doesn't
    /// map to a known error condition.
    #[error("Unexpected HTTP response from {endpoint}: {status_code} {reason}")]
    UnexpectedHttpResponse {
        /// The endpoint that returned the unexpected response
        endpoint: String,
        /// HTTP status code
        status_code: u16,
        /// Response body or error message
        reason: String,
    },
}

/// Errors related to TSIG (Transaction Signature) authentication for dynamic DNS updates.
///
/// TSIG is used to authenticate dynamic DNS update requests. These errors occur when
/// TSIG authentication fails due to invalid keys, mismatched algorithms, or replay attacks.
#[derive(Error, Debug, Clone)]
pub enum TsigError {
    /// TSIG connection or authentication error when attempting dynamic updates
    ///
    /// Returned when TSIG authentication fails. This could be due to:
    /// - Incorrect TSIG key name or secret
    /// - Mismatched TSIG algorithm between client and server
    /// - Clock skew between client and server
    /// - TSIG key not configured on the BIND9 server
    #[error("TSIG authentication failed for server {server}: {reason}")]
    TsigConnectionError {
        /// The DNS server (IP:port) where TSIG failed
        server: String,
        /// Specific reason for TSIG failure
        reason: String,
    },

    /// TSIG key not found in Kubernetes secret
    ///
    /// Returned when the expected TSIG key secret doesn't exist in Kubernetes.
    /// This indicates a configuration error or missing secret.
    #[error("TSIG key secret '{secret_name}' not found in namespace '{namespace}'")]
    TsigKeyNotFound {
        /// The Kubernetes secret name
        secret_name: String,
        /// The namespace where the secret should exist
        namespace: String,
    },

    /// Invalid TSIG key data in Kubernetes secret
    ///
    /// Returned when the TSIG secret exists but contains invalid data (missing fields,
    /// malformed base64, unsupported algorithm).
    #[error("Invalid TSIG key data in secret '{secret_name}': {reason}")]
    InvalidTsigKeyData {
        /// The Kubernetes secret name
        secret_name: String,
        /// Explanation of what is invalid
        reason: String,
    },

    /// TSIG verification failed (server rejected the signature)
    ///
    /// Returned when the BIND9 server rejects the TSIG signature on a dynamic update.
    /// This indicates the signature doesn't match, suggesting key mismatch or tampering.
    #[error("TSIG verification failed on server {server} for key '{key_name}'")]
    TsigVerificationFailed {
        /// The DNS server that rejected the signature
        server: String,
        /// The TSIG key name that was used
        key_name: String,
    },
}

/// Errors related to zone transfer operations (AXFR/IXFR).
///
/// These errors occur when attempting to trigger or perform zone transfers
/// between primary and secondary BIND9 instances.
#[derive(Error, Debug, Clone)]
pub enum ZoneTransferError {
    /// Zone transfer failed (AXFR or IXFR)
    ///
    /// Returned when a zone transfer from primary to secondary fails.
    /// This could be due to network issues, TSIG authentication failure,
    /// or the zone not being configured for transfer.
    #[error("Zone transfer for '{zone}' from {primary} to {secondary} failed: {reason}")]
    TransferFailed {
        /// The zone being transferred
        zone: String,
        /// The primary server IP
        primary: String,
        /// The secondary server IP
        secondary: String,
        /// Reason for the transfer failure
        reason: String,
    },

    /// Zone transfer not allowed by primary server
    ///
    /// Returned when the primary server refuses zone transfer (typically due to
    /// allow-transfer ACL configuration).
    #[error("Zone transfer for '{zone}' refused by primary {primary} (not in allow-transfer)")]
    TransferRefused {
        /// The zone being transferred
        zone: String,
        /// The primary server that refused
        primary: String,
    },

    /// Zone transfer timeout
    ///
    /// Returned when a zone transfer operation exceeds the timeout.
    /// This typically indicates a very large zone or network issues.
    #[error("Zone transfer for '{zone}' from {primary} timed out after {timeout_secs}s")]
    TransferTimeout {
        /// The zone being transferred
        zone: String,
        /// The primary server
        primary: String,
        /// Timeout in seconds
        timeout_secs: u64,
    },
}

/// Composite error type that encompasses all DNS operation errors.
///
/// This is the primary error type returned by Bindy's DNS operation functions.
/// It provides a unified interface for handling all possible DNS-related errors.
#[derive(Error, Debug, Clone)]
pub enum DnsError {
    /// Zone-related error (creation, deletion, not found, etc.)
    #[error(transparent)]
    Zone(#[from] ZoneError),

    /// DNS record-related error (update, query, deletion, etc.)
    #[error(transparent)]
    Record(#[from] RecordError),

    /// BIND9 instance or bindcar API unavailability
    #[error(transparent)]
    Instance(#[from] InstanceError),

    /// TSIG authentication or key management error
    #[error(transparent)]
    Tsig(#[from] TsigError),

    /// Zone transfer error
    #[error(transparent)]
    ZoneTransfer(#[from] ZoneTransferError),

    /// Generic error for operations that don't fit other categories
    #[error("DNS operation failed: {0}")]
    Generic(String),
}

impl DnsError {
    /// Returns true if this error is transient and the operation should be retried.
    ///
    /// Transient errors include network failures, timeouts, and temporary unavailability.
    /// Non-transient errors include not found, invalid data, and authentication failures.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            // Transient errors that should be retried
            Self::Zone(
                ZoneError::ZoneCreationFailed { .. } | ZoneError::ZoneDeletionFailed { .. },
            )
            | Self::Record(
                RecordError::RecordUpdateFailed { .. } | RecordError::RecordDeletionFailed { .. },
            )
            | Self::Instance(_)
            | Self::Tsig(TsigError::TsigConnectionError { .. })
            | Self::ZoneTransfer(
                ZoneTransferError::TransferFailed { .. }
                | ZoneTransferError::TransferTimeout { .. },
            )
            | Self::Generic(_) => true,

            // Permanent errors that should not be retried
            Self::Zone(
                ZoneError::ZoneNotFound { .. }
                | ZoneError::ZoneAlreadyExists { .. }
                | ZoneError::InvalidZoneConfiguration { .. },
            )
            | Self::Record(
                RecordError::RecordNotFound { .. } | RecordError::InvalidRecordData { .. },
            )
            | Self::Tsig(
                TsigError::TsigKeyNotFound { .. }
                | TsigError::InvalidTsigKeyData { .. }
                | TsigError::TsigVerificationFailed { .. },
            )
            | Self::ZoneTransfer(ZoneTransferError::TransferRefused { .. }) => false,
        }
    }

    /// Returns the Kubernetes status reason code for this error.
    ///
    /// This is used when updating CRD status conditions to provide
    /// structured error information.
    #[must_use]
    pub fn status_reason(&self) -> &'static str {
        match self {
            Self::Zone(ZoneError::ZoneNotFound { .. }) => "ZoneNotFound",
            Self::Zone(ZoneError::ZoneCreationFailed { .. }) => "ZoneCreationFailed",
            Self::Zone(ZoneError::ZoneAlreadyExists { .. }) => "ZoneAlreadyExists",
            Self::Zone(ZoneError::ZoneDeletionFailed { .. }) => "ZoneDeletionFailed",
            Self::Zone(ZoneError::InvalidZoneConfiguration { .. }) => "InvalidZoneConfiguration",

            Self::Record(RecordError::RecordNotFound { .. }) => "RecordNotFound",
            Self::Record(RecordError::RecordUpdateFailed { .. }) => "RecordUpdateFailed",
            Self::Record(RecordError::RecordDeletionFailed { .. }) => "RecordDeletionFailed",
            Self::Record(RecordError::InvalidRecordData { .. }) => "InvalidRecordData",

            Self::Instance(InstanceError::Bind9InstanceUnavailable { .. }) => {
                "Bind9InstanceUnavailable"
            }
            Self::Instance(InstanceError::HttpConnectionFailed { .. }) => "HttpConnectionFailed",
            Self::Instance(InstanceError::HttpRequestTimeout { .. }) => "HttpRequestTimeout",
            Self::Instance(InstanceError::UnexpectedHttpResponse { .. }) => {
                "UnexpectedHttpResponse"
            }

            Self::Tsig(TsigError::TsigConnectionError { .. }) => "TsigConnectionError",
            Self::Tsig(TsigError::TsigKeyNotFound { .. }) => "TsigKeyNotFound",
            Self::Tsig(TsigError::InvalidTsigKeyData { .. }) => "InvalidTsigKeyData",
            Self::Tsig(TsigError::TsigVerificationFailed { .. }) => "TsigVerificationFailed",

            Self::ZoneTransfer(ZoneTransferError::TransferFailed { .. }) => "ZoneTransferFailed",
            Self::ZoneTransfer(ZoneTransferError::TransferRefused { .. }) => "ZoneTransferRefused",
            Self::ZoneTransfer(ZoneTransferError::TransferTimeout { .. }) => "ZoneTransferTimeout",

            Self::Generic(_) => "DnsOperationFailed",
        }
    }
}

// Conversion from anyhow::Error to DnsError for backward compatibility
impl From<anyhow::Error> for DnsError {
    fn from(err: anyhow::Error) -> Self {
        Self::Generic(err.to_string())
    }
}
