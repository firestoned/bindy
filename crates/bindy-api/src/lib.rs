// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#![allow(unexpected_cfgs)]

//! # bindy-api: the Bindy API surface
//!
//! The leaf crate of the Bindy workspace (ADR-0009): the Custom Resource
//! Definition types and the constants, labels, selectors and status reasons
//! that describe them. It has no dependency on any other workspace crate, so
//! editing a CRD type rebuilds only the crates that use it.
//!
//! ## Modules
//!
//! - [`crd`] - Custom Resource Definition types for DNS resources
//! - [`crd_docs`] - Example manifests embedded in the generated API docs
//! - [`constants`] - Operator-wide constants
//! - [`labels`] - Label, annotation and finalizer keys
//! - [`selector`] - Label selector matching utilities
//! - [`status_reasons`] - Status condition reasons
//!
//! The `crdgen` and `crddoc` binaries, which regenerate
//! `deploy/operator/crds/` and the API reference, build only with the
//! `crdgen` feature.

pub mod constants;
pub mod crd;
pub mod crd_docs;
pub mod labels;
pub mod selector;
pub mod status_reasons;

#[cfg(test)]
mod crd_docs_tests;
#[cfg(test)]
mod crd_tests;
#[cfg(test)]
mod status_reasons_tests;
