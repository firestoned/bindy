// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `status_reasons` module
//!
//! These tests verify all status reason constants and helper functions.

#[cfg(test)]
mod tests {
    use crate::status_reasons::*;

    // ============================================================================
    // Test Common Reason Constants
    // ============================================================================

    #[test]
    fn test_reason_all_ready_constant() {
        assert_eq!(REASON_ALL_READY, "AllReady");
    }

    #[test]
    fn test_reason_ready_constant() {
        assert_eq!(REASON_READY, "Ready");
    }

    #[test]
    fn test_reason_partially_ready_constant() {
        assert_eq!(REASON_PARTIALLY_READY, "PartiallyReady");
    }

    #[test]
    fn test_reason_not_ready_constant() {
        assert_eq!(REASON_NOT_READY, "NotReady");
    }

    #[test]
    fn test_reason_no_children_constant() {
        assert_eq!(REASON_NO_CHILDREN, "NoChildren");
    }

    // ============================================================================
    // Test HTTP Error Mapping Reason Constants
    // ============================================================================

    #[test]
    fn test_reason_zone_not_found_constant() {
        assert_eq!(REASON_ZONE_NOT_FOUND, "ZoneNotFound");
    }

    // ============================================================================
    // Test Condition Type Constants
    // ============================================================================

    #[test]
    fn test_condition_type_ready_constant() {
        assert_eq!(CONDITION_TYPE_READY, "Ready");
    }

    #[test]
    fn test_condition_type_bind9_instance_prefix_constant() {
        assert_eq!(CONDITION_TYPE_BIND9_INSTANCE_PREFIX, "Bind9Instance");
    }

    #[test]
    fn test_condition_type_pod_prefix_constant() {
        assert_eq!(CONDITION_TYPE_POD_PREFIX, "Pod");
    }

    // ============================================================================
    // Test Helper Functions
    // ============================================================================

    #[test]
    fn test_bind9_instance_condition_type_zero() {
        assert_eq!(bind9_instance_condition_type(0), "Bind9Instance-0");
    }

    #[test]
    fn test_bind9_instance_condition_type_one() {
        assert_eq!(bind9_instance_condition_type(1), "Bind9Instance-1");
    }

    #[test]
    fn test_bind9_instance_condition_type_ten() {
        assert_eq!(bind9_instance_condition_type(10), "Bind9Instance-10");
    }

    #[test]
    fn test_bind9_instance_condition_type_large_index() {
        assert_eq!(bind9_instance_condition_type(999), "Bind9Instance-999");
    }

    #[test]
    fn test_pod_condition_type_zero() {
        assert_eq!(pod_condition_type(0), "Pod-0");
    }

    #[test]
    fn test_pod_condition_type_one() {
        assert_eq!(pod_condition_type(1), "Pod-1");
    }

    #[test]
    fn test_pod_condition_type_ten() {
        assert_eq!(pod_condition_type(10), "Pod-10");
    }

    #[test]
    fn test_pod_condition_type_large_index() {
        assert_eq!(pod_condition_type(999), "Pod-999");
    }

    // ============================================================================
    // Test Helper Function Consistency
    // ============================================================================

    #[test]
    fn test_helper_functions_use_correct_prefixes() {
        // Verify that helper functions use the prefix constants
        assert!(bind9_instance_condition_type(0).starts_with(CONDITION_TYPE_BIND9_INSTANCE_PREFIX));
        assert!(pod_condition_type(0).starts_with(CONDITION_TYPE_POD_PREFIX));
    }

    #[test]
    fn test_helper_functions_format_consistency() {
        // Verify format is {PREFIX}-{INDEX}
        for i in 0..5 {
            let instance_type = bind9_instance_condition_type(i);
            let pod_type = pod_condition_type(i);

            assert_eq!(
                instance_type,
                format!("{CONDITION_TYPE_BIND9_INSTANCE_PREFIX}-{i}")
            );
            assert_eq!(pod_type, format!("{CONDITION_TYPE_POD_PREFIX}-{i}"));
        }
    }

    // ============================================================================
    // Test Constant Value Uniqueness
    // ============================================================================

    #[test]
    fn test_all_ready_vs_ready_are_different() {
        // Critical distinction: these must be different values
        assert_ne!(REASON_ALL_READY, REASON_READY);
    }

    #[test]
    fn test_common_reasons_are_unique() {
        // Verify common reason constants have unique values
        let reasons = [
            REASON_ALL_READY,
            REASON_READY,
            REASON_PARTIALLY_READY,
            REASON_NOT_READY,
            REASON_NO_CHILDREN,
        ];

        for (i, reason1) in reasons.iter().enumerate() {
            for (j, reason2) in reasons.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        reason1, reason2,
                        "Constants at indices {i} and {j} have the same value: {reason1}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_http_error_reasons_are_unique() {
        // Verify HTTP error reason constants have unique values
        let reasons = [REASON_ZONE_NOT_FOUND];

        for (i, reason1) in reasons.iter().enumerate() {
            for (j, reason2) in reasons.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        reason1, reason2,
                        "Constants at indices {i} and {j} have the same value: {reason1}"
                    );
                }
            }
        }
    }

    // ============================================================================
    // Test Naming Conventions
    // ============================================================================

    #[test]
    fn test_reason_constants_follow_pascal_case() {
        // Verify all reason constants use PascalCase (no spaces, underscores in values)
        let reasons = [
            REASON_ALL_READY,
            REASON_READY,
            REASON_PARTIALLY_READY,
            REASON_NOT_READY,
            REASON_NO_CHILDREN,
        ];

        for reason in reasons {
            assert!(!reason.contains(' '), "Reason '{reason}' contains spaces");
            assert!(
                !reason.contains('_'),
                "Reason '{reason}' contains underscores"
            );
            // First character should be uppercase
            assert!(
                reason.chars().next().unwrap().is_uppercase(),
                "Reason '{reason}' doesn't start with uppercase"
            );
        }
    }

    #[test]
    fn test_condition_type_constants_follow_pascal_case() {
        let types = [
            CONDITION_TYPE_READY,
            CONDITION_TYPE_BIND9_INSTANCE_PREFIX,
            CONDITION_TYPE_POD_PREFIX,
        ];

        for type_name in types {
            assert!(
                !type_name.contains(' '),
                "Type '{type_name}' contains spaces"
            );
            assert!(
                !type_name.contains('_'),
                "Type '{type_name}' contains underscores"
            );
            assert!(
                type_name.chars().next().unwrap().is_uppercase(),
                "Type '{type_name}' doesn't start with uppercase"
            );
        }
    }

    #[test]
    fn rrset_conflict_reason_is_pinned() {
        // ADR-0027: the reason operators filter on for a held-back record.
        assert_eq!(REASON_RRSET_CONFLICT, "RRsetConflict");
    }
}
