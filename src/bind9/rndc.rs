// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! RNDC key generation and management functions.

use super::types::RndcKeyData;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hickory_client::rr::rdata::tsig::TsigAlgorithm;
use hickory_client::rr::Name;
use hickory_proto::rr::dnssec::tsig::TSigner;
use rand::Rng;
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::constants::TSIG_FUDGE_TIME_SECS;

/// Generate a new RNDC key with HMAC-SHA256.
///
/// Returns a base64-encoded 256-bit (32-byte) key suitable for rndc authentication.
#[must_use]
pub fn generate_rndc_key() -> RndcKeyData {
    let mut rng = rand::rng();
    let mut key_bytes = [0u8; 32]; // 256 bits for HMAC-SHA256
    rng.fill_bytes(&mut key_bytes);

    RndcKeyData {
        name: String::new(), // Will be set by caller
        algorithm: crate::crd::RndcAlgorithm::HmacSha256,
        secret: BASE64.encode(key_bytes),
    }
}

/// Create a Kubernetes Secret manifest for an RNDC key.
///
/// Returns a `BTreeMap` suitable for use as Secret data.
#[must_use]
pub fn create_rndc_secret_data(key_data: &RndcKeyData) -> BTreeMap<String, String> {
    let mut data = BTreeMap::new();
    data.insert("key-name".to_string(), key_data.name.clone());
    data.insert(
        "algorithm".to_string(),
        key_data.algorithm.as_str().to_string(),
    );
    data.insert("secret".to_string(), key_data.secret.clone());

    // Add rndc.key file content for BIND9 to use
    let rndc_key_content = format!(
        "key \"{}\" {{\n    algorithm {};\n    secret \"{}\";\n}};\n",
        key_data.name,
        key_data.algorithm.as_str(),
        key_data.secret
    );
    data.insert("rndc.key".to_string(), rndc_key_content);

    data
}

/// Parse RNDC key data from a Kubernetes Secret.
///
/// Supports two Secret formats:
/// 1. **Operator-generated** (all 4 fields): `key-name`, `algorithm`, `secret`, `rndc.key`
/// 2. **External/user-managed** (minimal): `rndc.key` only - parses the BIND9 key file
///
/// # Errors
///
/// Returns an error if:
/// - Neither the metadata fields nor `rndc.key` are present
/// - The `rndc.key` file cannot be parsed
/// - Values are not valid UTF-8 strings
pub fn parse_rndc_secret_data(data: &BTreeMap<String, Vec<u8>>) -> Result<RndcKeyData> {
    // Try the operator-generated format first (has all metadata fields)
    if let (Some(name_bytes), Some(algo_bytes), Some(secret_bytes)) = (
        data.get("key-name"),
        data.get("algorithm"),
        data.get("secret"),
    ) {
        let name = std::str::from_utf8(name_bytes)?.to_string();
        let algorithm_str = std::str::from_utf8(algo_bytes)?;
        let secret = std::str::from_utf8(secret_bytes)?.to_string();

        let algorithm = match algorithm_str {
            "hmac-md5" => crate::crd::RndcAlgorithm::HmacMd5,
            "hmac-sha1" => crate::crd::RndcAlgorithm::HmacSha1,
            "hmac-sha224" => crate::crd::RndcAlgorithm::HmacSha224,
            "hmac-sha256" => crate::crd::RndcAlgorithm::HmacSha256,
            "hmac-sha384" => crate::crd::RndcAlgorithm::HmacSha384,
            "hmac-sha512" => crate::crd::RndcAlgorithm::HmacSha512,
            _ => anyhow::bail!("Unsupported RNDC algorithm '{algorithm_str}'. Supported algorithms: hmac-md5, hmac-sha1, hmac-sha224, hmac-sha256, hmac-sha384, hmac-sha512"),
        };

        return Ok(RndcKeyData {
            name,
            algorithm,
            secret,
        });
    }

    // Fall back to parsing the rndc.key file (external Secret format)
    if let Some(rndc_key_bytes) = data.get("rndc.key") {
        let rndc_key_content = std::str::from_utf8(rndc_key_bytes)?;
        return parse_rndc_key_file(rndc_key_content);
    }

    anyhow::bail!(
        "Secret must contain either (key-name, algorithm, secret) or rndc.key field. \
         For external secrets, provide only 'rndc.key' with the BIND9 key file content."
    )
}

