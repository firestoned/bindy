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
//! use bindy::crd::{DNSZone, DNSZoneSpec, SOARecord, LabelSelector};
//! use std::collections::BTreeMap;
//!
//! // Create a DNS zone specification for a primary zone
//! let mut match_labels = BTreeMap::new();
//! match_labels.insert("dns-role".to_string(), "primary".to_string());
//!
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
//!     zone_type: Some("primary".to_string()),
//!     instance_selector: LabelSelector {
//!         match_labels: Some(match_labels),
//!         match_expressions: None,
//!     },
//!     soa_record: Some(soa),
//!     secondary_config: None,
//!     ttl: Some(3600),
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
pub mod crd;
pub mod reconcilers;

#[cfg(test)]
mod crd_tests;
