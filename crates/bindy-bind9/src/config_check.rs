// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Refuse to publish a BIND9 configuration `named` cannot parse (ADR-0013
//! stage 2).
//!
//! Every `named.conf*` file a ConfigMap builder renders is parsed with hornet
//! and run through its validator before the ConfigMap is returned. A parse
//! error or an Error-severity finding fails the build, so the operator never
//! writes the ConfigMap and the BIND9 pods keep the last configuration that
//! was published (bug-177: a stray `}` reached `named`, which then crash-
//! looped on every pod). Warnings never block; they are returned for logging.

use std::collections::BTreeMap;

/// File-name prefix of the files checked; `rndc.conf` is a different grammar.
const NAMED_CONF_PREFIX: &str = "named.conf";

/// A rendered BIND9 configuration file hornet does not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file} is not valid BIND9 configuration: {detail}")]
pub struct InvalidBind9Config {
    /// The ConfigMap key, e.g. `named.conf.options`
    pub file: String,
    /// hornet's parse error, or its Error-severity findings joined by `; `
    pub detail: String,
}

/// Check every `named.conf*` file in a ConfigMap's data.
///
/// # Arguments
/// * `data` - The ConfigMap data: file name to file content
///
/// # Returns
/// The Warning- and Info-severity findings, each prefixed with its file name.
///
/// # Errors
/// Returns [`InvalidBind9Config`] for the first file that fails to parse or
/// has an Error-severity finding.
pub fn check_named_conf_files(
    data: &BTreeMap<String, String>,
) -> Result<Vec<String>, InvalidBind9Config> {
    let mut warnings = Vec::new();
    for (file, text) in data
        .iter()
        .filter(|(name, _)| name.starts_with(NAMED_CONF_PREFIX))
    {
        let conf = hornet_bind9::parse_named_conf(text).map_err(|e| InvalidBind9Config {
            file: file.clone(),
            detail: e.to_string(),
        })?;
        let (errors, others): (Vec<_>, Vec<_>) = hornet_bind9::validate_named_conf(&conf)
            .into_iter()
            .partition(|finding| finding.severity == hornet_bind9::Severity::Error);
        if !errors.is_empty() {
            return Err(InvalidBind9Config {
                file: file.clone(),
                detail: errors
                    .into_iter()
                    .map(|finding| finding.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            });
        }
        warnings.extend(
            others
                .into_iter()
                .map(|finding| format!("{file}: {}: {}", finding.severity, finding.message)),
        );
    }
    Ok(warnings)
}

/// The [`InvalidBind9Config`] anywhere in an error's chain, if there is one.
///
/// Reconcilers use it to report a refused configuration with
/// `ConfigurationInvalid` rather than a generic failure; callers may have
/// wrapped the error with context on the way up.
#[must_use]
pub fn find_invalid_config(error: &anyhow::Error) -> Option<&InvalidBind9Config> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<InvalidBind9Config>())
}

#[cfg(test)]
#[path = "config_check_tests.rs"]
mod config_check_tests;
