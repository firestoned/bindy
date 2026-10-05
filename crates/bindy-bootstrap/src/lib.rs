// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! `bindy bootstrap` (ADR-0009, roadmap 01 Phase E): installs the operator
//! (namespace, CRDs, RBAC, Deployment), Scout, and multi-cluster access.
//!
//! The Scout RBAC built here is mirrored by static manifests in
//! `deploy/scout/`, `deploy/scout.yaml` and `docs/src/guide/scout.md`;
//! `bootstrap_tests` fails when they drift.

// The API modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{constants, crd};

mod bootstrap;

pub use bootstrap::*;

#[cfg(test)]
mod bootstrap_tests;
