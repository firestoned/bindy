// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Semantic comparison of the pod-template fields bindy owns.
//!
//! The API server defaults and canonicalises a Deployment when it stores it:
//! an absent `resources` comes back as `{}`, `0.5` CPU as `500m`, an
//! `optional: false` stays while an absent one stays absent, a `fieldRef`
//! gains `apiVersion: v1`, and so on. Comparing the stored object with the
//! one bindy renders by plain `!=` therefore reports a difference that no
//! patch can remove, and every reconcile then tries to roll the pods
//! (the v0.8.0-rc.6 rollout hot loop, ADR-0018 amendment of 2026-10-07).
//!
//! Each helper here treats "absent" and "what the API server stores for
//! absent" as equal, and names the defaulting it absorbs. Every helper is
//! pure.

use k8s_openapi::api::core::v1::{EnvVar, ResourceRequirements};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use std::collections::BTreeMap;

/// The API server's `imagePullPolicy` default for an image tagged `latest`
/// or not tagged at all.
const PULL_POLICY_ALWAYS: &str = "Always";

/// The API server's `imagePullPolicy` default for any other image.
const PULL_POLICY_IF_NOT_PRESENT: &str = "IfNotPresent";

/// The tag the API server assumes for an image with neither tag nor digest.
const IMPLICIT_IMAGE_TAG: &str = "latest";

/// The `apiVersion` the API server writes into a `fieldRef` that has none.
const FIELD_REF_DEFAULT_API_VERSION: &str = "v1";

/// Quantities are compared in nano-units: `n` is the smallest suffix.
const NANO_DECIMAL_EXPONENT: u32 = 9;

/// The radix of decimal quantity suffixes and of the `e` notation.
const DECIMAL_RADIX: i128 = 10;

/// The radix of binary quantity suffixes.
const BINARY_RADIX: i128 = 2;

/// Each binary suffix step (`Ki`, `Mi`, ...) multiplies by 2^10.
const BINARY_SUFFIX_BITS: u32 = 10;

/// Longest digit string parsed exactly; anything longer compares as text.
/// 30 digits times the largest multiplier (10^27 nanos for `E`) stays
/// within `i128`.
const MAX_QUANTITY_DIGITS: usize = 30;

/// Decimal suffixes and their power of ten.
const DECIMAL_SUFFIXES: &[(&str, i32)] = &[
    ("n", -9),
    ("u", -6),
    ("m", -3),
    ("", 0),
    ("k", 3),
    ("M", 6),
    ("G", 9),
    ("T", 12),
    ("P", 15),
    ("E", 18),
];

/// Binary suffixes and their number of [`BINARY_SUFFIX_BITS`] steps.
const BINARY_SUFFIXES: &[(&str, u32)] = &[
    ("Ki", 1),
    ("Mi", 2),
    ("Gi", 3),
    ("Ti", 4),
    ("Pi", 5),
    ("Ei", 6),
];

/// A quantity's suffix as a multiplier: `radix^exponent`, with a negative
/// decimal exponent meaning a division.
struct Multiplier {
    /// Powers of two (binary suffixes)
    binary_bits: u32,
    /// Power of ten (decimal suffixes and `e` notation)
    decimal_exponent: i32,
}

/// The multiplier a quantity suffix stands for, or `None` for an unknown one.
fn suffix_multiplier(suffix: &str) -> Option<Multiplier> {
    if let Some((_, steps)) = BINARY_SUFFIXES.iter().find(|(s, _)| *s == suffix) {
        return Some(Multiplier {
            binary_bits: steps * BINARY_SUFFIX_BITS,
            decimal_exponent: 0,
        });
    }
    if let Some((_, exponent)) = DECIMAL_SUFFIXES.iter().find(|(s, _)| *s == suffix) {
        return Some(Multiplier {
            binary_bits: 0,
            decimal_exponent: *exponent,
        });
    }
    let exponent = suffix.strip_prefix(['e', 'E'])?;
    Some(Multiplier {
        binary_bits: 0,
        decimal_exponent: exponent.parse().ok()?,
    })
}

