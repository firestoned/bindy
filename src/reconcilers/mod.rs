// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Kubernetes reconciliation controllers for DNS resources.
//!
//! This module contains the reconciliation logic for all Bindy Custom Resources.
//! Each reconciler watches for changes to its respective resource type and updates
//! BIND9 configurations accordingly.
//!
//! # Reconciliation Architecture
//!
//! Bindy follows the standard Kubernetes controller pattern:
//!
//! 1. **Watch** - Monitor resource changes via Kubernetes API
//! 2. **Reconcile** - Compare desired state (CRD spec) with actual state
//! 3. **Update** - Modify BIND9 configuration to match desired state
//! 4. **Status** - Report reconciliation results back to Kubernetes
//!
//! # Available Reconcilers
//!
//! ## Infrastructure
//!
//! - [`reconcile_bind9instance`] - Creates/updates BIND9 server deployments
//! - [`delete_bind9instance`] - Cleans up BIND9 server resources
//!
//! ## DNS Zones
//!
//! - [`reconcile_dnszone`] - Creates/updates DNS zone files
//! - [`delete_dnszone`] - Removes DNS zones
//!
//! ## DNS Records
//!
//! - [`reconcile_a_record`] - Manages IPv4 address records
//! - [`reconcile_aaaa_record`] - Manages IPv6 address records
//! - [`reconcile_cname_record`] - Manages canonical name aliases
//! - [`reconcile_mx_record`] - Manages mail exchange records
//! - [`reconcile_txt_record`] - Manages text records
//! - [`reconcile_ns_record`] - Manages nameserver delegation
//! - [`reconcile_srv_record`] - Manages service location records
//! - [`reconcile_caa_record`] - Manages certificate authority authorization
//!
//! # Example: Using a Reconciler
//!
//! ```rust,no_run
//! use bindy::reconcilers::reconcile_dnszone;
//! use bindy::crd::DNSZone;
//! use bindy::bind9::Bind9Manager;
//! use kube::Client;
//!
//! async fn reconcile_zone(dnszone: DNSZone) -> anyhow::Result<()> {
//!     let client = Client::try_default().await?;
//!     let zone_manager = Bind9Manager::new("/etc/bind/zones".to_string());
//!
//!     reconcile_dnszone(client, dnszone, &zone_manager).await?;
//!     Ok(())
//! }
//! ```

pub mod bind9instance;
pub mod dnszone;
pub mod records;

#[cfg(test)]
mod bind9instance_tests;
#[cfg(test)]
mod dnszone_tests;
#[cfg(test)]
mod records_tests;

pub use bind9instance::{delete_bind9instance, reconcile_bind9instance};
pub use dnszone::{delete_dnszone, reconcile_dnszone};
pub use records::{
    reconcile_a_record, reconcile_aaaa_record, reconcile_caa_record, reconcile_cname_record,
    reconcile_mx_record, reconcile_ns_record, reconcile_srv_record, reconcile_txt_record,
};