/// Parse a BIND9 key file (rndc.key format) to extract key metadata.
///
/// Expected format:
/// ```text
/// key "key-name" {
///     algorithm hmac-sha256;
///     secret "base64secret==";
/// };
/// ```
///
/// # Errors
///
/// Returns an error if the file format is invalid or required fields are missing.
fn parse_rndc_key_file(content: &str) -> Result<RndcKeyData> {
    // Simple regex-based parser for BIND9 key file format
    // Format: key "name" { algorithm algo; secret "secret"; };

    // Extract key name
    let name = content
        .lines()
        .find(|line| line.contains("key"))
        .and_then(|line| {
            line.split('"').nth(1) // Get the text between first pair of quotes
        })
        .context("Failed to parse key name from rndc.key file")?
        .to_string();

    // Extract algorithm
    let algorithm_str = content
        .lines()
        .find(|line| line.contains("algorithm"))
        .and_then(|line| {
            line.split_whitespace()
                .nth(1) // After "algorithm"
                .map(|s| s.trim_end_matches(';'))
        })
        .context("Failed to parse algorithm from rndc.key file")?;

    let algorithm = match algorithm_str {
        "hmac-md5" => crate::crd::RndcAlgorithm::HmacMd5,
        "hmac-sha1" => crate::crd::RndcAlgorithm::HmacSha1,
        "hmac-sha224" => crate::crd::RndcAlgorithm::HmacSha224,
        "hmac-sha256" => crate::crd::RndcAlgorithm::HmacSha256,
        "hmac-sha384" => crate::crd::RndcAlgorithm::HmacSha384,
        "hmac-sha512" => crate::crd::RndcAlgorithm::HmacSha512,
        _ => anyhow::bail!("Unsupported algorithm '{algorithm_str}' in rndc.key file"),
    };

    // Extract secret
    let secret = content
        .lines()
        .find(|line| line.contains("secret"))
        .and_then(|line| {
            line.split('"').nth(1) // Get the text between first pair of quotes
        })
        .context("Failed to parse secret from rndc.key file")?
        .to_string();

    Ok(RndcKeyData {
        name,
        algorithm,
        secret,
    })
}

/// Create a TSIG signer from RNDC key data.
///
/// # Errors
///
/// Returns an error if the algorithm is unsupported or key data is invalid.
pub fn create_tsig_signer(key_data: &RndcKeyData) -> Result<TSigner> {
    // Map RndcAlgorithm to hickory TsigAlgorithm
    let algorithm = match key_data.algorithm {
        crate::crd::RndcAlgorithm::HmacMd5 => TsigAlgorithm::HmacMd5,
        crate::crd::RndcAlgorithm::HmacSha1 => TsigAlgorithm::HmacSha1,
        crate::crd::RndcAlgorithm::HmacSha224 => TsigAlgorithm::HmacSha224,
        crate::crd::RndcAlgorithm::HmacSha256 => TsigAlgorithm::HmacSha256,
        crate::crd::RndcAlgorithm::HmacSha384 => TsigAlgorithm::HmacSha384,
        crate::crd::RndcAlgorithm::HmacSha512 => TsigAlgorithm::HmacSha512,
    };

    // Decode the base64 key
    let key_bytes = BASE64
        .decode(&key_data.secret)
        .context("Failed to decode TSIG key")?;

    // Create TSIG signer
    let signer = TSigner::new(
        key_bytes,
        algorithm,
        Name::from_str(&key_data.name).context("Invalid TSIG key name")?,
        u16::try_from(TSIG_FUDGE_TIME_SECS).unwrap_or(300),
    )
    .context("Failed to create TSIG signer")?;

    Ok(signer)
}

