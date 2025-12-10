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
    let mut rng = rand::thread_rng();
    let mut key_bytes = [0u8; 32]; // 256 bits for HMAC-SHA256
    rng.fill(&mut key_bytes);

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

#[cfg(test)]
#[path = "rndc_tests.rs"]
mod rndc_tests;
