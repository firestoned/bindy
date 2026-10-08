// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! BIND9 domain logic for the bindy operator (ADR-0009, roadmap 01 Phase C).
//!
//! Everything here talks to BIND9 or describes how a BIND9 instance is built,
//! and none of it is a controller: there is no `kube::runtime` import in this
//! crate. Controller crates depend on it; it depends only on `bindy-api` and
//! `bindy-controller-sdk`.
//!
//! - [`bind9`] - the bindcar HTTP client, RNDC keys, zone and record operations
//! - [`bind9_resources`] - the Deployment, Service, ConfigMap and friends built
//!   for an instance
//! - [`bind9_acl`] - ACL rendering for `named.conf`
//! - [`config_check`] - refuses a rendered `named.conf` hornet cannot parse (ADR-0013)
//! - [`placement`] - pod placement resolved from instance, cluster and provider
//! - [`instances`] - which instances a zone targets, and their BIND9 endpoints
//! - [`primary`] - primary instances, their pods and endpoints
//! - [`record_push`] - writing, deleting and replaying records on BIND9 primaries
//! - [`ddns`] - record hashing and dynamic-update helpers
//! - [`dns_errors`] - typed errors for DNS operations
//! - [`safe_volume`] - validation of user-supplied volume and Secret references
//! - [`context`] - the shared [`context::Context`] plus [`context::StoresBind9Ext`],
//!   which builds a [`bind9::Bind9Manager`] for an instance

// The API modules keep their `crate::` paths inside this crate.
pub(crate) use bindy_api::{constants, crd, labels};

pub mod bind9;
pub mod bind9_acl;
pub mod bind9_resources;
pub mod config_check;
pub mod context;
pub mod ddns;
pub mod dns_errors;
pub mod instances;
pub mod peers;
pub mod placement;
pub mod primary;
pub mod record_push;
pub mod safe_volume;

#[cfg(test)]
mod bind9_acl_tests;
#[cfg(test)]
mod bind9_resources_tests;
#[cfg(test)]
mod dns_errors_tests;
