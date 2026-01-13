// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Pagination helpers for Kubernetes API list operations.
//!
//! This module provides utilities for efficiently listing large resource sets
//! by fetching them in pages, reducing memory usage and API server load.

use crate::constants::KUBE_LIST_PAGE_SIZE;
use anyhow::Result;
use kube::{api::ListParams, Api, Resource};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use tracing::debug;

/// List all resources with automatic pagination.
///
/// Fetches resources in pages to reduce memory usage and API server load.
/// This is especially important when listing hundreds or thousands of resources
/// (e.g., 1000+ `DNSZone`s per namespace).
///
/// # Arguments
///
/// * `api` - Kubernetes API client for the resource type
/// * `list_params` - Base list parameters (labels, fields, etc.)
///
/// # Returns
///
/// Vector of all resources, fetched in pages
///
/// # Example
///
/// ```no_run
/// use kube::{Api, Client, api::ListParams};
/// use bindy::crd::DNSZone;
/// use bindy::reconcilers::pagination::list_all_paginated;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = Client::try_default().await?;
/// let api: Api<DNSZone> = Api::namespaced(client, "default");
///
/// let zones = list_all_paginated(&api, ListParams::default()).await?;
/// println!("Found {} zones", zones.len());
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns an error if Kubernetes API operations fail.
pub async fn list_all_paginated<K>(api: &Api<K>, mut list_params: ListParams) -> Result<Vec<K>>
where
    K: Resource<DynamicType = ()> + Clone + DeserializeOwned + Debug,
    K::DynamicType: Default,
{
    // Configure pagination
    list_params.limit = Some(KUBE_LIST_PAGE_SIZE);

    let mut all_items = Vec::new();
    let mut page_count = 0;

    loop {
        page_count += 1;
        let result = api.list(&list_params).await?;

        let item_count = result.items.len();
        all_items.extend(result.items);

        debug!(
            page = page_count,
            items_in_page = item_count,
            total_items = all_items.len(),
            "Fetched page from Kubernetes API"
        );

        // Check if there are more pages
        if let Some(continue_token) = result.metadata.continue_ {
            list_params.continue_token = Some(continue_token);
        } else {
            break;
        }
    }

    debug!(
        total_pages = page_count,
        total_items = all_items.len(),
        "Completed paginated list operation"
    );

    Ok(all_items)
}

#[cfg(test)]
#[path = "pagination_tests.rs"]
mod pagination_tests;