/// Create a Kubernetes Secret with RNDC key data and rotation tracking annotations.
///
/// This function creates a Secret with the RNDC key data (via `create_rndc_secret_data`)
/// and adds rotation tracking annotations for automatic key rotation.
///
/// # Arguments
///
/// * `namespace` - Kubernetes namespace for the Secret
/// * `name` - Secret name
/// * `key_data` - RNDC key data (name, algorithm, secret)
/// * `created_at` - Timestamp when the key was created or last rotated
/// * `rotate_after` - Optional duration after which to rotate (None = no rotation)
/// * `rotation_count` - Number of times the key has been rotated (0 for new keys)
///
/// # Returns
///
/// A Kubernetes Secret resource with:
/// - RNDC key data in `.data`
/// - Rotation tracking annotations in `.metadata.annotations`
///
/// # Annotations
///
/// - `bindy.firestoned.io/rndc-created-at`: ISO 8601 timestamp (always present)
/// - `bindy.firestoned.io/rndc-rotate-at`: ISO 8601 timestamp (only if `rotate_after` is Some)
/// - `bindy.firestoned.io/rndc-rotation-count`: Number of rotations (always present)
///
/// # Examples
///
/// ```rust,no_run
/// use bindy::bind9::rndc::{generate_rndc_key, create_rndc_secret_with_annotations};
/// use chrono::Utc;
/// use std::time::Duration;
///
/// let key_data = generate_rndc_key();
/// let created_at = Utc::now();
/// let rotate_after = Duration::from_secs(30 * 24 * 3600); // 30 days
///
/// let secret = create_rndc_secret_with_annotations(
///     "dns-system",
///     "bind9-primary-rndc-key",
///     &key_data,
///     created_at,
///     Some(rotate_after),
///     0, // First key, not rotated yet
/// );
/// ```
///
/// # Panics
///
/// May panic if the `rotate_after` duration cannot be converted to a chrono Duration.
/// This should not happen for valid rotation intervals (1h - 8760h).
#[must_use]
pub fn create_rndc_secret_with_annotations(
    namespace: &str,
    name: &str,
    key_data: &RndcKeyData,
    created_at: chrono::DateTime<chrono::Utc>,
    rotate_after: Option<std::time::Duration>,
    rotation_count: u32,
) -> k8s_openapi::api::core::v1::Secret {
    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::ByteString;

    // Create Secret data with RNDC key
    let secret_data_map = create_rndc_secret_data(key_data);
    let mut data = BTreeMap::new();
    for (k, v) in secret_data_map {
        data.insert(k, ByteString(v.into_bytes()));
    }

    // Create rotation tracking annotations
    let mut annotations = BTreeMap::new();
    annotations.insert(
        crate::constants::ANNOTATION_RNDC_CREATED_AT.to_string(),
        created_at.to_rfc3339(),
    );

    // Add rotate_at annotation if rotation is enabled
    if let Some(duration) = rotate_after {
        let rotate_at = created_at + chrono::Duration::from_std(duration).unwrap();
        annotations.insert(
            crate::constants::ANNOTATION_RNDC_ROTATE_AT.to_string(),
            rotate_at.to_rfc3339(),
        );
    }

    annotations.insert(
        crate::constants::ANNOTATION_RNDC_ROTATION_COUNT.to_string(),
        rotation_count.to_string(),
    );

    Secret {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            annotations: Some(annotations),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    }
}

