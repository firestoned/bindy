// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the generation helpers in `status.rs` (`should_reconcile`).

#[cfg(test)]
mod tests {
    use crate::status::should_reconcile;

    // ========== Tests for should_reconcile() ==========

    #[test]
    fn test_should_reconcile_when_generations_equal() {
        // Arrange: Generations match - no changes
        let current = Some(5);
        let observed = Some(5);

        // Act
        let result = should_reconcile(current, observed);

        // Assert: No reconciliation needed
        assert!(!result, "Should not reconcile when generations match");
    }

    #[test]
    fn test_should_reconcile_when_generations_differ() {
        // Arrange: Current generation is ahead - spec changed
        let current = Some(7);
        let observed = Some(5);

        // Act
        let result = should_reconcile(current, observed);

        // Assert: Reconciliation needed
        assert!(result, "Should reconcile when current > observed");
    }

    #[test]
    fn test_should_reconcile_first_reconciliation() {
        // Arrange: Observed is None - first reconciliation
        let current = Some(1);
        let observed = None;

        // Act
        let result = should_reconcile(current, observed);

        // Assert: Reconciliation needed
        assert!(
            result,
            "Should reconcile on first reconciliation (observed=None)"
        );
    }

    #[test]
    fn test_should_reconcile_no_generation_tracking() {
        // Arrange: Current is None - no generation tracking
        let current = None;
        let observed = Some(5);

        // Act
        let result = should_reconcile(current, observed);

        // Assert: No reconciliation (can't determine if changed)
        assert!(
            !result,
            "Should not reconcile when current generation is None"
        );
    }

    #[test]
    fn test_should_reconcile_both_none() {
        // Arrange: Both None - no generation tracking at all
        let current = None;
        let observed = None;

        // Act
        let result = should_reconcile(current, observed);

        // Assert: No reconciliation
        assert!(
            !result,
            "Should not reconcile when both generations are None"
        );
    }

    #[test]
    fn test_should_reconcile_generation_decreased() {
        // Arrange: Edge case - current < observed (shouldn't happen normally)
        let current = Some(3);
        let observed = Some(5);

        // Act
        let result = should_reconcile(current, observed);

        // Assert: Still reconciles because they differ
        assert!(
            result,
            "Should reconcile when generations differ (even if current < observed)"
        );
    }
}
