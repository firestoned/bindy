// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#![allow(unexpected_cfgs)]

//! # Bindy - BIND9 DNS Controller for Kubernetes
//!
//! Bindy is a high-performance Kubernetes controller written in Rust that manages BIND9 DNS
//! infrastructure through Custom Resource Definitions (CRDs).
//!
//! ## Overview
//!
//! This library provides the core functionality for the Bindy DNS controller, including:
//!
//! - Custom Resource Definitions (CRDs) for DNS zones and records
//! - Reconciliation logic for managing BIND9 configurations
//! - Zone file generation and management
//! - Integration with Kubernetes API server
//!
//! ## Modules
//!
//! - [`crd`] - Custom Resource Definition types for DNS resources
//! - [`reconcilers`] - Reconciliation logic for each resource type
//! - [`bind9`] - BIND9 zone file generation and management
//! - [`bind9_resources`] - BIND9 instance resource management
//!
//! ## Example
//!
//! ```rust,no_run
//! use bindy::crd::{DNSZone, DNSZoneSpec, SOARecord};
//!
//! // Create a DNS zone specification
//! let soa = SOARecord {
//!     primary_ns: "ns1.example.com.".to_string(),
//!     admin_email: "admin@example.com".to_string(),
//!     serial: 2024010101,
//!     refresh: 3600,
//!     retry: 600,
//!     expire: 604800,
//!     negative_ttl: 86400,
//! };
//!
//! let zone_spec = DNSZoneSpec {
//!     zone_name: "example.com".to_string(),
//!     cluster_ref: Some("my-dns-cluster".to_string()),
//!     global_cluster_ref: None,
//!     soa_record: soa,
//!     ttl: Some(3600),
//!     name_server_ips: None,
//! };
//! ```
//!
//! ## Features
//!
//! - **High Performance** - Native Rust with async/await
//! - **Label Selectors** - Target specific BIND9 instances
//! - **Multi-Record Types** - A, AAAA, CNAME, MX, TXT, NS, SRV, CAA
//! - **Status Tracking** - Full status subresources
//!
//! For more information, see the [documentation](https://firestoned.github.io/bindy/).

pub mod bind9;
pub mod bind9_resources;
pub mod constants;
pub mod crd;
pub mod crd_docs;
pub mod labels;
pub mod metrics;
pub mod reconcilers;

#[cfg(test)]
mod bind9_resources_tests;
#[cfg(test)]
mod crd_docs_tests;
#[cfg(test)]
mod crd_tests;
