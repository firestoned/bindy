// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Classification of Kubernetes API errors by HTTP status code.
//!
//! Every helper here goes by the `code` of a `kube::Error::Api` status, never
//! by the text of the error: matching on `"NotFound"` or `"AlreadyExists"` in
//! a formatted message also matches a 500 whose message happens to say so.
//!
//! The `_anyhow` variants walk the whole `anyhow` chain, so a kube error is
//! found through `.context(...)` layers, through another error that names it
//! as its source, and through `retry_api_call`, which passes non-retryable
//! errors (404, 409) through unchanged.
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy_controller_sdk::kube_errors::is_not_found;
//! use k8s_openapi::api::core::v1::ConfigMap;
//! use kube::Api;
//!
//! async fn exists(api: &Api<ConfigMap>, name: &str) -> Result<bool, kube::Error> {
//!     match api.get(name).await {
//!         Ok(_) => Ok(true),
//!         Err(e) if is_not_found(&e) => Ok(false),
//!         Err(e) => Err(e),
//!     }
//! }
//! ```

use bindy_api::constants::{
    HTTP_CONFLICT, HTTP_FORBIDDEN, HTTP_NOT_FOUND, HTTP_UNPROCESSABLE_ENTITY,
};

/// The HTTP status code of a Kubernetes API error, if `e` is one.
fn api_code(e: &kube::Error) -> Option<u16> {
    match e {
        kube::Error::Api(status) => Some(status.code),
        _ => None,
    }
}

/// Whether some error in `e`'s chain is a Kubernetes API error with `code`.
fn anyhow_has_code(e: &anyhow::Error, code: u16) -> bool {
    e.chain()
        .filter_map(|cause| cause.downcast_ref::<kube::Error>())
        .any(|kube_error| api_code(kube_error) == Some(code))
}

/// Whether `e` is the API server reporting the object as absent (HTTP 404).
///
/// # Arguments
/// * `e` - The error from a `kube::Api` call
///
/// # Returns
/// `true` only for a `kube::Error::Api` with code 404. Transport, decoding
/// and every other API status return `false`.
#[must_use]
pub fn is_not_found(e: &kube::Error) -> bool {
    api_code(e) == Some(HTTP_NOT_FOUND)
}

/// Whether `e` is the API server reporting a conflict (HTTP 409).
///
/// Covers both `AlreadyExists` (a create of an object that exists) and
/// `Conflict` (a write against a stale `resourceVersion`), which share the
/// code.
///
/// # Arguments
/// * `e` - The error from a `kube::Api` call
///
/// # Returns
/// `true` only for a `kube::Error::Api` with code 409.
#[must_use]
pub fn is_conflict(e: &kube::Error) -> bool {
    api_code(e) == Some(HTTP_CONFLICT)
}

/// Whether `e` is the API server refusing the request rather than failing
/// it: HTTP 403 (RBAC, or an admission policy denying with reason
/// `Forbidden`) or 422 (an admission policy denying with its default reason
/// `Invalid`, or a schema rejection). Retrying a refused request unchanged
/// cannot succeed.
///
/// # Arguments
/// * `e` - The error from a `kube::Api` call
///
/// # Returns
/// `true` only for a `kube::Error::Api` with code 403 or 422.
#[must_use]
pub fn is_refused(e: &kube::Error) -> bool {
    matches!(
        api_code(e),
        Some(HTTP_FORBIDDEN | HTTP_UNPROCESSABLE_ENTITY)
    )
}

/// [`is_not_found`] for an `anyhow::Error`: whether any error in its chain is
/// a Kubernetes API 404.
///
/// # Arguments
/// * `e` - The error, possibly wrapped in context
///
/// # Returns
/// `true` when a `kube::Error::Api` with code 404 is anywhere in the chain.
/// An error that only formats as "NotFound" (an `anyhow!` string) is not one.
#[must_use]
pub fn is_not_found_anyhow(e: &anyhow::Error) -> bool {
    anyhow_has_code(e, HTTP_NOT_FOUND)
}

/// [`is_conflict`] for an `anyhow::Error`: whether any error in its chain is
/// a Kubernetes API 409.
///
/// # Arguments
/// * `e` - The error, possibly wrapped in context
///
/// # Returns
/// `true` when a `kube::Error::Api` with code 409 is anywhere in the chain.
#[must_use]
pub fn is_conflict_anyhow(e: &anyhow::Error) -> bool {
    anyhow_has_code(e, HTTP_CONFLICT)
}

#[cfg(test)]
#[path = "kube_errors_tests.rs"]
mod kube_errors_tests;