/// The exact value of a Kubernetes quantity in nano-units, or `None` when it
/// does not parse or is not a whole number of nano-units.
fn quantity_nanos(quantity: &str) -> Option<i128> {
    let text = quantity.trim();
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let number_len = unsigned
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(unsigned.len());
    let (number, suffix) = unsigned.split_at(number_len);
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || digits.len() > MAX_QUANTITY_DIGITS {
        return None;
    }
    let mantissa: i128 = digits.parse().ok()?;
    let multiplier = suffix_multiplier(suffix)?;

    // value = mantissa * 2^bits * 10^(exponent + 9 - fraction digits)
    let fraction_digits = i32::try_from(fraction.len()).ok()?;
    let nano_exponent = i32::try_from(NANO_DECIMAL_EXPONENT).ok()?;
    let ten_exponent = multiplier.decimal_exponent + nano_exponent - fraction_digits;
    let scaled = mantissa.checked_mul(BINARY_RADIX.checked_pow(multiplier.binary_bits)?)?;
    let magnitude = if ten_exponent >= 0 {
        scaled.checked_mul(DECIMAL_RADIX.checked_pow(ten_exponent.unsigned_abs())?)?
    } else {
        let divisor = DECIMAL_RADIX.checked_pow(ten_exponent.unsigned_abs())?;
        if scaled % divisor != 0 {
            return None;
        }
        scaled / divisor
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// Whether two quantities are the same amount.
///
/// Absorbs the API server's canonicalisation of quantities: it stores `0.5`
/// as `500m`, `1000m` as `1`, `1024Mi` as `1Gi`. A quantity that does not
/// parse exactly is compared as text.
pub(super) fn quantities_equivalent(current: &Quantity, desired: &Quantity) -> bool {
    if current.0 == desired.0 {
        return true;
    }
    match (quantity_nanos(&current.0), quantity_nanos(&desired.0)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Whether two resource maps (`requests` or `limits`) hold the same amounts.
/// An absent map equals an empty one.
fn resource_maps_equivalent(
    current: Option<&BTreeMap<String, Quantity>>,
    desired: Option<&BTreeMap<String, Quantity>>,
) -> bool {
    let empty = BTreeMap::new();
    let current = current.unwrap_or(&empty);
    let desired = desired.unwrap_or(&empty);
    current.len() == desired.len()
        && current.iter().all(|(name, amount)| {
            desired
                .get(name)
                .is_some_and(|wanted| quantities_equivalent(amount, wanted))
        })
}

/// Whether a container's stored `resources` match the rendered ones.
///
/// Absorbs: the API server stores an absent `resources` as `{}` (which
/// deserializes to `Some` with every field `None`), keeps empty `requests`,
/// `limits` and `claims` as given, and canonicalises every quantity
/// ([`quantities_equivalent`]).
pub(super) fn resources_equivalent(
    current: Option<&ResourceRequirements>,
    desired: Option<&ResourceRequirements>,
) -> bool {
    let empty = ResourceRequirements::default();
    let current = current.unwrap_or(&empty);
    let desired = desired.unwrap_or(&empty);
    resource_maps_equivalent(current.requests.as_ref(), desired.requests.as_ref())
        && resource_maps_equivalent(current.limits.as_ref(), desired.limits.as_ref())
        && list_equivalent(current.claims.as_ref(), desired.claims.as_ref())
}

/// An env var with the API server's defaulting undone: an empty `value` is
/// dropped (it is `omitempty`, so `""` is stored as absent), `optional:
/// false` on a `secretKeyRef` / `configMapKeyRef` is dropped (false is the
/// default), and `fieldRef.apiVersion: v1` is dropped (the API server writes
/// it when absent).
fn normalized_env_var(var: &EnvVar) -> EnvVar {
    let mut var = var.clone();
    if var.value.as_deref() == Some("") {
        var.value = None;
    }
    let Some(source) = var.value_from.as_mut() else {
        return var;
    };
    if let Some(secret) = source.secret_key_ref.as_mut() {
        if secret.optional == Some(false) {
            secret.optional = None;
        }
    }
    if let Some(config_map) = source.config_map_key_ref.as_mut() {
        if config_map.optional == Some(false) {
            config_map.optional = None;
        }
    }
    if let Some(field) = source.field_ref.as_mut() {
        if field.api_version.as_deref() == Some(FIELD_REF_DEFAULT_API_VERSION) {
            field.api_version = None;
        }
    }
    var
}

/// Whether a container's stored `env` matches the rendered one, in order.
///
/// Absorbs: an absent list equals an empty one, and each variable is
/// compared after [`normalized_env_var`].
pub(super) fn env_equivalent(current: Option<&Vec<EnvVar>>, desired: Option<&Vec<EnvVar>>) -> bool {
    let current = current.map_or(&[][..], Vec::as_slice);
    let desired = desired.map_or(&[][..], Vec::as_slice);
    current.len() == desired.len()
        && current
            .iter()
            .zip(desired)
            .all(|(a, b)| normalized_env_var(a) == normalized_env_var(b))
}

/// The `imagePullPolicy` the API server writes when a container has none:
/// `Always` for an image tagged `latest` or with neither tag nor digest,
/// `IfNotPresent` otherwise.
fn default_pull_policy(image: &str) -> &'static str {
    if image.contains('@') {
        return PULL_POLICY_IF_NOT_PRESENT;
    }
    // A ':' before the last '/' is a registry port, not a tag.
    let last_segment = image.rsplit('/').next().unwrap_or(image);
    let tag = last_segment
        .split_once(':')
        .map_or(IMPLICIT_IMAGE_TAG, |(_, tag)| tag);
    if tag == IMPLICIT_IMAGE_TAG {
        return PULL_POLICY_ALWAYS;
    }
    PULL_POLICY_IF_NOT_PRESENT
}

/// Whether a container's stored `imagePullPolicy` matches the rendered one.
///
/// Absorbs: an absent policy is stored as the API server's default for the
/// container's image ([`default_pull_policy`]).
pub(super) fn pull_policy_equivalent(
    current: Option<&str>,
    desired: Option<&str>,
    image: Option<&str>,
) -> bool {
    let effective = |policy: Option<&str>| -> Option<String> {
        policy
            .map(str::to_string)
            .or_else(|| image.map(|i| default_pull_policy(i).to_string()))
    };
    effective(current) == effective(desired)
}

/// Whether two lists are equal, an absent list equalling an empty one.
///
/// Absorbs: a list rendered empty (or absent) that the API server stores
/// absent (or empty), as for `readinessGates`, `topologySpreadConstraints`
/// and `resources.claims`.
pub(super) fn list_equivalent<T: PartialEq>(
    current: Option<&Vec<T>>,
    desired: Option<&Vec<T>>,
) -> bool {
    current.map_or(&[][..], Vec::as_slice) == desired.map_or(&[][..], Vec::as_slice)
}

/// Whether two string maps are equal, an absent map equalling an empty one.
///
/// Absorbs: labels or annotations rendered empty (or absent) that the API
/// server stores absent (or empty).
pub(super) fn map_equivalent(
    current: Option<&BTreeMap<String, String>>,
    desired: Option<&BTreeMap<String, String>>,
) -> bool {
    let empty = BTreeMap::new();
    current.unwrap_or(&empty) == desired.unwrap_or(&empty)
}

#[cfg(test)]
#[path = "template_drift_tests.rs"]
mod template_drift_tests;
