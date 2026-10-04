// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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

    #[test]
    fn test_reason_progressing_constant() {
        assert_eq!(REASON_PROGRESSING, "Progressing");
    }

    // ============================================================================
    // Test Error-Related Reason Constants
    // ============================================================================

    #[test]
    fn test_reason_rndc_authentication_failed_constant() {
        assert_eq!(
            REASON_RNDC_AUTHENTICATION_FAILED,
            "RNDCAuthenticationFailed"
        );
    }

    #[test]
    fn test_reason_bindcar_unreachable_constant() {
        assert_eq!(REASON_BINDCAR_UNREACHABLE, "BindcarUnreachable");
    }

    #[test]
    fn test_reason_zone_transfer_complete_constant() {
        assert_eq!(REASON_ZONE_TRANSFER_COMPLETE, "ZoneTransferComplete");
    }

    #[test]
    fn test_reason_zone_transfer_failed_constant() {
        assert_eq!(REASON_ZONE_TRANSFER_FAILED, "ZoneTransferFailed");
    }

    #[test]
    fn test_reason_pods_pending_constant() {
        assert_eq!(REASON_PODS_PENDING, "PodsPending");
    }

    #[test]
    fn test_reason_pods_crashing_constant() {
        assert_eq!(REASON_PODS_CRASHING, "PodsCrashing");
    }

    // ============================================================================
    // Test HTTP Error Mapping Reason Constants
    // ============================================================================

    #[test]
    fn test_reason_bindcar_bad_request_constant() {
        assert_eq!(REASON_BINDCAR_BAD_REQUEST, "BindcarBadRequest");
    }

    #[test]
    fn test_reason_bindcar_auth_failed_constant() {
        assert_eq!(REASON_BINDCAR_AUTH_FAILED, "BindcarAuthFailed");
    }

    #[test]
    fn test_reason_zone_not_found_constant() {
        assert_eq!(REASON_ZONE_NOT_FOUND, "ZoneNotFound");
    }

    #[test]
    fn test_reason_bindcar_internal_error_constant() {
        assert_eq!(REASON_BINDCAR_INTERNAL_ERROR, "BindcarInternalError");
    }

    #[test]
    fn test_reason_bindcar_not_implemented_constant() {
        assert_eq!(REASON_BINDCAR_NOT_IMPLEMENTED, "BindcarNotImplemented");
    }

    #[test]
    fn test_reason_gateway_error_constant() {
        assert_eq!(REASON_GATEWAY_ERROR, "GatewayError");
    }

    // ============================================================================
    // Test Bind9Cluster Specific Reason Constants
    // ============================================================================

    #[test]
    fn test_reason_instances_created_constant() {
        assert_eq!(REASON_INSTANCES_CREATED, "InstancesCreated");
    }

    #[test]
    fn test_reason_instances_scaling_constant() {
        assert_eq!(REASON_INSTANCES_SCALING, "InstancesScaling");
    }

    #[test]
    fn test_reason_instances_pending_constant() {
        assert_eq!(REASON_INSTANCES_PENDING, "InstancesPending");
    }

    // ============================================================================
    // Test ClusterBind9Provider Specific Reason Constants
    // ============================================================================

    #[test]
    fn test_reason_clusters_ready_constant() {
        assert_eq!(REASON_CLUSTERS_READY, "ClustersReady");
    }

    #[test]
    fn test_reason_clusters_progressing_constant() {
        assert_eq!(REASON_CLUSTERS_PROGRESSING, "ClustersProgressing");
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
            REASON_PROGRESSING,
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
        let reasons = [
            REASON_BINDCAR_BAD_REQUEST,
            REASON_BINDCAR_AUTH_FAILED,
            REASON_ZONE_NOT_FOUND,
            REASON_BINDCAR_INTERNAL_ERROR,
            REASON_BINDCAR_NOT_IMPLEMENTED,
            REASON_GATEWAY_ERROR,
            REASON_BINDCAR_UNREACHABLE,
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
            REASON_PROGRESSING,
            REASON_BINDCAR_BAD_REQUEST,
            REASON_BINDCAR_AUTH_FAILED,
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
}
