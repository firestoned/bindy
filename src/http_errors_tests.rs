// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for http_errors module
//!
//! These tests verify HTTP error code mapping to status condition reasons.

#[cfg(test)]
mod tests {
    use crate::http_errors::*;
    use crate::status_reasons::*;

    // ============================================================================
    // Test HTTP 4xx Error Code Mappings
    // ============================================================================

    #[test]
    fn test_map_http_400_bad_request() {
        let (reason, message) = map_http_error_to_reason(400);
        assert_eq!(reason, REASON_BINDCAR_BAD_REQUEST);
        assert!(message.contains("400"));
        assert!(message.contains("Invalid request"));
    }

    #[test]
    fn test_map_http_401_unauthorized() {
        let (reason, message) = map_http_error_to_reason(401);
        assert_eq!(reason, REASON_BINDCAR_AUTH_FAILED);
        assert!(message.contains("401"));
        assert!(message.contains("authentication"));
    }

    #[test]
    fn test_map_http_403_forbidden() {
        let (reason, message) = map_http_error_to_reason(403);
        assert_eq!(reason, REASON_BINDCAR_AUTH_FAILED);
        assert!(message.contains("403"));
        assert!(message.contains("authorization"));
    }

    #[test]
    fn test_map_http_404_not_found() {
        let (reason, message) = map_http_error_to_reason(404);
        assert_eq!(reason, REASON_ZONE_NOT_FOUND);
        assert!(message.contains("404"));
        assert!(message.contains("not found"));
    }

    // ============================================================================
    // Test HTTP 5xx Error Code Mappings
    // ============================================================================

    #[test]
    fn test_map_http_500_internal_server_error() {
        let (reason, message) = map_http_error_to_reason(500);
        assert_eq!(reason, REASON_BINDCAR_INTERNAL_ERROR);
        assert!(message.contains("500"));
        assert!(message.contains("internal error"));
    }

    #[test]
    fn test_map_http_501_not_implemented() {
        let (reason, message) = map_http_error_to_reason(501);
        assert_eq!(reason, REASON_BINDCAR_NOT_IMPLEMENTED);
        assert!(message.contains("501"));
        assert!(message.contains("not supported"));
    }

    #[test]
    fn test_map_http_502_bad_gateway() {
        let (reason, message) = map_http_error_to_reason(502);
        assert_eq!(reason, REASON_GATEWAY_ERROR);
        assert!(message.contains("502"));
        assert!(message.contains("gateway"));
    }

    #[test]
    fn test_map_http_503_service_unavailable() {
        let (reason, message) = map_http_error_to_reason(503);
        assert_eq!(reason, REASON_GATEWAY_ERROR);
        assert!(message.contains("503"));
        assert!(message.contains("unavailable"));
    }

    #[test]
    fn test_map_http_504_gateway_timeout() {
        let (reason, message) = map_http_error_to_reason(504);
        assert_eq!(reason, REASON_GATEWAY_ERROR);
        assert!(message.contains("504"));
        assert!(message.contains("timeout"));
    }

    // ============================================================================
    // Test Unknown/Unmapped HTTP Status Codes
    // ============================================================================

