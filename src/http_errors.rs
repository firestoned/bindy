// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! HTTP error code mapping to Kubernetes status condition reasons.
//!
//! This module provides utilities for mapping HTTP status codes from the Bindcar API
//! to standardized Kubernetes condition reasons. This enables consistent error handling
//! and troubleshooting across the operator.
//!
//! # Usage
//!
//! ```rust,no_run
//! use bindy::http_errors::{map_http_error_to_reason, map_connection_error};
//!
//! # async fn example() {
//! # let client = reqwest::Client::new();
//! // When making HTTP calls to Bindcar API
//! match client.get("http://localhost:8080/zones").send().await {
//!     Ok(response) => {
//!         if response.status().is_success() {
//!             // Handle success
//!         } else {
//!             let (reason, message) = map_http_error_to_reason(response.status().as_u16());
//!             // Set condition with this reason
//!         }
//!     }
//!     Err(e) => {
//!         let (reason, message) = map_connection_error();
//!         // Set condition for connection failure
//!     }
//! }
//! # }
//! ```

use crate::status_reasons::{
    REASON_BINDCAR_AUTH_FAILED, REASON_BINDCAR_BAD_REQUEST, REASON_BINDCAR_INTERNAL_ERROR,
    REASON_BINDCAR_NOT_IMPLEMENTED, REASON_BINDCAR_UNREACHABLE, REASON_GATEWAY_ERROR, REASON_READY,
    REASON_ZONE_NOT_FOUND,
};

/// Map HTTP status code to condition reason and message.
///
/// This function converts HTTP status codes from Bindcar API responses into
/// standardized Kubernetes condition reasons and human-readable messages.
///
/// # Arguments
///
/// * `status_code` - HTTP status code (e.g., 400, 404, 500)
///
/// # Returns
///
/// A tuple of `(reason, message)`:
/// - `reason` - Constant from `status_reasons` module
/// - `message` - Human-readable explanation of the error
///
/// # HTTP Code Mapping
///
/// | HTTP Code | Reason | Meaning |
/// |-----------|--------|---------|
/// | 400 | `BindcarBadRequest` | Invalid request format |
/// | 401 | `BindcarAuthFailed` | Authentication required |
/// | 403 | `BindcarAuthFailed` | Insufficient permissions |
/// | 404 | `ZoneNotFound` | Resource not found |
/// | 500 | `BindcarInternalError` | Internal server error |
/// | 501 | `BindcarNotImplemented` | Feature not implemented |
/// | 502 | `GatewayError` | Bad gateway |
/// | 503 | `GatewayError` | Service unavailable |
/// | 504 | `GatewayError` | Gateway timeout |
/// | Other | `BindcarUnreachable` | Unexpected error |
///
/// # Example
///
/// ```rust
/// use bindy::http_errors::map_http_error_to_reason;
///
/// let (reason, message) = map_http_error_to_reason(404);
/// assert_eq!(reason, "ZoneNotFound");
/// assert!(message.contains("404"));
///
/// let (reason, message) = map_http_error_to_reason(503);
/// assert_eq!(reason, "GatewayError");
/// assert!(message.contains("503"));
/// ```
#[must_use]
pub fn map_http_error_to_reason(status_code: u16) -> (&'static str, String) {
    match status_code {
        400 => (
            REASON_BINDCAR_BAD_REQUEST,
            "Invalid request to Bindcar API (400)".into(),
        ),
        401 => (
            REASON_BINDCAR_AUTH_FAILED,
            "Bindcar authentication required (401)".into(),
        ),
        403 => (
            REASON_BINDCAR_AUTH_FAILED,
            "Bindcar authorization failed (403)".into(),
        ),
        404 => (
            REASON_ZONE_NOT_FOUND,
            "Zone or resource not found in BIND9 (404)".into(),
        ),
        500 => (
            REASON_BINDCAR_INTERNAL_ERROR,
            "Bindcar API internal error (500)".into(),
        ),
        501 => (
            REASON_BINDCAR_NOT_IMPLEMENTED,
            "Operation not supported by Bindcar (501)".into(),
        ),
        502 => (
            REASON_GATEWAY_ERROR,
            "Bad gateway reaching Bindcar (502)".into(),
        ),
        503 => (
            REASON_GATEWAY_ERROR,
            "Bindcar service unavailable (503)".into(),
        ),
        504 => (
            REASON_GATEWAY_ERROR,
            "Gateway timeout reaching Bindcar (504)".into(),
        ),
        _ => (
            REASON_BINDCAR_UNREACHABLE,
            format!("Unexpected HTTP error from Bindcar ({status_code})"),
        ),
    }
}

