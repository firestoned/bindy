// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `cleanup.rs`
//!
//! These tests document expected behavior for cleanup operations.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
mod tests {
    use crate::crd::InstanceReference;

    #[tokio::test]
    async fn test_cleanup_deleted_instances_no_instances() {
        // This test requires mocking the Kubernetes API
        // For now, we document the expected behavior:
        //
        // Given: A zone with no instances in status
        // When: cleanup_deleted_instances is called
        // Then: Should return Ok(0) immediately
        //       AND log debug message "No instances in status - skipping cleanup"
    }

    #[test]
    fn test_instance_reference_equality() {
        let inst1 = InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: "instance-1".to_string(),
            namespace: "default".to_string(),
            last_reconciled_at: None,
        };

        let inst2 = InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: "instance-1".to_string(),
            namespace: "default".to_string(),
            last_reconciled_at: Some("2025-01-01T00:00:00Z".to_string()),
        };

        // InstanceReference equality should ignore last_reconciled_at
        assert_eq!(inst1, inst2);
    }

    // ========================================================================
    // 404-aware existence checks (existence_from_get_result)
    // ========================================================================

    const HTTP_NOT_FOUND: u16 = 404;

    fn kube_api_error(code: u16) -> kube::Error {
        kube::Error::Api(
            kube::core::Status::failure("test error", "TestReason")
                .with_code(code)
                .boxed(),
        )
    }

    #[test]
    fn test_existence_from_get_result_ok_means_exists() {
        let result: Result<i32, kube::Error> = Ok(42);
        let exists = super::super::existence_from_get_result(result).unwrap();
        assert!(exists);
    }

    #[test]
    fn test_existence_from_get_result_404_means_deleted() {
        let result: Result<i32, kube::Error> = Err(kube_api_error(HTTP_NOT_FOUND));
        let exists = super::super::existence_from_get_result(result).unwrap();
        assert!(!exists);
    }

    #[test]
    fn test_existence_from_get_result_transient_errors_propagate() {
        // CRITICAL: transient API errors must NOT be conflated with "deleted" -
        // doing so would trigger the self-healing path and delete live DNS data
        const HTTP_TOO_MANY_REQUESTS: u16 = 429;
        const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;
        const HTTP_GATEWAY_TIMEOUT: u16 = 504;
        for code in [
            HTTP_TOO_MANY_REQUESTS,
            HTTP_INTERNAL_SERVER_ERROR,
            HTTP_GATEWAY_TIMEOUT,
        ] {
            let result: Result<i32, kube::Error> = Err(kube_api_error(code));
            assert!(
                super::super::existence_from_get_result(result).is_err(),
                "HTTP {code} must propagate as an error, not report deletion"
            );
        }
    }

    #[test]
    fn test_existence_from_get_result_non_api_error_propagates() {
        // kube errors that are not Api responses (e.g. connection failures)
        // must also propagate
        let result: Result<i32, kube::Error> = Err(kube::Error::LinesCodecMaxLineLengthExceeded);
        assert!(super::super::existence_from_get_result(result).is_err());
    }
}