    #[test]
    fn test_map_http_unknown_code() {
        let (reason, message) = map_http_error_to_reason(999);
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("999"));
        assert!(message.contains("Unexpected"));
    }

    #[test]
    fn test_map_http_1xx_informational() {
        // 1xx codes should map to BINDCAR_UNREACHABLE (unexpected)
        let (reason, message) = map_http_error_to_reason(100);
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("100"));
    }

    #[test]
    fn test_map_http_2xx_success() {
        // 2xx codes should map to BINDCAR_UNREACHABLE (unexpected in error context)
        let (reason, message) = map_http_error_to_reason(200);
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("200"));
    }

    #[test]
    fn test_map_http_3xx_redirect() {
        // 3xx codes should map to BINDCAR_UNREACHABLE (unexpected)
        let (reason, message) = map_http_error_to_reason(301);
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("301"));
    }

    #[test]
    fn test_map_http_other_4xx_codes() {
        // Other 4xx codes should map to BINDCAR_UNREACHABLE
        for code in [405, 406, 408, 409, 410, 429] {
            let (reason, message) = map_http_error_to_reason(code);
            assert_eq!(
                reason, REASON_BINDCAR_UNREACHABLE,
                "Code {} should map to BINDCAR_UNREACHABLE",
                code
            );
            assert!(
                message.contains(&code.to_string()),
                "Message should contain code {}",
                code
            );
        }
    }

    #[test]
    fn test_map_http_other_5xx_codes() {
        // Other 5xx codes should map to BINDCAR_UNREACHABLE
        for code in [505, 506, 507, 508, 509, 510, 511] {
            let (reason, message) = map_http_error_to_reason(code);
            assert_eq!(
                reason, REASON_BINDCAR_UNREACHABLE,
                "Code {} should map to BINDCAR_UNREACHABLE",
                code
            );
            assert!(
                message.contains(&code.to_string()),
                "Message should contain code {}",
                code
            );
        }
    }

    // ============================================================================
    // Test Gateway Error Consolidation
    // ============================================================================

    #[test]
    fn test_gateway_errors_map_to_same_reason() {
        // All gateway errors (502, 503, 504) should map to REASON_GATEWAY_ERROR
        let gateway_codes = [502, 503, 504];
        for code in gateway_codes {
            let (reason, _) = map_http_error_to_reason(code);
            assert_eq!(
                reason, REASON_GATEWAY_ERROR,
                "Code {} should map to GATEWAY_ERROR",
                code
            );
        }
    }

    #[test]
    fn test_gateway_errors_have_distinct_messages() {
        // Gateway errors should have different messages even though they map to the same reason
        let (_, msg_502) = map_http_error_to_reason(502);
        let (_, msg_503) = map_http_error_to_reason(503);
        let (_, msg_504) = map_http_error_to_reason(504);

        assert_ne!(msg_502, msg_503);
        assert_ne!(msg_502, msg_504);
        assert_ne!(msg_503, msg_504);
    }

    // ============================================================================
    // Test Authentication/Authorization Error Consolidation
    // ============================================================================

    #[test]
    fn test_auth_errors_map_to_same_reason() {
        // Both 401 and 403 should map to REASON_BINDCAR_AUTH_FAILED
        let (reason_401, _) = map_http_error_to_reason(401);
        let (reason_403, _) = map_http_error_to_reason(403);

        assert_eq!(reason_401, REASON_BINDCAR_AUTH_FAILED);
        assert_eq!(reason_403, REASON_BINDCAR_AUTH_FAILED);
    }

    #[test]
    fn test_auth_errors_have_distinct_messages() {
        // 401 and 403 should have different messages
        let (_, msg_401) = map_http_error_to_reason(401);
        let (_, msg_403) = map_http_error_to_reason(403);

        assert_ne!(msg_401, msg_403);
        assert!(msg_401.contains("authentication"));
        assert!(msg_403.contains("authorization"));
    }

    // ============================================================================
    // Test Connection Error Mapping
    // ============================================================================

    #[test]
    fn test_map_connection_error() {
        let (reason, message) = map_connection_error();
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("Cannot connect"));
        assert!(message.contains("Bindcar"));
    }

    // ============================================================================
    // Test Message Format Consistency
    // ============================================================================

    #[test]
    fn test_all_messages_are_non_empty() {
        // All mapped errors should return non-empty messages
        let test_codes = [400, 401, 403, 404, 500, 501, 502, 503, 504, 999];
        for code in test_codes {
            let (_, message) = map_http_error_to_reason(code);
            assert!(
                !message.is_empty(),
                "Message for code {} should not be empty",
                code
            );
        }
    }

    #[test]
    fn test_all_messages_contain_status_code() {
        // All mapped errors should include the HTTP status code in the message
        let test_codes = [400, 401, 403, 404, 500, 501, 502, 503, 504];
        for code in test_codes {
            let (_, message) = map_http_error_to_reason(code);
            assert!(
                message.contains(&code.to_string()),
                "Message for code {} should contain the status code. Got: {}",
                code,
                message
            );
        }
    }

    #[test]
    fn test_messages_are_actionable() {
        // Messages should provide context about what went wrong
        let (_, msg_400) = map_http_error_to_reason(400);
        let (_, msg_404) = map_http_error_to_reason(404);
        let (_, msg_500) = map_http_error_to_reason(500);

        assert!(msg_400.contains("Invalid") || msg_400.contains("request"));
        assert!(msg_404.contains("not found") || msg_404.contains("Zone"));
        assert!(msg_500.contains("internal") || msg_500.contains("error"));
    }

    // ============================================================================
    // Test Reason Constant Correctness
    // ============================================================================

    #[test]
    fn test_mapped_reasons_match_constants() {
        // Verify that returned reasons match the actual constant values
        let (reason_400, _) = map_http_error_to_reason(400);
        assert_eq!(reason_400, "BindcarBadRequest");

        let (reason_401, _) = map_http_error_to_reason(401);
        assert_eq!(reason_401, "BindcarAuthFailed");

        let (reason_404, _) = map_http_error_to_reason(404);
        assert_eq!(reason_404, "ZoneNotFound");

        let (reason_500, _) = map_http_error_to_reason(500);
        assert_eq!(reason_500, "BindcarInternalError");

        let (reason_502, _) = map_http_error_to_reason(502);
        assert_eq!(reason_502, "GatewayError");

        let (reason_conn, _) = map_connection_error();
        assert_eq!(reason_conn, "BindcarUnreachable");
    }

    // ============================================================================
    // Test Edge Cases
    // ============================================================================

    #[test]
    fn test_zero_status_code() {
        // Code 0 is not a valid HTTP status, should map to unreachable
        let (reason, message) = map_http_error_to_reason(0);
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("0"));
    }

    #[test]
    fn test_very_large_status_code() {
        // Very large codes should map to unreachable
        let (reason, message) = map_http_error_to_reason(9999);
        assert_eq!(reason, REASON_BINDCAR_UNREACHABLE);
        assert!(message.contains("9999"));
    }

    // ============================================================================
    // Test Return Type Consistency
    // ============================================================================

    #[test]
    fn test_return_type_is_static_str_and_string() {
        // Verify the return types are correct (&'static str, String)
        let (reason, message) = map_http_error_to_reason(404);

        // reason should be &'static str (can't directly test lifetime, but we can verify it's a str)
        let _: &str = reason;

        // message should be String (can be moved)
        let _owned: String = message;
    }

    #[test]
    fn test_connection_error_return_type() {
        let (reason, message) = map_connection_error();

        let _: &str = reason;
        let _owned: String = message;
    }
}
