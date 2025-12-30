// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Label selector matching utilities.
//!
//! This module provides helper functions for matching Kubernetes label selectors
//! against resource labels. It supports both `matchLabels` and `matchExpressions`
//! as defined in the Kubernetes API.
//!
//! # Architecture
//!
//! The label selector watch pattern uses kube-rs's reflector/store to maintain
//! an in-memory cache of resources. When a resource changes, watch mappers
//! synchronously query these caches to find related resources using label selectors.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::collections::BTreeMap;
//! use bindy::crd::LabelSelector;
//! use bindy::selector::matches_selector;
//!
//! # fn example() {
//! let mut labels = BTreeMap::new();
//! labels.insert("app".to_string(), "web".to_string());
//!
//! let mut match_labels = BTreeMap::new();
//! match_labels.insert("app".to_string(), "web".to_string());
//!
//! let selector = LabelSelector {
//!     match_labels: Some(match_labels),
//!     match_expressions: None,
//! };
//!
//! assert!(matches_selector(&selector, &labels));
//! # }
//! ```

use crate::crd::{LabelSelector, LabelSelectorRequirement};
use std::collections::BTreeMap;

/// Check if a set of labels matches a label selector.
///
/// This function implements the Kubernetes label selector matching logic:
/// - All `matchLabels` entries must be present with exact values
/// - All `matchExpressions` requirements must be satisfied
/// - An empty selector matches everything
/// - A selector with no matchLabels and no matchExpressions matches everything
///
/// # Arguments
/// * `selector` - The label selector to evaluate
/// * `labels` - The labels to match against
///
/// # Returns
/// `true` if the labels match the selector, `false` otherwise
///
/// # Examples
/// ```
/// use std::collections::BTreeMap;
/// use bindy::crd::LabelSelector;
/// use bindy::selector::matches_selector;
///
/// let mut labels = BTreeMap::new();
/// labels.insert("app".to_string(), "web".to_string());
/// labels.insert("env".to_string(), "prod".to_string());
///
/// let mut match_labels = BTreeMap::new();
/// match_labels.insert("app".to_string(), "web".to_string());
///
/// let selector = LabelSelector {
///     match_labels: Some(match_labels),
///     match_expressions: None,
/// };
///
/// assert!(matches_selector(&selector, &labels));
/// ```
#[must_use]
pub fn matches_selector(selector: &LabelSelector, labels: &BTreeMap<String, String>) -> bool {
    // Check matchLabels
    if let Some(match_labels) = &selector.match_labels {
        for (key, value) in match_labels {
            if labels.get(key) != Some(value) {
                return false;
            }
        }
    }

    // Check matchExpressions
    if let Some(match_expressions) = &selector.match_expressions {
        for expr in match_expressions {
            if !matches_expression(expr, labels) {
                return false;
            }
        }
    }

    true
}

/// Check if a set of labels matches a single label selector requirement.
///
/// Implements the four Kubernetes label selector operators:
/// - `In`: Label value must be in the provided set
/// - `NotIn`: Label value must not be in the provided set
/// - `Exists`: Label key must be present (value doesn't matter)
/// - `DoesNotExist`: Label key must not be present
///
/// # Arguments
/// * `expr` - The label selector requirement to evaluate
/// * `labels` - The labels to match against
///
/// # Returns
/// `true` if the labels satisfy the requirement, `false` otherwise
fn matches_expression(expr: &LabelSelectorRequirement, labels: &BTreeMap<String, String>) -> bool {
    let key = &expr.key;
    let values = expr.values.as_deref().unwrap_or(&[]);

    match expr.operator.as_str() {
        "In" => {
            // Label must exist and value must be in the set
            labels.get(key).is_some_and(|v| values.contains(v))
        }
        "NotIn" => {
            // If label doesn't exist, it passes
            // If label exists, value must not be in the set
            labels.get(key).is_none_or(|v| !values.contains(v))
        }
        "Exists" => {
            // Label key must be present
            labels.contains_key(key)
        }
        "DoesNotExist" => {
            // Label key must not be present
            !labels.contains_key(key)
        }
        _ => {
            // Unknown operator - fail closed
            tracing::warn!("Unknown label selector operator: {}", expr.operator);
            false
        }
    }
}

#[cfg(test)]
#[path = "selector_tests.rs"]
mod selector_tests;
