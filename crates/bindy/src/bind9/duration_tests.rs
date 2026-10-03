// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for duration parsing (Go-style duration format)

#[cfg(test)]
mod tests {
    use super::super::parse_duration;
    use std::time::Duration;

    // ========================================================================
    // Valid Duration Parsing Tests
    // ========================================================================

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(
            parse_duration("1h").unwrap(),
            Duration::from_secs(3600),
            "1 hour should be 3600 seconds"
        );
        assert_eq!(
            parse_duration("24h").unwrap(),
            Duration::from_secs(86400),
            "24 hours should be 86400 seconds"
        );
        assert_eq!(
            parse_duration("720h").unwrap(),
            Duration::from_secs(2_592_000),
            "720 hours (30 days) should be 2592000 seconds"
        );
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(
            parse_duration("1d").unwrap(),
            Duration::from_secs(86400),
            "1 day should be 86400 seconds"
        );
        assert_eq!(
            parse_duration("30d").unwrap(),
            Duration::from_secs(2_592_000),
            "30 days should be 2592000 seconds"
        );
        assert_eq!(
            parse_duration("90d").unwrap(),
            Duration::from_secs(7_776_000),
            "90 days should be 7776000 seconds"
        );
    }

    #[test]
    fn test_parse_duration_weeks() {
        assert_eq!(
            parse_duration("1w").unwrap(),
            Duration::from_secs(604_800),
            "1 week should be 604800 seconds"
        );
        assert_eq!(
            parse_duration("4w").unwrap(),
            Duration::from_secs(2_419_200),
            "4 weeks should be 2419200 seconds"
        );
    }

    // ========================================================================
    // Invalid Format Tests
    // ========================================================================

    #[test]
    fn test_parse_duration_empty_string() {
        let result = parse_duration("");
        assert!(result.is_err(), "Empty string should return an error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("cannot be empty"),
            "Error should mention empty string"
        );
    }

    #[test]
    fn test_parse_duration_no_digits() {
        let result = parse_duration("abc");
        assert!(
            result.is_err(),
            "String with no digits should return an error"
        );
    }

    #[test]
    fn test_parse_duration_no_unit() {
        let result = parse_duration("10");
        assert!(
            result.is_err(),
            "Duration without unit should return an error"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("must end with a unit"),
            "Error should mention missing unit"
        );
    }

    #[test]
    fn test_parse_duration_invalid_unit() {
        let result = parse_duration("10x");
        assert!(
            result.is_err(),
            "Duration with invalid unit should return an error"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Unsupported duration unit"),
            "Error should mention unsupported unit"
        );
    }

    #[test]
    fn test_parse_duration_negative_value() {
        let result = parse_duration("-5h");
        assert!(result.is_err(), "Negative duration should return an error");
    }

    // ========================================================================
    // Boundary Tests
    // ========================================================================

    #[test]
    fn test_parse_duration_at_minimum() {
        // Exactly at minimum (1 hour)
        let result = parse_duration("1h");
        assert!(result.is_ok(), "Duration of exactly 1h should be valid");
        assert_eq!(result.unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn test_parse_duration_at_maximum() {
        // Exactly at maximum (8760 hours = 365 days)
        let result_hours = parse_duration("8760h");
        assert!(
            result_hours.is_ok(),
            "Duration of exactly 8760h should be valid"
        );

        let result_days = parse_duration("365d");
        assert!(
            result_days.is_ok(),
            "Duration of exactly 365d should be valid"
        );

        // Both should be equivalent
        assert_eq!(result_hours.unwrap(), result_days.unwrap());
    }

    #[test]
    fn test_parse_duration_above_maximum() {
        // Above maximum (8760 hours)
        let result_hours = parse_duration("8761h");
        assert!(
            result_hours.is_err(),
            "Duration above 8760h should return an error"
        );
        let err_hours = result_hours.unwrap_err();
        assert!(
            err_hours.to_string().contains("exceeds maximum"),
            "Error should mention maximum bound"
        );

        // Above maximum (365 days)
        let result_days = parse_duration("366d");
        assert!(
            result_days.is_err(),
            "Duration above 365d should return an error"
        );
    }

    // ========================================================================
    // Edge Cases
    // ========================================================================

    #[test]
    fn test_parse_duration_zero() {
        let result = parse_duration("0h");
        assert!(
            result.is_err(),
            "Duration of 0h should return an error (below minimum)"
        );
    }

    #[test]
    fn test_parse_duration_large_numbers() {
        // Large but valid number
        let result = parse_duration("8000h");
        assert!(result.is_ok(), "8000h should be valid (within bounds)");
        assert_eq!(result.unwrap(), Duration::from_secs(28_800_000));
    }

    #[test]
    fn test_parse_duration_whitespace() {
        // Leading/trailing whitespace should fail
        let result = parse_duration(" 24h ");
        assert!(
            result.is_err(),
            "Duration with whitespace should return an error"
        );
    }

    // ========================================================================
    // Real-World Usage Tests
    // ========================================================================

    #[test]
    fn test_parse_duration_common_intervals() {
        // 30 days (common rotation interval)
        assert_eq!(
            parse_duration("720h").unwrap(),
            parse_duration("30d").unwrap(),
            "720h and 30d should be equivalent"
        );

        // 90 days (recommended maximum)
        assert_eq!(
            parse_duration("2160h").unwrap(),
            parse_duration("90d").unwrap(),
            "2160h and 90d should be equivalent"
        );

        // 7 days
        assert_eq!(
            parse_duration("168h").unwrap(),
            parse_duration("7d").unwrap(),
            "168h and 7d should be equivalent"
        );
        assert_eq!(
            parse_duration("7d").unwrap(),
            parse_duration("1w").unwrap(),
            "7d and 1w should be equivalent"
        );
    }
}
