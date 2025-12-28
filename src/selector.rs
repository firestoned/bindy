// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Label selector matching utilities for `DNSZone` → Record watching.
//!
//! This module provides functions to match Kubernetes resources against
//! label selectors, enabling the `DNSZone` controller to watch for records
//! that match its label selector criteria.
//!
//! # Architecture
//!
//! The label selector watch pattern uses kube-rs's reflector/store to maintain
//! an in-memory cache of all `DNSZone` resources. When a record changes, the watch mapper
//! synchronously queries this cache to find all zones that select the record.
//!
//! # Example
//!
//! ```rust,no_run
//! use bindy::selector::find_zones_for_record;
//! use bindy::crd::DNSZone;
//! use kube::runtime::reflector::Store;
//!
//! # async fn example(store: Store<DNSZone>, record: bindy::crd::ARecord) {
//! // Find all DNSZones that select this record
//! let matching_zones = find_zones_for_record(&store, &record);
//! # }
//! ```

use crate::crd::DNSZone;
use kube::runtime::reflector::{ObjectRef, Store};
use kube::{Resource, ResourceExt};

/// Find all `DNSZone` resources in the store that select this record via label selectors.
///
/// This function is called by the controller's watch mapper when a record
/// changes. It synchronously queries the in-memory `DNSZone` cache to find
/// all zones whose `recordsFrom.selector` matches the record's labels.
///
/// # Arguments
///
/// * `store` - In-memory cache of `DNSZone` resources maintained by the reflector
/// * `record` - The record resource to match against zone selectors
///
/// # Returns
///
/// A vector of `ObjectRef<DNSZone>` for all zones that selected this record.
/// The returned zones will be reconciled by the controller.
///
/// # Example
///
/// ```rust,no_run
/// use bindy::selector::find_zones_for_record;
/// use bindy::crd::{ARecord, DNSZone};
/// use kube::runtime::reflector::Store;
///
/// # async fn example(store: Store<DNSZone>, record: ARecord) {
/// let matching_zones = find_zones_for_record(&store, &record);
/// for zone_ref in matching_zones {
///     println!("Zone {} selected this record", zone_ref.name);
/// }
/// # }
/// ```
pub fn find_zones_for_record<K>(store: &Store<DNSZone>, record: &K) -> Vec<ObjectRef<DNSZone>>
where
    K: Resource<DynamicType = ()> + ResourceExt,
{
    let record_labels = record.labels();
    let record_namespace = record.namespace().unwrap_or_default();

    store
        .state()
        .iter()
        .filter(|zone| {
            // Only consider zones in the same namespace
            let zone_namespace = zone.namespace().unwrap_or_default();
            if zone_namespace != record_namespace {
                return false;
            }

            // Check if any recordsFrom selector matches this record
            if let Some(ref records_from) = zone.spec.records_from {
                records_from
                    .iter()
                    .any(|source| source.selector.matches(record_labels))
            } else {
                false
            }
        })
        .map(|zone| ObjectRef::from_obj(&**zone))
        .collect()
}

#[cfg(test)]
#[path = "selector_tests.rs"]
mod selector_tests;
