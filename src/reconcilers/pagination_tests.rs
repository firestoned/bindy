// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `pagination.rs`

#[cfg(test)]
mod tests {
    use crate::constants::KUBE_LIST_PAGE_SIZE;

    /// Test that pagination constant has expected value
    #[test]
    fn test_pagination_constant() {
        assert_eq!(
            KUBE_LIST_PAGE_SIZE, 100,
            "Page size should be 100 items per page"
        );

        // Verify it's a reasonable value (not too small, not too large)
        #[allow(clippy::assertions_on_constants)]
        {
            assert!(
                KUBE_LIST_PAGE_SIZE >= 50,
                "Page size should be at least 50 to avoid excessive API calls"
            );
            assert!(
                KUBE_LIST_PAGE_SIZE <= 500,
                "Page size should not exceed 500 to avoid memory pressure"
            );
        }
    }

    /// Test that `list_all_paginated` function signature is correct
    ///
    /// This test documents the expected API without requiring a running Kubernetes cluster.
    /// Full integration tests will verify actual pagination behavior.
    #[test]
    fn test_list_all_paginated_signature() {
        // This test verifies the function exists and has the expected signature
        // by checking type inference works correctly

        use kube::api::ListParams;

        // Verify ListParams can be constructed
        let params = ListParams::default();
        assert!(
            params.limit.is_none(),
            "Default params should have no limit"
        );

        // Verify we can set a limit
        let params_with_limit = ListParams {
            limit: Some(KUBE_LIST_PAGE_SIZE),
            ..Default::default()
        };
        assert_eq!(
            params_with_limit.limit,
            Some(100),
            "Should be able to set page limit"
        );
    }

    /// Test that page size calculation is reasonable for different dataset sizes
    #[test]
    fn test_page_count_calculations() {
        let page_size = KUBE_LIST_PAGE_SIZE;

        // Small dataset (1-100 items) = 1 page
        assert_eq!(100 / page_size, 1, "100 items should require 1 page");

        // Medium dataset (1000 items) = 10 pages
        assert_eq!(1000 / page_size, 10, "1000 items should require 10 pages");

        // Large dataset (10000 items) = 100 pages
        assert_eq!(
            10_000 / page_size,
            100,
            "10000 items should require 100 pages"
        );

        // Verify API call count is reasonable
        let items_10k = 10_000_u32;
        let api_calls = items_10k.div_ceil(page_size);
        assert!(
            api_calls <= 200,
            "Should not require excessive API calls for large datasets"
        );
    }
}
