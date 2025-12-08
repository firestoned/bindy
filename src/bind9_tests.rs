// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for bind9 RNDC protocol management
//!
//! These tests verify the RNDC key generation, secret parsing, and data structures
//! used by the `Bind9Manager` for communicating with BIND9 servers.

#[cfg(test)]
#[allow(unexpected_cfgs)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData, SRVRecordData};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use bindcar::ZONE_TYPE_PRIMARY;
    use std::collections::BTreeMap;

    #[test]
    fn test_bind9_manager_creation() {
        let manager = Bind9Manager::new();
        // Verify manager can be created
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
        assert!(debug_output.contains("client"));
        assert!(debug_output.contains("token"));
    }

    #[test]
    fn test_bind9_manager_default() {
        let manager = Bind9Manager::new();
        // Verify default implementation works
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
    }

    #[test]
    fn test_generate_rndc_key() {
        let key = Bind9Manager::generate_rndc_key();

        // Verify key has correct algorithm
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);

        // Verify secret is base64 encoded
        assert!(!key.secret.is_empty());
        assert!(BASE64.decode(&key.secret).is_ok());

        // Verify secret is 32 bytes (256 bits) when decoded
        let decoded = BASE64.decode(&key.secret).unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn test_generate_rndc_key_uniqueness() {
        let key1 = Bind9Manager::generate_rndc_key();
        let key2 = Bind9Manager::generate_rndc_key();

        // Verify each generated key is unique
        assert_ne!(key1.secret, key2.secret);
    }

    #[test]
    fn test_create_rndc_secret_data() {
        let key = RndcKeyData {
            name: "test-instance".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdHNlY3JldA==".to_string(),
        };

        let secret_data = Bind9Manager::create_rndc_secret_data(&key);

        // Verify all fields are present
        assert_eq!(
            secret_data.get("key-name"),
            Some(&"test-instance".to_string())
        );
        assert_eq!(
            secret_data.get("algorithm"),
            Some(&"hmac-sha256".to_string())
        );
        assert_eq!(
            secret_data.get("secret"),
            Some(&"dGVzdHNlY3JldA==".to_string())
        );
        // Verify rndc.key file content is generated
        assert!(secret_data.contains_key("rndc.key"));
        let rndc_key_content = secret_data.get("rndc.key").unwrap();
        assert!(rndc_key_content.contains("key \"test-instance\" {"));
        assert!(rndc_key_content.contains("algorithm hmac-sha256;"));
        assert!(rndc_key_content.contains("secret \"dGVzdHNlY3JldA==\";"));
        assert_eq!(secret_data.len(), 4);
    }

    #[test]
    fn test_parse_rndc_secret_data() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"bind9-primary".to_vec());
        data.insert("algorithm".to_string(), b"hmac-sha256".to_vec());
        data.insert("secret".to_string(), b"dGVzdHNlY3JldA==".to_vec());

        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        assert_eq!(key.name, "bind9-primary");
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);
        assert_eq!(key.secret, "dGVzdHNlY3JldA==");
    }

    #[test]
    fn test_parse_rndc_secret_data_missing_all_fields() {
        let data = BTreeMap::new();

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rndc.key"));
    }

    #[test]
    fn test_parse_rndc_secret_data_from_rndc_key_file() {
        // Test parsing external secret with only rndc.key field
        let rndc_key_content = r#"key "bindy-operator" {
    algorithm hmac-sha256;
    secret "dGVzdHNlY3JldA==";
};
"#;
        let mut data = BTreeMap::new();
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        assert_eq!(key.name, "bindy-operator");
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);
        assert_eq!(key.secret, "dGVzdHNlY3JldA==");
    }

    #[test]
    fn test_parse_rndc_secret_data_prefers_metadata_fields() {
        // If both formats are present (operator-generated secret), prefer metadata fields
        let rndc_key_content = r#"key "wrong-name" {
    algorithm hmac-sha1;
    secret "wrongsecret==";
};
"#;
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"correct-name".to_vec());
        data.insert("algorithm".to_string(), b"hmac-sha256".to_vec());
        data.insert("secret".to_string(), b"correctsecret==".to_vec());
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        // Should use metadata fields, not parse rndc.key
        assert_eq!(key.name, "correct-name");
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);
        assert_eq!(key.secret, "correctsecret==");
    }

    #[test]
    fn test_parse_rndc_secret_data_invalid_utf8() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), vec![0xFF, 0xFE, 0xFD]); // Invalid UTF-8
        data.insert("algorithm".to_string(), b"hmac-sha256".to_vec());
        data.insert("secret".to_string(), b"dGVzdHNlY3JldA==".to_vec());

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
    }

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
    fn test_round_trip_rndc_key_secret_data() {
        // Generate a key
        let mut original_key = Bind9Manager::generate_rndc_key();
        original_key.name = "test-instance".to_string();

        // Convert to secret data
        let secret_data = Bind9Manager::create_rndc_secret_data(&original_key);

        // Convert secret data to bytes (simulating k8s Secret storage)
        let mut byte_data = BTreeMap::new();
        for (k, v) in secret_data {
            byte_data.insert(k, v.into_bytes());
        }

        // Parse back
        let parsed_key = Bind9Manager::parse_rndc_secret_data(&byte_data).unwrap();

        // Verify round-trip
        assert_eq!(original_key.name, parsed_key.name);
        assert_eq!(original_key.algorithm, parsed_key.algorithm);
        assert_eq!(original_key.secret, parsed_key.secret);
    }

    #[test]
    fn test_generate_multiple_keys_with_names() {
        let names = vec!["primary", "secondary", "backup"];
        let mut keys = Vec::new();

        for name in names {
            let mut key = Bind9Manager::generate_rndc_key();
            key.name = name.to_string();
            keys.push(key);
        }

        // Verify all keys have unique secrets
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i].secret, keys[j].secret);
            }
        }

        // Verify names are set correctly
        assert_eq!(keys[0].name, "primary");
        assert_eq!(keys[1].name, "secondary");
        assert_eq!(keys[2].name, "backup");
    }

    #[test]
    fn test_hmac_sha256_algorithm() {
        let key = Bind9Manager::generate_rndc_key();
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);
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
    fn test_base64_secret_encoding() {
        let key = Bind9Manager::generate_rndc_key();

        // Decode the secret
        let decoded = BASE64.decode(&key.secret).unwrap();

        // Re-encode it
        let reencoded = BASE64.encode(&decoded);

        // Should match original
        assert_eq!(key.secret, reencoded);
    }

    #[test]
    fn test_secret_data_btreemap_ordering() {
        let key = RndcKeyData {
            name: "instance".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "c2VjcmV0".to_string(),
        };

        let data = Bind9Manager::create_rndc_secret_data(&key);

        // BTreeMap should have keys in sorted order
        let keys: Vec<&String> = data.keys().collect();
        assert_eq!(keys, vec!["algorithm", "key-name", "rndc.key", "secret"]);
    }

    // =====================================================
    // Comprehensive Edge-Case Tests for Missing Coverage
    // =====================================================

    // Tests for all supported RNDC algorithms
    #[test]
    fn test_parse_rndc_secret_data_all_algorithms() {
        use crate::crd::RndcAlgorithm;

        let algorithms = vec![
            ("hmac-md5", RndcAlgorithm::HmacMd5),
            ("hmac-sha1", RndcAlgorithm::HmacSha1),
            ("hmac-sha224", RndcAlgorithm::HmacSha224),
            ("hmac-sha256", RndcAlgorithm::HmacSha256),
            ("hmac-sha384", RndcAlgorithm::HmacSha384),
            ("hmac-sha512", RndcAlgorithm::HmacSha512),
        ];

        for (algo_str, expected_algo) in algorithms {
            let mut data = BTreeMap::new();
            data.insert("key-name".to_string(), b"test-key".to_vec());
            data.insert("algorithm".to_string(), algo_str.as_bytes().to_vec());
            data.insert("secret".to_string(), b"dGVzdHNlY3JldA==".to_vec());

            let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

            assert_eq!(key.algorithm, expected_algo);
            assert_eq!(key.name, "test-key");
            assert_eq!(key.secret, "dGVzdHNlY3JldA==");
        }
    }

    #[test]
    fn test_parse_rndc_secret_data_unsupported_algorithm() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"test-key".to_vec());
        data.insert("algorithm".to_string(), b"hmac-unsupported".to_vec());
        data.insert("secret".to_string(), b"dGVzdHNlY3JldA==".to_vec());

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unsupported RNDC algorithm"));
        assert!(err_msg.contains("hmac-unsupported"));
    }

    #[test]
    fn test_parse_rndc_secret_data_invalid_utf8_in_algorithm() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"test-key".to_vec());
        data.insert("algorithm".to_string(), vec![0xFF, 0xFE, 0xFD]); // Invalid UTF-8
        data.insert("secret".to_string(), b"dGVzdHNlY3JldA==".to_vec());

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_rndc_secret_data_invalid_utf8_in_secret() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"test-key".to_vec());
        data.insert("algorithm".to_string(), b"hmac-sha256".to_vec());
        data.insert("secret".to_string(), vec![0xFF, 0xFE, 0xFD]); // Invalid UTF-8

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_rndc_secret_data_empty_key_name() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"".to_vec()); // Empty key name
        data.insert("algorithm".to_string(), b"hmac-sha256".to_vec());
        data.insert("secret".to_string(), b"dGVzdHNlY3JldA==".to_vec());

        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        assert_eq!(key.name, "");
    }

    #[test]
    fn test_parse_rndc_secret_data_empty_secret() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"test-key".to_vec());
        data.insert("algorithm".to_string(), b"hmac-sha256".to_vec());
        data.insert("secret".to_string(), b"".to_vec()); // Empty secret

        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        assert_eq!(key.secret, "");
    }

    #[test]
    fn test_create_rndc_secret_data_all_algorithms() {
        use crate::crd::RndcAlgorithm;

        let algorithms = vec![
            (RndcAlgorithm::HmacMd5, "hmac-md5"),
            (RndcAlgorithm::HmacSha1, "hmac-sha1"),
            (RndcAlgorithm::HmacSha224, "hmac-sha224"),
            (RndcAlgorithm::HmacSha256, "hmac-sha256"),
            (RndcAlgorithm::HmacSha384, "hmac-sha384"),
            (RndcAlgorithm::HmacSha512, "hmac-sha512"),
        ];

        for (algorithm, expected_str) in algorithms {
            let key = RndcKeyData {
                name: "test-instance".to_string(),
                algorithm: algorithm.clone(),
                secret: "dGVzdHNlY3JldA==".to_string(),
            };

            let secret_data = Bind9Manager::create_rndc_secret_data(&key);

            assert_eq!(
                secret_data.get("algorithm"),
                Some(&expected_str.to_string())
            );
            assert_eq!(
                secret_data.get("key-name"),
                Some(&"test-instance".to_string())
            );
        }
    }

    #[test]
    fn test_generate_rndc_key_produces_valid_base64() {
        for _ in 0..10 {
            let key = Bind9Manager::generate_rndc_key();

            // Verify secret is valid base64
            let decoded = BASE64.decode(&key.secret).expect("Should be valid base64");

            // Verify it's exactly 32 bytes
            assert_eq!(decoded.len(), 32);

            // Verify it doesn't contain only zeros (extremely unlikely but possible)
            assert!(decoded.iter().any(|&b| b != 0));
        }
    }

    #[test]
    fn test_rndc_key_data_name_with_special_characters() {
        let key = RndcKeyData {
            name: "bind9-primary_01.example-cluster".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let secret_data = Bind9Manager::create_rndc_secret_data(&key);
        let mut byte_data = BTreeMap::new();
        for (k, v) in secret_data {
            byte_data.insert(k, v.into_bytes());
        }

        let parsed = Bind9Manager::parse_rndc_secret_data(&byte_data).unwrap();

        assert_eq!(parsed.name, "bind9-primary_01.example-cluster");
    }

    #[test]
    fn test_rndc_key_data_very_long_name() {
        let long_name = "a".repeat(253); // DNS label max length
        let key = RndcKeyData {
            name: long_name.clone(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha512,
            secret: "dGVzdA==".to_string(),
        };

        let secret_data = Bind9Manager::create_rndc_secret_data(&key);
        let mut byte_data = BTreeMap::new();
        for (k, v) in secret_data {
            byte_data.insert(k, v.into_bytes());
        }

        let parsed = Bind9Manager::parse_rndc_secret_data(&byte_data).unwrap();

        assert_eq!(parsed.name, long_name);
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
            ttl: Some(2147483647), // Max i32
        };
        assert_eq!(srv_max.priority, 65535);
        assert_eq!(srv_max.weight, 65535);
        assert_eq!(srv_max.port, 65535);
        assert_eq!(srv_max.ttl, Some(2147483647));
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
    fn test_rndc_key_data_unicode_in_name() {
        let key = RndcKeyData {
            name: "bind9-主要".to_string(), // Chinese characters
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let secret_data = Bind9Manager::create_rndc_secret_data(&key);
        let mut byte_data = BTreeMap::new();
        for (k, v) in secret_data {
            byte_data.insert(k, v.into_bytes());
        }

        let parsed = Bind9Manager::parse_rndc_secret_data(&byte_data).unwrap();

        assert_eq!(parsed.name, "bind9-主要");
    }

    #[test]
    fn test_round_trip_all_algorithms() {
        use crate::crd::RndcAlgorithm;

        let algorithms = vec![
            RndcAlgorithm::HmacMd5,
            RndcAlgorithm::HmacSha1,
            RndcAlgorithm::HmacSha224,
            RndcAlgorithm::HmacSha256,
            RndcAlgorithm::HmacSha384,
            RndcAlgorithm::HmacSha512,
        ];

        for algorithm in algorithms {
            let original_key = RndcKeyData {
                name: "test-instance".to_string(),
                algorithm: algorithm.clone(),
                secret: "dGVzdHNlY3JldA==".to_string(),
            };

            let secret_data = Bind9Manager::create_rndc_secret_data(&original_key);
            let mut byte_data = BTreeMap::new();
            for (k, v) in secret_data {
                byte_data.insert(k, v.into_bytes());
            }

            let parsed_key = Bind9Manager::parse_rndc_secret_data(&byte_data).unwrap();

            assert_eq!(original_key.name, parsed_key.name);
            assert_eq!(original_key.algorithm, parsed_key.algorithm);
            assert_eq!(original_key.secret, parsed_key.secret);
        }
    }

    #[test]
    fn test_parse_rndc_secret_data_with_extra_fields() {
        let mut data = BTreeMap::new();
        data.insert("key-name".to_string(), b"test-key".to_vec());
        data.insert("algorithm".to_string(), b"hmac-sha256".to_vec());
        data.insert("secret".to_string(), b"dGVzdHNlY3JldA==".to_vec());
        data.insert("extra-field".to_string(), b"should-be-ignored".to_vec());
        data.insert("another-extra".to_string(), b"also-ignored".to_vec());

        // Should succeed and ignore extra fields
        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        assert_eq!(key.name, "test-key");
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);
        assert_eq!(key.secret, "dGVzdHNlY3JldA==");
    }

    #[test]
    fn test_generate_rndc_key_no_name_by_default() {
        let key = Bind9Manager::generate_rndc_key();

        // Generated keys should have empty name by default
        assert_eq!(key.name, "");
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
    fn test_rndc_secret_with_special_base64_characters() {
        // Test secrets with all valid base64 characters including +, /, and =
        let special_secrets = vec![
            "AAAA++++////====",
            "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6",
            "dGVzdC90ZXN0K3Rlc3Q9dGVzdA==",
        ];

        for secret in special_secrets {
            let key = RndcKeyData {
                name: "test".to_string(),
                algorithm: crate::crd::RndcAlgorithm::HmacSha256,
                secret: secret.to_string(),
            };

            let secret_data = Bind9Manager::create_rndc_secret_data(&key);
            let mut byte_data = BTreeMap::new();
            for (k, v) in secret_data {
                byte_data.insert(k, v.into_bytes());
            }

            let parsed = Bind9Manager::parse_rndc_secret_data(&byte_data).unwrap();

            assert_eq!(parsed.secret, secret);
        }
    }

    // Async method tests - placeholders since actual implementation requires mocking

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_a_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        // This test now requires a real BIND9 server with TSIG authentication
        let result = manager
            .add_a_record(
                "example.com",
                "www",
                "192.0.2.1",
                Some(300),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        // Will fail without real server - test is ignored by default
        let _ = result;
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_aaaa_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_aaaa_record(
                "example.com",
                "www",
                "2001:db8::1",
                Some(300),
                "localhost:953",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_cname_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_cname_record(
                "example.com",
                "blog",
                "www.example.com.",
                Some(300),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        let _ = result;
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_txt_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let texts = vec!["v=spf1 mx ~all".to_string()];
        let result = manager
            .add_txt_record(
                "example.com",
                "@",
                &texts,
                Some(3600),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        let _ = result;
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_mx_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_mx_record(
                "example.com",
                "@",
                10,
                "mail.example.com.",
                Some(3600),
                "localhost:953",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_ns_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_ns_record(
                "example.com",
                "@",
                "ns1.example.com.",
                Some(3600),
                "localhost:953",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_srv_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sip.example.com.".to_string(),
            ttl: Some(3600),
        };

        let result = manager
            .add_srv_record(
                "example.com",
                "_sip._tcp",
                &srv_data,
                "localhost:953",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_caa_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issue",
                "letsencrypt.org",
                Some(3600),
                "localhost:953",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }

    // =====================================================
    // HTTP API URL Building Tests
    // =====================================================

    #[test]
    fn test_build_api_url_with_http() {
        let url = crate::bind9::Bind9Manager::build_api_url("http://localhost:8080");
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn test_build_api_url_without_scheme() {
        let url = crate::bind9::Bind9Manager::build_api_url("localhost:8080");
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn test_build_api_url_with_https() {
        let url = crate::bind9::Bind9Manager::build_api_url("https://api.example.com:8443");
        assert_eq!(url, "https://api.example.com:8443");
    }

    #[test]
    fn test_build_api_url_trailing_slash() {
        let url = crate::bind9::Bind9Manager::build_api_url("http://localhost:8080/");
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn test_build_api_url_ipv4() {
        let url = crate::bind9::Bind9Manager::build_api_url("192.168.1.1:8080");
        assert_eq!(url, "http://192.168.1.1:8080");
    }

    #[test]
    fn test_build_api_url_ipv6() {
        let url = crate::bind9::Bind9Manager::build_api_url("[::1]:8080");
        assert_eq!(url, "http://[::1]:8080");
    }

    #[test]
    fn test_build_api_url_dns_name() {
        let url = crate::bind9::Bind9Manager::build_api_url(
            "bind9-api.dns-system.svc.cluster.local:8080",
        );
        assert_eq!(url, "http://bind9-api.dns-system.svc.cluster.local:8080");
    }

    // =====================================================
    // Negative Test Cases for HTTP API
    // =====================================================

    #[tokio::test]
    #[ignore = "Requires mock HTTP server or real server returning errors"]
    async fn test_reload_zone_not_found() {
        let manager = Bind9Manager::new();

        // Should return error when zone doesn't exist
        let result = manager
            .reload_zone("nonexistent.com", "localhost:8080")
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not found") || err_msg.contains("404"));
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server returning 500 error"]
    async fn test_server_status_500_error() {
        let manager = Bind9Manager::new();

        // Should handle 500 errors gracefully
        let result = manager.server_status("localhost:8080").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server with timeout"]
    async fn test_http_request_timeout() {
        let manager = Bind9Manager::new();

        // Should timeout if server is unresponsive
        let result = manager
            .reload_zone("example.com", "10.255.255.1:8080") // Non-routable IP
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("timeout") || err_msg.contains("connect"));
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server returning invalid JSON"]
    async fn test_invalid_json_response() {
        let manager = Bind9Manager::new();

        // Should handle malformed JSON responses
        let result = manager.server_status("localhost:8080").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_add_zone_duplicate() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test-key".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdHNlY3JldA==".to_string(),
        };

        let soa_record = crate::crd::SOARecord {
            primary_ns: "ns1.example.com.".to_string(),
            admin_email: "admin.example.com.".to_string(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604_800,
            negative_ttl: 86400,
        };

        // First add should succeed
        let result1 = manager
            .add_zone(
                "example.com",
                ZONE_TYPE_PRIMARY,
                "localhost:8080",
                &key_data,
                &soa_record,
                None,
                None, // no secondary IPs
            )
            .await;
        assert!(result1.is_ok());

        // Second add of same zone should be idempotent (no error)
        let result2 = manager
            .add_zone(
                "example.com",
                ZONE_TYPE_PRIMARY,
                "localhost:8080",
                &key_data,
                &soa_record,
                None,
                None, // no secondary IPs
            )
            .await;
        assert!(result2.is_ok());
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_delete_nonexistent_zone() {
        let manager = Bind9Manager::new();

        // Deleting non-existent zone should not error (idempotent)
        let result = manager
            .delete_zone("nonexistent.com", "localhost:8080")
            .await;

        // Should either succeed or return specific "not found" error
        if result.is_err() {
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("not found") || err_msg.contains("404"));
        }
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_zone_exists_false() {
        let manager = Bind9Manager::new();

        let exists = manager
            .zone_exists("definitely-does-not-exist-12345.com", "localhost:8080")
            .await;

        assert!(!exists);
    }

    #[tokio::test]
    #[ignore = "Requires mock HTTP server"]
    async fn test_zone_exists_connection_error() {
        let manager = Bind9Manager::new();

        // Should return false on connection error
        let exists = manager
            .zone_exists("example.com", "invalid-host:99999")
            .await;

        assert!(!exists);
    }

    // =====================================================
    // Edge Case Tests for HTTP API
    // =====================================================

    #[test]
    fn test_build_api_url_empty_string() {
        let url = crate::bind9::Bind9Manager::build_api_url("");
        // Should handle empty string gracefully
        assert!(url.is_empty() || url == "http://");
    }

    #[test]
    fn test_build_api_url_only_port() {
        let url = crate::bind9::Bind9Manager::build_api_url(":8080");
        assert_eq!(url, "http://:8080");
    }

    #[test]
    fn test_build_api_url_no_port() {
        let url = crate::bind9::Bind9Manager::build_api_url("localhost");
        assert_eq!(url, "http://localhost");
    }

    #[test]
    fn test_build_api_url_multiple_slashes() {
        let url = crate::bind9::Bind9Manager::build_api_url("http://localhost:8080///");
        assert_eq!(url, "http://localhost:8080");
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

    // =====================================================
    // ZoneConfig and bindcar Integration Tests
    // =====================================================

    #[test]
    fn test_zone_config_to_zone_file_basic() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 2025010101,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
        };

        let zone_file = zone_config.to_zone_file();

        assert!(zone_file.contains("$TTL 3600"));
        assert!(zone_file.contains("@ IN SOA ns1.example.com. admin.example.com."));
        assert!(zone_file.contains("2025010101"));
        assert!(zone_file.contains("@ IN NS ns1.example.com."));
    }

    #[test]
    fn test_zone_config_with_dns_records() {
        use bindcar::{DnsRecord, SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![
                DnsRecord {
                    name: "@".to_string(),
                    record_type: "A".to_string(),
                    value: "192.0.2.1".to_string(),
                    ttl: None,
                    priority: None,
                },
                DnsRecord {
                    name: "www".to_string(),
                    record_type: "A".to_string(),
                    value: "192.0.2.2".to_string(),
                    ttl: Some(300),
                    priority: None,
                },
            ],
            also_notify: None,
            allow_transfer: None,
        };

        let zone_file = zone_config.to_zone_file();

        assert!(zone_file.contains("@ IN A 192.0.2.1"));
        assert!(zone_file.contains("www 300 IN A 192.0.2.2"));
    }

    #[test]
    fn test_zone_config_minimal() {
        use bindcar::{SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 300,
            soa: SoaRecord {
                primary_ns: "ns.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec![],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
        };

        let zone_file = zone_config.to_zone_file();

        assert!(zone_file.contains("$TTL 300"));
        assert!(zone_file.contains("@ IN SOA ns.example.com. admin.example.com."));
    }

    #[test]
    fn test_create_zone_request_serialization() {
        use bindcar::{CreateZoneRequest, SoaRecord, ZoneConfig};
        use std::collections::HashMap;

        let zone_config = ZoneConfig {
            ttl: 3600,
            soa: SoaRecord {
                primary_ns: "ns1.example.com.".to_string(),
                admin_email: "admin.example.com.".to_string(),
                serial: 1,
                refresh: 3600,
                retry: 600,
                expire: 604800,
                negative_ttl: 86400,
            },
            name_servers: vec!["ns1.example.com.".to_string()],
            name_server_ips: HashMap::new(),
            records: vec![],
            also_notify: None,
            allow_transfer: None,
        };

        let request = CreateZoneRequest {
            zone_name: "example.com".to_string(),
            zone_type: ZONE_TYPE_PRIMARY.to_string(),
            zone_config,
            update_key_name: Some("bind9-key".to_string()),
        };

        // Verify it can be serialized to JSON
        let json = serde_json::to_string(&request);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("zoneName"));
        assert!(json_str.contains("example.com"));
        assert!(json_str.contains("zoneType"));
        assert!(json_str.contains(ZONE_TYPE_PRIMARY));
        assert!(json_str.contains("zoneConfig"));
        assert!(json_str.contains("updateKeyName"));
        assert!(json_str.contains("bind9-key"));
    }

    #[test]
    fn test_zone_response_deserialization() {
        use bindcar::ZoneResponse;

        let json = r#"{"success": true, "message": "Zone created successfully"}"#;

        let response: Result<ZoneResponse, _> = serde_json::from_str(json);
        assert!(response.is_ok());

        let response = response.unwrap();
        assert!(response.success);
        assert_eq!(response.message, "Zone created successfully");
        assert_eq!(response.details, None);
    }

    #[test]
    fn test_zone_response_deserialization_with_details() {
        use bindcar::ZoneResponse;

        let json = r#"{
            "success": false,
            "message": "Zone creation failed",
            "details": "Zone already exists"
        }"#;

        let response: Result<ZoneResponse, _> = serde_json::from_str(json);
        assert!(response.is_ok());

        let response = response.unwrap();
        assert!(!response.success);
        assert_eq!(response.message, "Zone creation failed");
        assert_eq!(response.details, Some("Zone already exists".to_string()));
    }

    #[test]
    fn test_soa_record_default_values() {
        use bindcar::SoaRecord;

        let soa = SoaRecord {
            primary_ns: "ns.example.com.".to_string(),
            admin_email: "admin.example.com.".to_string(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        assert_eq!(soa.refresh, 3600);
        assert_eq!(soa.retry, 600);
        assert_eq!(soa.expire, 604800);
        assert_eq!(soa.negative_ttl, 86400);
    }

    #[test]
    fn test_dns_record_with_mx_priority() {
        use bindcar::DnsRecord;

        let record = DnsRecord {
            name: "@".to_string(),
            record_type: "MX".to_string(),
            value: "mail.example.com.".to_string(),
            ttl: Some(3600),
            priority: Some(10),
        };

        assert_eq!(record.priority, Some(10));
        assert_eq!(record.record_type, "MX");
    }

    #[test]
    fn test_dns_record_without_priority() {
        use bindcar::DnsRecord;

        let record = DnsRecord {
            name: "www".to_string(),
            record_type: "A".to_string(),
            value: "192.0.2.1".to_string(),
            ttl: None,
            priority: None,
        };

        assert_eq!(record.priority, None);
    }
}

// Disable old file-based tests - they test the old API
#[allow(unexpected_cfgs)]
#[cfg(all(test, feature = "file-based-tests"))]
mod old_tests {
    use crate::bind9::{Bind9Manager, SRVRecordData};
    use crate::crd::SOARecord;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Bind9Manager) {
        let temp_dir = TempDir::new().unwrap();
        let manager = Bind9Manager::new(temp_dir.path().to_string_lossy().to_string());
        (temp_dir, manager)
    }

    fn create_test_zone(manager: &Bind9Manager) {
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };
        manager.create_zone_file("example.com", &soa, 3600).unwrap();
    }

    #[test]
    fn test_create_zone_file() {
        let (_temp_dir, manager) = setup();

        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };
        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        assert!(manager.zone_exists("example.com"));

        // Verify content
        let path = Path::new(&manager.zones_dir()).join("db.example.com");
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("$TTL 3600"));
        assert!(content.contains("ns1.example.com."));
        assert!(content.contains("admin.example.com"));
        assert!(content.contains("2024010101"));
    }

    #[test]
    fn test_create_secondary_zone() {
        let (_temp_dir, manager) = setup();

        let primary_servers = vec!["10.0.1.1".into(), "10.0.1.2".into()];
        manager
            .create_secondary_zone("example.com", &primary_servers)
            .unwrap();

        let path = Path::new(&manager.zones_dir()).join("db.example.com.secondary");
        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("Secondary zone for example.com"));
        assert!(content.contains("10.0.1.1; 10.0.1.2"));
        assert!(content.contains("Primary servers"));
    }

    #[test]
    fn test_add_a_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("www 300  IN  A  192.0.2.1"));
    }

    #[test]
    fn test_add_a_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_a_record("example.com", "mail", "192.0.2.2", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("mail  IN  A  192.0.2.2"));
    }

    #[test]
    fn test_add_aaaa_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_aaaa_record("example.com", "www", "2001:db8::1", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("www 300  IN  AAAA  2001:db8::1"));
    }

    #[test]
    fn test_add_aaaa_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_aaaa_record("example.com", "mail", "2001:db8::2", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("mail  IN  AAAA  2001:db8::2"));
    }

    #[test]
    fn test_add_cname_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_cname_record("example.com", "blog", "www.example.com.", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("blog 300  IN  CNAME  www.example.com."));
    }

    #[test]
    fn test_add_cname_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_cname_record("example.com", "ftp", "www.example.com.", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("ftp  IN  CNAME  www.example.com."));
    }

    #[test]
    fn test_add_txt_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec!["v=spf1 mx ~all".into()];
        manager
            .add_txt_record("example.com", "@", &texts, Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  TXT  \"v=spf1 mx ~all\""));
    }

    #[test]
    fn test_add_txt_record_multiple_values() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec!["part1".into(), "part2".into()];
        manager
            .add_txt_record("example.com", "_dmarc", &texts, None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("_dmarc  IN  TXT  \"part1\" \"part2\""));
    }

    #[test]
    fn test_add_mx_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  MX  10 mail.example.com."));
    }

    #[test]
    fn test_add_mx_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_mx_record("example.com", "@", 20, "mail2.example.com.", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@  IN  MX  20 mail2.example.com."));
    }

    #[test]
    fn test_add_ns_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_ns_record("example.com", "@", "ns2.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  NS  ns2.example.com."));
    }

    #[test]
    fn test_add_ns_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_ns_record("example.com", "subdomain", "ns3.example.com.", None)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("subdomain  IN  NS  ns3.example.com."));
    }

    #[test]
    fn test_add_srv_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sipserver.example.com.".into(),
            ttl: Some(3600),
        };
        manager
            .add_srv_record("example.com", "_sip._tcp", &srv_data)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("_sip._tcp 3600  IN  SRV  10 60 5060 sipserver.example.com."));
    }

    #[test]
    fn test_add_srv_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let srv_data = SRVRecordData {
            priority: 0,
            weight: 100,
            port: 389,
            target: "ldap.example.com.".into(),
            ttl: None,
        };
        manager
            .add_srv_record("example.com", "_ldap._tcp", &srv_data)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("_ldap._tcp  IN  SRV  0 100 389 ldap.example.com."));
    }

    #[test]
    fn test_add_caa_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issue",
                "letsencrypt.org",
                Some(3600),
            )
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@ 3600  IN  CAA  0 issue \"letsencrypt.org\""));
    }

    #[test]
    fn test_add_caa_record_without_ttl() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_caa_record(
                "example.com",
                "@",
                128,
                "iodef",
                "mailto:admin@example.com",
                None,
            )
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();
        assert!(content.contains("@  IN  CAA  128 iodef \"mailto:admin@example.com\""));
    }

    #[test]
    fn test_delete_zone() {
        let (_temp_dir, manager) = setup();
        create_test_zone(&manager);

        assert!(manager.zone_exists("example.com"));

        manager.delete_zone("example.com").unwrap();

        assert!(!manager.zone_exists("example.com"));
    }

    #[test]
    fn test_delete_zone_nonexistent() {
        let (_temp_dir, manager) = setup();

        // Should not error when deleting non-existent zone
        manager.delete_zone("nonexistent.com").unwrap();
    }

    #[test]
    fn test_zone_exists_false() {
        let (_temp_dir, manager) = setup();

        assert!(!manager.zone_exists("nonexistent.com"));
    }

    #[test]
    fn test_multiple_records() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Add multiple different record types
        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_aaaa_record("example.com", "www", "2001:db8::1", Some(300))
            .unwrap();
        manager
            .add_cname_record("example.com", "blog", "www.example.com.", Some(300))
            .unwrap();
        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("www 300  IN  A  192.0.2.1"));
        assert!(content.contains("www 300  IN  AAAA  2001:db8::1"));
        assert!(content.contains("blog 300  IN  CNAME  www.example.com."));
        assert!(content.contains("@ 3600  IN  MX  10 mail.example.com."));
    }

    // =====================================================
    // Additional Comprehensive Tests
    // =====================================================

    #[test]
    fn test_zone_name_with_underscores() {
        let (_temp_dir, manager) = setup();
        let soa = SOARecord {
            primary_ns: "ns1._example.com.".into(),
            admin_email: "admin@_example.com".into(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let result = manager.create_zone_file("_example.com", &soa, 3600);
        assert!(result.is_ok());
        assert!(manager.zone_exists("_example.com"));
    }

    #[test]
    fn test_zone_name_with_hyphens() {
        let (_temp_dir, manager) = setup();
        let soa = SOARecord {
            primary_ns: "ns1.my-example.com.".into(),
            admin_email: "admin@my-example.com".into(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let result = manager.create_zone_file("my-example.com", &soa, 3600);
        assert!(result.is_ok());
        assert!(manager.zone_exists("my-example.com"));
    }

    #[test]
    fn test_add_multiple_a_records() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_a_record("example.com", "www", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "mail", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "ftp", "192.0.2.3", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("www 300  IN  A  192.0.2.1"));
        assert!(content.contains("mail 300  IN  A  192.0.2.2"));
        assert!(content.contains("ftp 300  IN  A  192.0.2.3"));
    }

    #[test]
    fn test_special_characters_in_txt_record() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec!["v=spf1 include:_spf.google.com ~all".into()];
        manager
            .add_txt_record("example.com", "@", &texts, Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("v=spf1 include:_spf.google.com ~all"));
    }

    #[test]
    fn test_multiple_txt_record_strings() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let texts = vec!["part1".into(), "part2".into(), "part3".into()];
        manager
            .add_txt_record("example.com", "_dmarc", &texts, Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("_dmarc 3600  IN  TXT  \"part1\" \"part2\" \"part3\""));
    }

    #[test]
    fn test_large_serial_number() {
        let (_temp_dir, manager) = setup();
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 9999999999i64,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        manager.create_zone_file("example.com", &soa, 3600).unwrap();

        let content =
            fs::read_to_string(Path::new(&manager.zones_dir()).join("db.example.com")).unwrap();

        assert!(content.contains("9999999999"));
    }

    #[test]
    fn test_different_email_formats() {
        let (_temp_dir, manager) = setup();

        let soa1 = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        let soa2 = SOARecord {
            primary_ns: "ns1.example.org.".into(),
            admin_email: "hostmaster@example.org".into(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        manager
            .create_zone_file("example1.com", &soa1, 3600)
            .unwrap();
        manager
            .create_zone_file("example2.org", &soa2, 3600)
            .unwrap();

        let content1 =
            fs::read_to_string(Path::new(&manager.zones_dir()).join("db.example1.com")).unwrap();
        let content2 =
            fs::read_to_string(Path::new(&manager.zones_dir()).join("db.example2.org")).unwrap();

        assert!(content1.contains("admin.example.com"));
        assert!(content2.contains("hostmaster.example.org"));
    }

    #[test]
    fn test_custom_ttl_values() {
        let (temp_dir, manager) = setup();

        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        manager.create_zone_file("example.com", &soa, 7200).unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("$TTL 7200"));
    }

    #[test]
    fn test_mx_record_priority_order() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Add MX records with different priorities
        manager
            .add_mx_record("example.com", "@", 10, "mail1.example.com.", Some(3600))
            .unwrap();
        manager
            .add_mx_record("example.com", "@", 20, "mail2.example.com.", Some(3600))
            .unwrap();
        manager
            .add_mx_record("example.com", "@", 30, "mail3.example.com.", Some(3600))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("@ 3600  IN  MX  10 mail1.example.com."));
        assert!(content.contains("@ 3600  IN  MX  20 mail2.example.com."));
        assert!(content.contains("@ 3600  IN  MX  30 mail3.example.com."));
    }

    #[test]
    fn test_srv_record_parameters() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sipserver.example.com.".into(),
            ttl: Some(3600),
        };
        manager
            .add_srv_record("example.com", "_sip._tcp", &srv_data)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("_sip._tcp 3600  IN  SRV  10 60 5060 sipserver.example.com."));
    }

    #[test]
    fn test_caa_record_flags_and_tag() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Issue CAA record
        manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issue",
                "letsencrypt.org",
                Some(3600),
            )
            .unwrap();
        // Wildcard issue record
        manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issuewild",
                "letsencrypt.org",
                Some(3600),
            )
            .unwrap();
        // IODEF record
        manager
            .add_caa_record(
                "example.com",
                "@",
                128,
                "iodef",
                "mailto:admin@example.com",
                Some(3600),
            )
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("@ 3600  IN  CAA  0 issue \"letsencrypt.org\""));
        assert!(content.contains("@ 3600  IN  CAA  0 issuewild \"letsencrypt.org\""));
        assert!(content.contains("@ 3600  IN  CAA  128 iodef \"mailto:admin@example.com\""));
    }

    #[test]
    fn test_complete_zone_workflow() {
        let (_temp_dir, manager) = setup();

        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 2024010101,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };

        // Step 1: Create zone
        manager.create_zone_file("example.com", &soa, 3600).unwrap();
        assert!(manager.zone_exists("example.com"));

        // Step 2: Add apex records
        manager
            .add_a_record("example.com", "@", "192.0.2.1", Some(300))
            .unwrap();
        manager
            .add_aaaa_record("example.com", "@", "2001:db8::1", Some(300))
            .unwrap();
        manager
            .add_ns_record("example.com", "@", "ns1.example.com.", Some(3600))
            .unwrap();
        manager
            .add_ns_record("example.com", "@", "ns2.example.com.", Some(3600))
            .unwrap();

        // Step 3: Add subdomain records
        manager
            .add_a_record("example.com", "www", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "mail", "192.0.2.3", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "ftp", "192.0.2.4", Some(300))
            .unwrap();

        // Step 4: Add service records
        manager
            .add_mx_record("example.com", "@", 10, "mail.example.com.", Some(3600))
            .unwrap();
        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sip.example.com.".into(),
            ttl: Some(3600),
        };
        manager
            .add_srv_record("example.com", "_sip._tcp", &srv_data)
            .unwrap();

        // Step 5: Add TXT records
        let spf = vec!["v=spf1 mx ~all".into()];
        manager
            .add_txt_record("example.com", "@", &spf, Some(3600))
            .unwrap();

        // Step 6: Verify zone still exists
        assert!(manager.zone_exists("example.com"));

        // Step 7: Delete zone
        manager.delete_zone("example.com").unwrap();
        assert!(!manager.zone_exists("example.com"));
    }

    #[test]
    fn test_primary_and_secondary_zones_together() {
        let (_temp_dir, manager) = setup();

        // Create primary zone
        let soa = SOARecord {
            primary_ns: "ns1.example.com.".into(),
            admin_email: "admin@example.com".into(),
            serial: 1,
            refresh: 3600,
            retry: 600,
            expire: 604800,
            negative_ttl: 86400,
        };
        manager.create_zone_file("primary.com", &soa, 3600).unwrap();

        // Create secondary zone
        let primary_servers = vec!["10.0.1.1".into(), "10.0.1.2".into()];
        manager
            .create_secondary_zone("secondary.com", &primary_servers)
            .unwrap();

        assert!(manager.zone_exists("primary.com"));
        // Note: Secondary zones are stored with .secondary extension, so they don't match zone_exists()
        // which looks for "db.{zone_name}", but we can verify the file exists separately
        let secondary_path = Path::new(&manager.zones_dir()).join("db.secondary.com.secondary");
        assert!(secondary_path.exists());
    }

    #[test]
    fn test_secondary_zone_with_multiple_primaries() {
        let (_temp_dir, manager) = setup();

        let primary_servers = vec![
            "10.0.1.1".into(),
            "10.0.1.2".into(),
            "10.0.1.3".into(),
            "10.0.1.4".into(),
        ];
        manager
            .create_secondary_zone("secondary.com", &primary_servers)
            .unwrap();

        let content =
            fs::read_to_string(Path::new(&manager.zones_dir()).join("db.secondary.com.secondary"))
                .unwrap();

        for server in primary_servers {
            assert!(content.contains(&server));
        }
    }

    #[test]
    fn test_nameserver_records_at_subdomain() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        manager
            .add_ns_record(
                "example.com",
                "subdomain",
                "ns1.subdomain.example.com.",
                Some(3600),
            )
            .unwrap();
        manager
            .add_ns_record(
                "example.com",
                "subdomain",
                "ns2.subdomain.example.com.",
                Some(3600),
            )
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("subdomain 3600  IN  NS  ns1.subdomain.example.com."));
        assert!(content.contains("subdomain 3600  IN  NS  ns2.subdomain.example.com."));
    }

    #[test]
    fn test_zone_apex_vs_subdomain() {
        let (temp_dir, manager) = setup();
        create_test_zone(&manager);

        // Apex records
        manager
            .add_a_record("example.com", "@", "192.0.2.1", Some(300))
            .unwrap();
        // Subdomain records
        manager
            .add_a_record("example.com", "www", "192.0.2.2", Some(300))
            .unwrap();
        manager
            .add_a_record("example.com", "api", "192.0.2.3", Some(300))
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("db.example.com")).unwrap();

        assert!(content.contains("@ 300  IN  A  192.0.2.1"));
        assert!(content.contains("www 300  IN  A  192.0.2.2"));
        assert!(content.contains("api 300  IN  A  192.0.2.3"));
    }

    #[test]
    fn test_parse_rndc_key_file_all_algorithms() {
        use crate::crd::RndcAlgorithm;

        let algorithms = vec![
            ("hmac-md5", RndcAlgorithm::HmacMd5),
            ("hmac-sha1", RndcAlgorithm::HmacSha1),
            ("hmac-sha224", RndcAlgorithm::HmacSha224),
            ("hmac-sha256", RndcAlgorithm::HmacSha256),
            ("hmac-sha384", RndcAlgorithm::HmacSha384),
            ("hmac-sha512", RndcAlgorithm::HmacSha512),
        ];

        for (algo_str, expected_algo) in algorithms {
            let rndc_key_content = format!(
                r#"key "test-key" {{
    algorithm {};
    secret "dGVzdHNlY3JldA==";
}};
"#,
                algo_str
            );
            let mut data = BTreeMap::new();
            data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

            let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

            assert_eq!(key.algorithm, expected_algo);
            assert_eq!(key.name, "test-key");
            assert_eq!(key.secret, "dGVzdHNlY3JldA==");
        }
    }

    #[test]
    fn test_parse_rndc_key_file_invalid_format_no_key() {
        let rndc_key_content = r#"algorithm hmac-sha256;
secret "dGVzdHNlY3JldA==";
"#;
        let mut data = BTreeMap::new();
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("key name"));
    }

    #[test]
    fn test_parse_rndc_key_file_invalid_format_no_algorithm() {
        let rndc_key_content = r#"key "test-key" {
    secret "dGVzdHNlY3JldA==";
};
"#;
        let mut data = BTreeMap::new();
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("algorithm"));
    }

    #[test]
    fn test_parse_rndc_key_file_invalid_format_no_secret() {
        let rndc_key_content = r#"key "test-key" {
    algorithm hmac-sha256;
};
"#;
        let mut data = BTreeMap::new();
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("secret"));
    }

    #[test]
    fn test_parse_rndc_key_file_unsupported_algorithm() {
        let rndc_key_content = r#"key "test-key" {
    algorithm hmac-unsupported;
    secret "dGVzdHNlY3JldA==";
};
"#;
        let mut data = BTreeMap::new();
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let result = Bind9Manager::parse_rndc_secret_data(&data);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unsupported algorithm"));
        assert!(err_msg.contains("hmac-unsupported"));
    }

    #[test]
    fn test_parse_rndc_key_file_with_comments() {
        let rndc_key_content = r#"# This is a comment
key "bindy-operator" {
    # Algorithm comment
    algorithm hmac-sha256;
    # Secret comment
    secret "dGVzdHNlY3JldA==";
};
"#;
        let mut data = BTreeMap::new();
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        assert_eq!(key.name, "bindy-operator");
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);
        assert_eq!(key.secret, "dGVzdHNlY3JldA==");
    }

    #[test]
    fn test_parse_rndc_key_file_compact_format() {
        // Test parsing with minimal whitespace
        let rndc_key_content = r#"key "my-key" { algorithm hmac-sha256; secret "c2VjcmV0"; };"#;
        let mut data = BTreeMap::new();
        data.insert("rndc.key".to_string(), rndc_key_content.as_bytes().to_vec());

        let key = Bind9Manager::parse_rndc_secret_data(&data).unwrap();

        assert_eq!(key.name, "my-key");
        assert_eq!(key.algorithm, crate::crd::RndcAlgorithm::HmacSha256);
        assert_eq!(key.secret, "c2VjcmV0");
    }

    #[test]
    fn test_rndc_error_parse_zone_not_found() {
        let response =
            "rndc: 'zonestatus' failed: not found\nno matching zone 'example.com' in any view";
        let error = RndcError::parse(response).expect("Should parse error");

        assert_eq!(error.command, "zonestatus");
        assert_eq!(error.error, "not found");
        assert_eq!(
            error.details.as_deref(),
            Some("no matching zone 'example.com' in any view")
        );
    }

    #[test]
    fn test_rndc_error_parse_addzone_already_exists() {
        let response =
            "rndc: 'addzone' failed: already exists\nzone 'example.com' already exists in view '_default'";
        let error = RndcError::parse(response).expect("Should parse error");

        assert_eq!(error.command, "addzone");
        assert_eq!(error.error, "already exists");
        assert!(error.details.is_some());
        assert!(error
            .details
            .unwrap()
            .contains("zone 'example.com' already exists"));
    }

    #[test]
    fn test_rndc_error_parse_without_details() {
        let response = "rndc: 'reload' failed: permission denied";
        let error = RndcError::parse(response).expect("Should parse error");

        assert_eq!(error.command, "reload");
        assert_eq!(error.error, "permission denied");
        assert_eq!(error.details, None);
    }

    #[test]
    fn test_rndc_error_parse_multiline_details() {
        let response = r#"rndc: 'delzone' failed: zone not found
zone 'example.com' was not found
check your zone configuration
verify the zone name is correct"#;
        let error = RndcError::parse(response).expect("Should parse error");

        assert_eq!(error.command, "delzone");
        assert_eq!(error.error, "zone not found");
        let details = error.details.unwrap();
        assert!(details.contains("zone 'example.com' was not found"));
        assert!(details.contains("check your zone configuration"));
        assert!(details.contains("verify the zone name is correct"));
    }

    #[test]
    fn test_rndc_error_parse_invalid_format() {
        let response = "This is not an RNDC error";
        let error = RndcError::parse(response);
        assert!(error.is_none());
    }

    #[test]
    fn test_rndc_error_parse_missing_command() {
        let response = "rndc: failed: some error";
        let error = RndcError::parse(response);
        assert!(error.is_none());
    }

    #[test]
    fn test_rndc_error_display() {
        let error = RndcError {
            command: "zonestatus".to_string(),
            error: "not found".to_string(),
            details: Some("no matching zone 'example.com' in any view".to_string()),
        };

        let display = format!("{error}");
        assert_eq!(display, "RNDC command 'zonestatus' failed: not found");
    }
}
