// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Helper functions for DNS zone reconciliation.
//!
//! This module contains the validation and change detection helper functions
//! extracted from the main reconcile_dnszone() function to improve maintainability.

// Moved to `bindy-bind9` (ADR-0009 §2, amended 2026-10-05); re-exported so
// the zone controller keeps its paths.
// Every RNDC key and endpoint read goes through an `InstanceResolver` now
// (ADR-0016), so the uncached `load_rndc_key` and `get_endpoint` are no longer
// re-exported here.
pub use bindy_bind9::instances::{
    for_each_instance_endpoint, for_each_instance_endpoint_with_policy, EndpointFailurePolicy,
    HTTP_STATUS_NOT_FOUND,
};