/// Map connection error to condition reason and message.
///
/// Use this when the HTTP client cannot establish a connection to Bindcar,
/// before receiving any HTTP status code.
///
/// # Returns
///
/// A tuple of `(reason, message)`:
/// - `reason` - `REASON_BINDCAR_UNREACHABLE`
/// - `message` - Human-readable explanation
///
/// # Common Causes
///
/// - Bindcar container not running
/// - Network policy blocking traffic
/// - Bindcar listening on wrong port
/// - DNS resolution failure
///
/// # Example
///
/// ```rust,no_run
/// use bindy::http_errors::map_connection_error;
///
/// # async fn example() {
/// # let client = reqwest::Client::new();
/// match client.get("http://localhost:8080/zones").send().await {
///     Ok(response) => { /* handle response */ }
///     Err(e) => {
///         let (reason, message) = map_connection_error();
///         // Set condition: Pod-0 status=False reason=BindcarUnreachable
///     }
/// }
/// # }
/// ```
#[must_use]
pub fn map_connection_error() -> (&'static str, String) {
    (
        REASON_BINDCAR_UNREACHABLE,
        "Cannot connect to Bindcar API".into(),
    )
}

/// Check if HTTP status code indicates success (2xx).
///
/// # Arguments
///
/// * `status_code` - HTTP status code
///
/// # Returns
///
/// `true` if status code is in the 2xx range, `false` otherwise
///
/// # Example
///
/// ```rust
/// use bindy::http_errors::is_success_status;
///
/// assert!(is_success_status(200));
/// assert!(is_success_status(201));
/// assert!(is_success_status(204));
/// assert!(!is_success_status(404));
/// assert!(!is_success_status(500));
/// ```
#[must_use]
pub const fn is_success_status(status_code: u16) -> bool {
    status_code >= 200 && status_code < 300
}

/// Get condition reason for successful operations.
///
/// Use this when an operation completes successfully and you need to set
/// a condition to reflect success.
///
/// # Returns
///
/// A tuple of `(reason, message)` for successful operations
///
/// # Example
///
/// ```rust
/// use bindy::http_errors::success_reason;
///
/// let (reason, message) = success_reason();
/// // Set condition: Pod-0 status=True reason=Ready message="Pod is ready"
/// ```
#[must_use]
pub fn success_reason() -> (&'static str, &'static str) {
    (REASON_READY, "Operation completed successfully")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_http_400() {
        let (reason, message) = map_http_error_to_reason(400);
        assert_eq!(reason, REASON_BINDCAR_BAD_REQUEST);
        assert!(message.contains("400"));
        assert!(message.contains("Invalid"));
    }

    #[test]
    fn test_map_http_401() {
        let (reason, message) = map_http_error_to_reason(401);
        assert_eq!(reason, REASON_BINDCAR_AUTH_FAILED);
        assert!(message.contains("401"));
    }

    #[test]
    fn test_map_http_403() {
        let (reason, message) = map_http_error_to_reason(403);
        assert_eq!(reason, REASON_BINDCAR_AUTH_FAILED);
        assert!(message.contains("403"));
    }

    #[test]
    fn test_map_http_404() {
        let (reason, message) = map_http_error_to_reason(404);
        assert_eq!(reason, REASON_ZONE_NOT_FOUND);
        assert!(message.contains("404"));
        assert!(message.contains("not found"));
    }

    #[test]
    fn test_map_http_500() {
        let (reason, message) = map_http_error_to_reason(500);
        assert_eq!(reason, REASON_BINDCAR_INTERNAL_ERROR);
        assert!(message.contains("500"));
    }

    #[test]
    fn test_map_http_501() {
        let (reason, message) = map_http_error_to_reason(501);
        assert_eq!(reason, REASON_BINDCAR_NOT_IMPLEMENTED);
        assert!(message.contains("501"));
    }

    #[test]
    fn test_map_http_502() {
        let (reason, message) = map_http_error_to_reason(502);
        assert_eq!(reason, REASON_GATEWAY_ERROR);
        assert!(message.contains("502"));
    }

    #[test]
    fn test_map_http_503() {
        let (reason, message) = map_http_error_to_reason(503);
        assert_eq!(reason, REASON_GATEWAY_ERROR);
        assert!(message.contains("503"));
    }

    #[test]
    fn test_map_http_504() {
        let (reason, message) = map_http_error_to_reason(504);
        assert_eq!(reason, REASON_GATEWAY_ERROR);
        assert!(message.contains("504"));
    }

    #[test]
    fn test_map_http_unknown() {
        let (reason, message) = map_http_error_to_reason(418); // I'm a teapot
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("418"));
    }

    #[test]
    fn test_map_connection_error() {
        let (reason, message) = map_connection_error();
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("connect"));
    }

    #[test]
    fn test_is_success_status() {
        // Success codes
        assert!(is_success_status(200));
        assert!(is_success_status(201));
        assert!(is_success_status(204));
        assert!(is_success_status(299));

        // Non-success codes
        assert!(!is_success_status(199));
        assert!(!is_success_status(300));
        assert!(!is_success_status(400));
        assert!(!is_success_status(404));
        assert!(!is_success_status(500));
    }

    #[test]
    fn test_success_reason() {
        let (reason, message) = success_reason();
        assert_eq!(reason, REASON_READY);
        assert!(message.contains("success"));
    }
}
