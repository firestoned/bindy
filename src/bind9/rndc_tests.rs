// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for RNDC key generation and parsing.

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use std::collections::BTreeMap;

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

    // ========================================================================
    // Rotation Annotation Tests
    // ========================================================================

    #[test]
    fn test_create_rndc_secret_with_annotations_no_rotation() {
        use chrono::Utc;

        let key_data = RndcKeyData {
            name: "test-instance".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdHNlY3JldA==".to_string(),
        };

        let created_at = Utc::now();
        let secret = crate::bind9::rndc::create_rndc_secret_with_annotations(
            "dns-system",
            "test-rndc-key",
            &key_data,
            created_at,
            None, // No rotation
            0,
        );

        // Verify basic Secret structure
        assert_eq!(secret.metadata.name, Some("test-rndc-key".to_string()));
        assert_eq!(secret.metadata.namespace, Some("dns-system".to_string()));

        // Verify annotations
        let annotations = secret.metadata.annotations.as_ref().unwrap();
        assert!(annotations.contains_key(crate::constants::ANNOTATION_RNDC_CREATED_AT));
        assert!(!annotations.contains_key(crate::constants::ANNOTATION_RNDC_ROTATE_AT)); // No rotation
        assert_eq!(
            annotations.get(crate::constants::ANNOTATION_RNDC_ROTATION_COUNT),
            Some(&"0".to_string())
        );

        // Verify Secret data contains RNDC key
        let data = secret.data.as_ref().unwrap();
        assert!(data.contains_key("key-name"));
        assert!(data.contains_key("algorithm"));
        assert!(data.contains_key("secret"));
        assert!(data.contains_key("rndc.key"));
    }

    #[test]
    fn test_create_rndc_secret_with_annotations_with_rotation() {
        use chrono::{DateTime, Utc};
        use std::time::Duration;

        let key_data = RndcKeyData {
            name: "test-instance".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdHNlY3JldA==".to_string(),
        };

        let created_at = Utc::now();
        let rotate_after = Duration::from_secs(30 * 24 * 3600); // 30 days
        let secret = crate::bind9::rndc::create_rndc_secret_with_annotations(
            "dns-system",
            "test-rndc-key",
            &key_data,
            created_at,
            Some(rotate_after),
            5, // Fifth rotation
        );

        // Verify annotations
        let annotations = secret.metadata.annotations.as_ref().unwrap();
        assert!(annotations.contains_key(crate::constants::ANNOTATION_RNDC_CREATED_AT));
        assert!(annotations.contains_key(crate::constants::ANNOTATION_RNDC_ROTATE_AT)); // Rotation enabled
        assert_eq!(
            annotations.get(crate::constants::ANNOTATION_RNDC_ROTATION_COUNT),
            Some(&"5".to_string())
        );

        // Verify rotate_at is in the future (created_at + 30 days)
        let created_str = annotations
            .get(crate::constants::ANNOTATION_RNDC_CREATED_AT)
            .unwrap();
        let rotate_str = annotations
            .get(crate::constants::ANNOTATION_RNDC_ROTATE_AT)
            .unwrap();

        let created_parsed: DateTime<Utc> = created_str.parse().unwrap();
        let rotate_parsed: DateTime<Utc> = rotate_str.parse().unwrap();

        // rotate_at should be exactly 30 days after created_at
        let expected_rotate_at = created_parsed + chrono::Duration::seconds(30 * 24 * 3600);
        assert_eq!(rotate_parsed, expected_rotate_at);
    }

    #[test]
    fn test_parse_rotation_annotations_complete() {
        use chrono::Utc;
        use std::collections::BTreeMap;

        let created_at = Utc::now();
        let rotate_at = created_at + chrono::Duration::days(30);

        let mut annotations = BTreeMap::new();
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
            created_at.to_rfc3339(),
        );
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATE_AT.to_string(),
            rotate_at.to_rfc3339(),
        );
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATION_COUNT.to_string(),
            "3".to_string(),
        );

        let result = crate::bind9::rndc::parse_rotation_annotations(&annotations);
        assert!(result.is_ok());

        let (parsed_created, parsed_rotate, count) = result.unwrap();
        assert_eq!(parsed_created.to_rfc3339(), created_at.to_rfc3339());
        assert_eq!(parsed_rotate.unwrap().to_rfc3339(), rotate_at.to_rfc3339());
        assert_eq!(count, 3);
    }

    #[test]
    fn test_parse_rotation_annotations_no_rotation() {
        use chrono::Utc;
        use std::collections::BTreeMap;

        let created_at = Utc::now();

        let mut annotations = BTreeMap::new();
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
            created_at.to_rfc3339(),
        );
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATION_COUNT.to_string(),
            "0".to_string(),
        );
        // No rotate_at annotation

        let result = crate::bind9::rndc::parse_rotation_annotations(&annotations);
        assert!(result.is_ok());

        let (parsed_created, parsed_rotate, count) = result.unwrap();
        assert_eq!(parsed_created.to_rfc3339(), created_at.to_rfc3339());
        assert!(parsed_rotate.is_none());
        assert_eq!(count, 0);
    }

    #[test]
    fn test_parse_rotation_annotations_missing_created_at() {
        use std::collections::BTreeMap;

        let annotations = BTreeMap::new();

        let result = crate::bind9::rndc::parse_rotation_annotations(&annotations);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("created-at annotation"));
    }

    #[test]
    fn test_parse_rotation_annotations_invalid_timestamp() {
        use std::collections::BTreeMap;

        let mut annotations = BTreeMap::new();
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
            "not-a-valid-timestamp".to_string(),
        );

        let result = crate::bind9::rndc::parse_rotation_annotations(&annotations);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_rotation_annotations_invalid_count() {
        use chrono::Utc;
        use std::collections::BTreeMap;

        let mut annotations = BTreeMap::new();
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
            Utc::now().to_rfc3339(),
        );
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATION_COUNT.to_string(),
            "not-a-number".to_string(),
        );

        let result = crate::bind9::rndc::parse_rotation_annotations(&annotations);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_rotation_due_past_timestamp() {
        use chrono::Utc;

        let past_time = Utc::now() - chrono::Duration::hours(1);
        let now = Utc::now();

        assert!(crate::bind9::rndc::is_rotation_due(Some(past_time), now));
    }

    #[test]
    fn test_is_rotation_due_future_timestamp() {
        use chrono::Utc;

        let future_time = Utc::now() + chrono::Duration::hours(1);
        let now = Utc::now();

        assert!(!crate::bind9::rndc::is_rotation_due(Some(future_time), now));
    }

    #[test]
    fn test_is_rotation_due_no_rotate_at() {
        use chrono::Utc;

        let now = Utc::now();

        // No rotate_at means rotation is disabled
        assert!(!crate::bind9::rndc::is_rotation_due(None, now));
    }

    #[test]
    fn test_is_rotation_due_exact_time() {
        use chrono::Utc;

        let now = Utc::now();

        // Rotation at exactly current time should be considered due
        assert!(crate::bind9::rndc::is_rotation_due(Some(now), now));
    }
}
