// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Scout (ADR-0009, roadmap 01 Phase E): watches Ingresses, LoadBalancer
//! Services and Gateway API routes on a workload cluster and writes the
//! matching `ARecord`s to the bindy cluster. `bindy scout` runs
//! [`run_scout`].
//!
//! Scout's watches go through the shared `WatchSet` from
//! `bindy-controller-sdk`: one for the local cluster's sources, one for the
//! bindy cluster's `DNSZone`s.

// The API modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{constants, crd};

mod scout;

pub use scout::*;

#[cfg(test)]
mod scout_tests;