/// Parse rotation tracking annotations from a Kubernetes Secret.
///
/// Extracts the `created_at`, `rotate_at`, and `rotation_count` annotations
/// from a Secret's metadata.
///
/// # Arguments
///
/// * `annotations` - Secret annotations map
///
/// # Returns
///
/// A tuple of:
/// - `created_at`: Timestamp when the key was created or last rotated
/// - `rotate_at`: Optional timestamp when rotation is due (None if rotation disabled)
/// - `rotation_count`: Number of times the key has been rotated
///
/// # Errors
///
/// Returns an error if:
/// - The `created-at` annotation is missing
/// - Any timestamp cannot be parsed as ISO 8601
/// - The `rotation-count` annotation cannot be parsed as u32
///
/// # Examples
///
/// ```rust,no_run
/// use std::collections::BTreeMap;
/// use bindy::bind9::rndc::parse_rotation_annotations;
///
/// let mut annotations = BTreeMap::new();
/// annotations.insert(
///     "bindy.firestoned.io/rndc-created-at".to_string(),
///     "2025-01-26T10:00:00Z".to_string()
/// );
/// annotations.insert(
///     "bindy.firestoned.io/rndc-rotate-at".to_string(),
///     "2025-02-25T10:00:00Z".to_string()
/// );
/// annotations.insert(
///     "bindy.firestoned.io/rndc-rotation-count".to_string(),
///     "5".to_string()
/// );
///
/// let (created_at, rotate_at, count) = parse_rotation_annotations(&annotations).unwrap();
/// assert_eq!(count, 5);
/// ```
pub fn parse_rotation_annotations(
    annotations: &BTreeMap<String, String>,
) -> Result<(
    chrono::DateTime<chrono::Utc>,
    Option<chrono::DateTime<chrono::Utc>>,
    u32,
)> {
    // Parse created_at (required)
    let created_at_str = annotations
        .get(crate::constants::ANNOTATION_RNDC_CREATED_AT)
        .context("Missing created-at annotation")?;
    let created_at = chrono::DateTime::parse_from_rfc3339(created_at_str)
        .context("Failed to parse created-at timestamp")?
        .with_timezone(&chrono::Utc);

    // Parse rotate_at (optional)
    let rotate_at =
        if let Some(rotate_at_str) = annotations.get(crate::constants::ANNOTATION_RNDC_ROTATE_AT) {
            Some(
                chrono::DateTime::parse_from_rfc3339(rotate_at_str)
                    .context("Failed to parse rotate-at timestamp")?
                    .with_timezone(&chrono::Utc),
            )
        } else {
            None
        };

    // Parse rotation_count (default to 0 if missing)
    let rotation_count = annotations
        .get(crate::constants::ANNOTATION_RNDC_ROTATION_COUNT)
        .map(|s| s.parse::<u32>().context("Failed to parse rotation-count"))
        .transpose()?
        .unwrap_or(0);

    Ok((created_at, rotate_at, rotation_count))
}

/// Check if RNDC key rotation is due based on the rotation timestamp.
///
/// Rotation is due if:
/// - `rotate_at` is Some AND
/// - `rotate_at` is less than or equal to `now`
///
/// # Arguments
///
/// * `rotate_at` - Optional timestamp when rotation should occur (None = no rotation)
/// * `now` - Current timestamp
///
/// # Returns
///
/// - `true` if rotation is due (`rotate_at` has passed)
/// - `false` if rotation is not due or disabled (`rotate_at` is None)
///
/// # Examples
///
/// ```rust
/// use bindy::bind9::rndc::is_rotation_due;
/// use chrono::Utc;
///
/// let past_time = Utc::now() - chrono::Duration::hours(1);
/// let now = Utc::now();
///
/// assert!(is_rotation_due(Some(past_time), now)); // Rotation is due
///
/// let future_time = Utc::now() + chrono::Duration::hours(1);
/// assert!(!is_rotation_due(Some(future_time), now)); // Not due yet
///
/// assert!(!is_rotation_due(None, now)); // Rotation disabled
/// ```
#[must_use]
pub fn is_rotation_due(
    rotate_at: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    match rotate_at {
        Some(rotate_time) => rotate_time <= now,
        None => false, // No rotation scheduled
    }
}

#[cfg(test)]
#[path = "rndc_tests.rs"]
mod rndc_tests;
