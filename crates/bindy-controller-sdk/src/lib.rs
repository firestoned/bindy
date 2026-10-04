// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#![allow(unexpected_cfgs)]

//! # bindy-controller-sdk: the shared controller framework
//!
//! What every bindy controller needs and none of them owns (ADR-0009):
//! error handling and requeue policy, status helpers, retry and
//! backoff, paginated LISTs, the rate-limited client, namespace scoping and
//! Prometheus metrics. It depends only on `bindy-api`; no controller crate
//! and no BIND9 code.
//!
//! ## Modules
//!
//! - [`error`] - [`error::ReconcileError`] and [`error::error_policy`]
//! - [`requeue`] - Requeue intervals for ready and not-ready resources
//! - [`retry`] - Kubernetes and HTTP retry, reconcile backoff
//! - [`status`] - Status condition helpers
//! - [`pagination`] - Paginated LIST helpers
//! - [`resources`] - Generic create-or-update helpers
//! - [`rate_limit`] - The client-side rate-limited Kubernetes client (ADR-0005)
//! - [`namespace_scope`] - Cluster-wide or per-namespace watch scope
//! - [`http_errors`] - HTTP error mapping to status reasons
//! - [`metrics`] - Prometheus metrics
//! - [`watch`] - The shared watch layer: one watch and cache per kind and namespace

pub mod error;
pub mod http_errors;
pub mod metrics;
pub mod namespace_scope;
pub mod pagination;
pub mod rate_limit;
pub mod requeue;
pub mod resources;
pub mod retry;
pub mod status;
pub mod watch;

#[cfg(test)]
mod http_errors_tests;
#[cfg(test)]
mod status_tests;
