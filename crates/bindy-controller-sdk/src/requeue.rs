// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! The requeue policy every controller shares.
//!
//! A reconcile that succeeds requeues after [`REQUEUE_WHEN_READY_SECS`] as a
//! drift backstop; one whose resource is not yet ready requeues sooner, after
//! [`REQUEUE_WHEN_NOT_READY_SECS`]. A reconcile that fails is requeued by
//! [`crate::error::error_policy`] with per-object exponential backoff instead.

/// Requeue interval for resources that are ready (5 minutes)
pub const REQUEUE_WHEN_READY_SECS: u64 = 300;

/// Requeue interval for resources that are not ready (30 seconds)
pub const REQUEUE_WHEN_NOT_READY_SECS: u64 = 30;
