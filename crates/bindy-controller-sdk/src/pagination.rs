// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Pagination helpers for Kubernetes API list operations.
//!
//! This module provides utilities for efficiently listing large resource sets
//! by fetching them in pages, reducing memory usage and API server load.

use anyhow::{bail, Result};
use bindy_api::constants::KUBE_LIST_PAGE_SIZE;
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
/// use bindy_api::crd::DNSZone;
/// use bindy_controller_sdk::pagination::list_all_paginated;
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
/// Returns an error if Kubernetes API operations fail, or if the pages are
/// anomalous (a repeated continue token, an empty page that still has a
/// continue token, or more than [`MAX_REASONABLE_PAGES`] pages). Callers act
/// on the full list, so a partial one is never returned.
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

        // Decide before keeping the page: an anomalous page fails the whole
        // list, the caller never sees a truncated one.
        let step = next_page_step(
            page_count,
            item_count,
            last_continue_token.as_deref(),
            result.metadata.continue_,
        )
        .inspect_err(|e| {
            error!(
                page = page_count,
                items_in_page = item_count,
                total_items = all_items.len(),
                "Paginated list of {} aborted: {e:#}",
                K::kind(&())
            );
        })?;

        all_items.extend(result.items);

        debug!(
            page = page_count,
            items_in_page = item_count,
            total_items = all_items.len(),
            ?step,
            "Fetched page from Kubernetes API"
        );

        let PageStep::Next(continue_token) = step else {
            debug!(
                page = page_count,
                total_items = all_items.len(),
                "No continue token - pagination complete"
            );
            break;
        };
        last_continue_token = Some(continue_token.clone());
        list_params.continue_token = Some(continue_token);
    }

    debug!(
        total_pages = page_count,
        total_items = all_items.len(),
        "Completed paginated list operation"
    );

    crate::metrics::record_kube_api_pagination_pages(K::kind(&()).as_ref(), page_count);

    Ok(all_items)
}

/// What [`list_all_paginated`] does after fetching a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageStep {
    /// The page was the last one: the list is complete.
    Done,
    /// Fetch the next page with this continue token.
    Next(String),
}

/// Decide what follows a fetched page, failing on every pagination anomaly.
///
/// An empty continue token is treated as none: the Kubernetes API sometimes
/// returns `Some("")` instead of `None` for the last page.
///
/// # Arguments
///
/// * `page_count` - 1-based number of the page just fetched
/// * `items_in_page` - Number of items in that page
/// * `last_token` - The continue token the page was fetched with, if any
/// * `continue_token` - The continue token the page returned, if any
///
/// # Errors
///
/// Returns an error, so the caller never acts on a partial list, when the
/// page returned the token it was fetched with (an infinite loop), when an
/// empty page still carries a continue token (an API bug), or when more pages
/// remain after [`MAX_REASONABLE_PAGES`] pages.
pub(crate) fn next_page_step(
    page_count: usize,
    items_in_page: usize,
    last_token: Option<&str>,
    continue_token: Option<String>,
) -> Result<PageStep> {
    let Some(token) = continue_token.filter(|token| !token.is_empty()) else {
        return Ok(PageStep::Done);
    };

    if last_token == Some(token.as_str()) {
        bail!("pagination infinite loop: page {page_count} returned the same continue token twice");
    }

    if items_in_page == 0 {
        bail!("pagination API bug: page {page_count} returned 0 items but a continue token");
    }

    if page_count >= MAX_REASONABLE_PAGES {
        bail!(
            "pagination safety limit: more than {MAX_REASONABLE_PAGES} pages, refusing a partial list"
        );
    }

    Ok(PageStep::Next(token))
}

#[cfg(test)]
#[path = "pagination_tests.rs"]
mod pagination_tests;
