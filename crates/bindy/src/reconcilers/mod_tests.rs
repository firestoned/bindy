// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for reconciler helper functions.

#[cfg(test)]
mod tests {
    use super::super::{should_reconcile, status_changed};

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

    // ========== Tests for status_changed() ==========

    #[test]
    fn test_status_changed_both_none() {
        // Arrange: Both None - no change
        let current: Option<i32> = None;
        let new: Option<i32> = None;

        // Act
        let changed = status_changed(&current, &new);

        // Assert: No change
        assert!(!changed, "Should return false when both are None");
    }

    #[test]
    fn test_status_changed_current_none_new_some() {
        // Arrange: Current None, new Some - status being set
        let current: Option<i32> = None;
        let new = Some(42);

        // Act
        let changed = status_changed(&current, &new);

        // Assert: Changed
        assert!(changed, "Should return true when status is being set");
    }

    #[test]
    fn test_status_changed_current_some_new_none() {
        // Arrange: Current Some, new None - status being cleared
        let current = Some(42);
        let new: Option<i32> = None;

        // Act
        let changed = status_changed(&current, &new);

        // Assert: Changed
        assert!(changed, "Should return true when status is being cleared");
    }

    #[test]
    fn test_status_changed_values_equal() {
        // Arrange: Both Some with equal values - no change
        let current = Some(42);
        let new = Some(42);

        // Act
        let changed = status_changed(&current, &new);

        // Assert: No change
        assert!(!changed, "Should return false when values are equal");
    }

    #[test]
    fn test_status_changed_values_differ() {
        // Arrange: Both Some with different values - changed
        let current = Some(42);
        let new = Some(99);

        // Act
        let changed = status_changed(&current, &new);

        // Assert: Changed
        assert!(changed, "Should return true when values differ");
    }

    #[test]
    fn test_status_changed_with_strings() {
        // Test with String type to verify generic works
        let current = Some("old-value".to_string());
        let new = Some("new-value".to_string());

        // Act
        let changed = status_changed(&current, &new);

        // Assert: Changed
        assert!(changed, "Should work with String type");
    }

    #[test]
    fn test_status_changed_with_custom_type() {
        // Test with custom type to verify PartialEq bound works
        #[derive(Debug, PartialEq)]
        struct CustomStatus {
            ready: bool,
            count: i32,
        }

        let current = Some(CustomStatus {
            ready: true,
            count: 3,
        });
        let new = Some(CustomStatus {
            ready: true,
            count: 5,
        });

        // Act
        let changed = status_changed(&current, &new);

        // Assert: Changed (count differs)
        assert!(changed, "Should work with custom types");
    }

    #[test]
    fn test_status_changed_prevents_update_loop() {
        // Simulate reconciliation loop prevention scenario
        //
        // Iteration 1: Status changes from None to Some(3)
        let current1: Option<i32> = None;
        let new1 = Some(3);
        assert!(
            status_changed(&current1, &new1),
            "Iteration 1: Should update status (None → Some(3))"
        );

        // Iteration 2: Status remains Some(3) after update
        let current2 = Some(3);
        let new2 = Some(3);
        assert!(
            !status_changed(&current2, &new2),
            "Iteration 2: Should NOT update status (Some(3) → Some(3))"
        );

        // Iteration 3: Status changes to Some(5)
        let current3 = Some(3);
        let new3 = Some(5);
        assert!(
            status_changed(&current3, &new3),
            "Iteration 3: Should update status (Some(3) → Some(5))"
        );

        // Iteration 4: Status remains Some(5)
        let current4 = Some(5);
        let new4 = Some(5);
        assert!(
            !status_changed(&current4, &new4),
            "Iteration 4: Should NOT update status (Some(5) → Some(5))"
        );
    }
}
