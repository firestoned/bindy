// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! YAML output for the manifests bindy generates (CRDs, bootstrap resources).
//!
//! One serializer configuration for every caller, so generated files share a
//! style: long strings stay on one line (no `>-` folding), and no line ends in
//! whitespace.

use serde::Serialize;

/// Serialize `value` to YAML in bindy's manifest style.
///
/// Long single-line strings are kept on one line, and the indentation the
/// emitter writes on blank lines inside block scalars is removed. A line made
/// only of indentation reads back as an empty line, so the parsed value is
/// unchanged.
///
/// # Errors
/// Returns the serializer's error if `value` cannot be represented as YAML.
pub fn to_string<T: Serialize>(value: &T) -> Result<String, serde_saphyr::SerializeError> {
    let mut options = serde_saphyr::SerializerOptions::default();
    options.folded_wrap_chars = usize::MAX;
    let yaml = serde_saphyr::to_string_with_options(value, options)?;
    Ok(strip_blank_line_indentation(&yaml))
}

/// Remove the whitespace from lines that contain nothing else.
fn strip_blank_line_indentation(yaml: &str) -> String {
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        if body.trim().is_empty() {
            out.push_str(newline);
            continue;
        }
        out.push_str(line);
    }
    out
}
