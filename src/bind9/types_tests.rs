// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for BIND9 data types (`RndcKeyData`, `SRVRecordData`, `RndcError`).

#[cfg(test)]
mod tests {
    use crate::bind9::{RndcKeyData, SRVRecordData};

    #[test]
    fn test_rndc_key_data_clone() {
        let key = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let cloned = key.clone();

        assert_eq!(key.name, cloned.name);
        assert_eq!(key.algorithm, cloned.algorithm);
        assert_eq!(key.secret, cloned.secret);
    }

    #[test]
    fn test_rndc_key_data_debug() {
        let key = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha512,
            secret: "c2VjcmV0".to_string(),
        };

        let debug_str = format!("{key:?}");

        // Verify debug output contains all fields
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("HmacSha512")); // Enum variant name in debug output
        assert!(debug_str.contains("c2VjcmV0"));
    }

    #[test]
    fn test_srv_record_data_creation() {
        let srv = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sip.example.com.".to_string(),
            ttl: Some(3600),
        };

        assert_eq!(srv.priority, 10);
        assert_eq!(srv.weight, 60);
        assert_eq!(srv.port, 5060);
        assert_eq!(srv.target, "sip.example.com.");
        assert_eq!(srv.ttl, Some(3600));
    }

    #[test]
    fn test_srv_record_data_without_ttl() {
        let srv = SRVRecordData {
            priority: 0,
            weight: 100,
            port: 389,
            target: "ldap.example.com.".to_string(),
            ttl: None,
        };

        assert_eq!(srv.ttl, None);
    }

    #[test]
    fn test_srv_record_various_ports() {
        let test_cases = vec![
            (80, "http"),
            (443, "https"),
            (389, "ldap"),
            (5060, "sip"),
            (3306, "mysql"),
        ];

        for (port, service) in test_cases {
            let srv = SRVRecordData {
                priority: 10,
                weight: 50,
                port,
                target: format!("{service}.example.com."),
                ttl: Some(3600),
            };

            assert_eq!(srv.port, port);
            assert!(srv.target.contains(service));
        }
    }

    #[test]
    fn test_srv_record_priority_weight_combinations() {
        let combinations = vec![(0, 0), (10, 50), (20, 100), (100, 1)];

        for (priority, weight) in combinations {
            let srv = SRVRecordData {
                priority,
                weight,
                port: 443,
                target: "server.example.com.".to_string(),
                ttl: None,
            };

            assert_eq!(srv.priority, priority);
            assert_eq!(srv.weight, weight);
        }
    }

    #[test]
    fn test_rndc_key_data_with_different_algorithms() {
        use crate::crd::RndcAlgorithm;
        let algorithms = vec![
            RndcAlgorithm::HmacSha256,
            RndcAlgorithm::HmacSha512,
            RndcAlgorithm::HmacMd5,
        ];

        for algorithm in algorithms {
            let key = RndcKeyData {
                name: "test".to_string(),
                algorithm: algorithm.clone(),
                secret: "dGVzdA==".to_string(),
            };

            assert_eq!(key.algorithm, algorithm);
        }
    }

    #[test]
    fn test_srv_record_data_edge_case_values() {
        // Test minimum values
        let srv_min = SRVRecordData {
            priority: 0,
            weight: 0,
            port: 0,
            target: ".".to_string(), // Root domain
            ttl: Some(0),
        };
        assert_eq!(srv_min.priority, 0);
        assert_eq!(srv_min.weight, 0);
        assert_eq!(srv_min.port, 0);

        // Test maximum reasonable values
        let srv_max = SRVRecordData {
            priority: 65535,
            weight: 65535,
            port: 65535,
            target: "very.long.subdomain.example.com.".to_string(),
            ttl: Some(2_147_483_647), // Max i32
        };
        assert_eq!(srv_max.priority, 65535);
        assert_eq!(srv_max.weight, 65535);
        assert_eq!(srv_max.port, 65535);
        assert_eq!(srv_max.ttl, Some(2_147_483_647));
    }

    #[test]
    fn test_srv_record_data_negative_values() {
        // Test negative priority (should be allowed by type but semantically invalid)
        let srv = SRVRecordData {
            priority: -1,
            weight: -1,
            port: -1,
            target: "server.example.com.".to_string(),
            ttl: Some(-1),
        };
        assert_eq!(srv.priority, -1);
        assert_eq!(srv.weight, -1);
        assert_eq!(srv.port, -1);
        assert_eq!(srv.ttl, Some(-1));
    }

    #[test]
    fn test_srv_record_target_trailing_dot() {
        let srv_with_dot = SRVRecordData {
            priority: 10,
            weight: 50,
            port: 443,
            target: "server.example.com.".to_string(), // With trailing dot
            ttl: None,
        };

        let srv_without_dot = SRVRecordData {
            priority: 10,
            weight: 50,
            port: 443,
            target: "server.example.com".to_string(), // Without trailing dot
            ttl: None,
        };

        assert!(srv_with_dot.target.ends_with('.'));
        assert!(!srv_without_dot.target.ends_with('.'));
    }

    #[test]
    fn test_rndc_key_data_empty_secret() {
        let key = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: String::new(),
        };

        assert_eq!(key.secret, "");
    }

    #[test]
    fn test_rndc_key_data_very_long_secret() {
        let long_secret = "A".repeat(10000);
        let key = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: long_secret.clone(),
        };

        assert_eq!(key.secret.len(), 10000);
        assert_eq!(key.secret, long_secret);
    }

    #[test]
    fn test_srv_record_zero_ttl() {
        let srv = SRVRecordData {
            priority: 10,
            weight: 50,
            port: 443,
            target: "server.example.com.".to_string(),
            ttl: Some(0),
        };

        assert_eq!(srv.ttl, Some(0));
    }

    #[test]
    fn test_srv_record_max_i32_ttl() {
        let srv = SRVRecordData {
            priority: 10,
            weight: 50,
            port: 443,
            target: "server.example.com.".to_string(),
            ttl: Some(i32::MAX),
        };

        assert_eq!(srv.ttl, Some(i32::MAX));
    }
}
