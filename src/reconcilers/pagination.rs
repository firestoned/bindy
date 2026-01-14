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
use tracing::{debug, error};

/// Maximum number of pages to fetch before aborting pagination.
///
/// This safety limit prevents infinite loops in case of Kubernetes API bugs
/// where the continue token never becomes None or repeats indefinitely.
/// With 100 items per page, 10,000 pages = 1,000,000 resources maximum.
const MAX_REASONABLE_PAGES: usize = 10_000;

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
    let mut last_continue_token: Option<String> = None;

    loop {
        page_count += 1;

        // Log current pagination state for debugging
        debug!(
            page = page_count,
            continue_token = ?list_params.continue_token,
            limit = ?list_params.limit,
            "About to fetch page from Kubernetes API"
        );

        let result = api.list(&list_params).await?;

        let item_count = result.items.len();

        // CRITICAL: Treat empty string continue tokens as None
        // The Kubernetes API sometimes returns Some("") instead of None for the last page
        let new_continue_token = result
            .metadata
            .continue_
            .clone()
            .filter(|token| !token.is_empty());

        // CRITICAL: Check if we're stuck in an infinite loop (same continue token repeated)
        if let Some(ref new_token) = new_continue_token {
            if last_continue_token.as_ref() == Some(new_token) {
                error!(
                    page = page_count,
                    continue_token = ?new_token,
                    items_in_page = item_count,
                    "PAGINATION INFINITE LOOP DETECTED: Same continue token returned twice! Breaking loop to prevent infinite paging."
                );
                break;
            }
        }

        // CRITICAL: Check for empty page with continue token (API bug)
        if item_count == 0 && new_continue_token.is_some() {
            error!(
                page = page_count,
                continue_token = ?new_continue_token,
                total_items = all_items.len(),
                "PAGINATION API BUG DETECTED: Received 0 items but got a continue token! Breaking loop to prevent infinite paging."
            );
            break;
        }

        all_items.extend(result.items);

        debug!(
            page = page_count,
            items_in_page = item_count,
            total_items = all_items.len(),
            continue_token = ?new_continue_token,
            "Fetched page from Kubernetes API"
        );

        // Check if there are more pages
        if let Some(continue_token) = new_continue_token.clone() {
            last_continue_token = Some(continue_token.clone());
            list_params.continue_token = Some(continue_token);
        } else {
            debug!(
                page = page_count,
                total_items = all_items.len(),
                "No continue token - pagination complete"
            );
            break;
        }

        // Safety check: Prevent infinite loops if we somehow exceed a reasonable page count
        if page_count >= MAX_REASONABLE_PAGES {
            error!(
                page = page_count,
                total_items = all_items.len(),
                "PAGINATION SAFETY LIMIT EXCEEDED: More than {} pages fetched! Breaking loop to prevent infinite paging. This indicates a serious bug.",
                MAX_REASONABLE_PAGES
            );
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
