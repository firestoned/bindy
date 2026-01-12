// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Constants used in DNS zone reconciliation.

/// Port name for the Bindcar HTTP API
pub const BINDCAR_API_PORT_NAME: &str = "http";

/// Port name for DNS over TCP
pub const DNS_TCP_PORT_NAME: &str = "dns-tcp";

/// Port name for RNDC API
pub const RNDC_API_PORT_NAME: &str = "rndc-api";

/// Pod phase indicating the pod is running
pub const POD_PHASE_RUNNING: &str = "Running";

/// API version for Bindy resources
pub const BINDY_API_VERSION: &str = "bindy.firestoned.io/v1beta1";

/// Bind9Instance resource kind
pub const BIND9_INSTANCE_KIND: &str = "Bind9Instance";
