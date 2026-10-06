// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#![allow(unexpected_cfgs)]

//! # bindy-controller-sdk: the shared controller framework
//!
//! What every bindy controller needs and none of them owns (ADR-0009):
//! error handling and retry policy, status helpers, retry and
//! backoff, paginated LISTs, the rate-limited client, namespace scoping and
//! Prometheus metrics. It depends only on `bindy-api`; no controller crate
//! and no BIND9 code.
//!
//! ## Modules
//!
//! - [`context`] - The shared `Context`, `Stores` and the record-kind registry
//! - [`error`] - [`error::ReconcileError`], [`error::error_policy`] and the
//!   retry and convergence actions a reconcile returns (ADR-0016)
//! - [`retry`] - Kubernetes and HTTP retry, reconcile backoff
//! - [`status`] - Status condition helpers
//! - [`pagination`] - Paginated LIST helpers
//! - [`resources`] - Generic create-or-update helpers
//! - [`rate_limit`] - The client-side rate-limited Kubernetes client (ADR-0005)
//! - [`request_timeout`] - The deadline on non-watch Kubernetes API requests (ADR-0014)
//! - [`namespace_scope`] - Cluster-wide or per-namespace watch scope
//! - [`leader`] - Leader election over a Kubernetes `Lease`
//! - [`shutdown`] - The draining shutdown signal and the controller supervisor
//! - [`reconcile`] - Timing and metrics around one reconcile; success awaits
//!   the next change, a scheduled wake is capped (ADR-0016)
//! - [`finalizers`] - Adding, removing and honouring finalizers
//! - [`http_errors`] - HTTP error mapping to status reasons
//! - [`metrics`] - Prometheus metrics
//! - [`watch`] - The shared watch layer: one watch and cache per kind and namespace

pub mod context;
pub mod error;
pub mod finalizers;
pub mod http_errors;
pub mod leader;
pub mod metrics;
pub mod namespace_scope;
pub mod pagination;
pub mod rate_limit;
pub mod reconcile;
pub mod request_timeout;
pub mod resources;
pub mod retry;
pub mod shutdown;
pub mod status;
pub mod watch;

#[cfg(test)]
mod http_errors_tests;
#[cfg(test)]
mod status_tests;
