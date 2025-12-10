// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Types and constants for BIND9 management.

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
pub const SERVICE_ACCOUNT_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";

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

#[cfg(test)]
#[path = "types_tests.rs"]
mod types_tests;
