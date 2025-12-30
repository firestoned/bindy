// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Module-level documentation for CRD types
//!
//! This file contains additional doc examples that will be included in the API reference.

/// # Examples
///
/// ## Creating a DNS Zone
///
/// ```rust,no_run
/// use bindy::crd::{DNSZone, DNSZoneSpec, SOARecord};
///
/// let soa = SOARecord {
///     primary_ns: "ns1.example.com.".to_string(),
///     admin_email: "admin@example.com".to_string(),
///     serial: 2024010101,
///     refresh: 3600,
///     retry: 600,
///     expire: 604800,
///     negative_ttl: 86400,
/// };
///
/// let spec = DNSZoneSpec {
///     zone_name: "example.com".to_string(),
///     cluster_ref: None,
///     soa_record: soa,
///     ttl: Some(3600),
///     name_server_ips: None,
///     records_from: None,
///     bind9_instances_from: None,
/// };
/// ```
///
/// ## Creating DNS Records
///
/// ```rust,no_run
/// use bindy::crd::{ARecordSpec, MXRecordSpec, TXTRecordSpec};
///
/// // A Record
/// let a_record = ARecordSpec {
///     name: "www".to_string(),
///     ipv4_address: "192.0.2.1".to_string(),
///     ttl: Some(300),
/// };
///
/// // MX Record
/// let mx_record = MXRecordSpec {
///     name: "@".to_string(),
///     priority: 10,
///     mail_server: "mail.example.com.".to_string(),
///     ttl: Some(3600),
/// };
///
/// // TXT Record (SPF)
/// let txt_record = TXTRecordSpec {
///     name: "@".to_string(),
///     text: vec!["v=spf1 include:_spf.example.com ~all".to_string()],
///     ttl: Some(3600),
/// };
/// ```
pub struct CRDExamples;
