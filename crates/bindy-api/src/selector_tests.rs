// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for label selector matching logic.

use super::*;
use crate::crd::{LabelSelector, LabelSelectorRequirement};
use std::collections::BTreeMap;

fn create_labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn create_match_labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    create_labels(pairs)
}

#[test]
fn test_empty_selector_matches_everything() {
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: None,
    };

    let labels = create_labels(&[("app", "web"), ("env", "prod")]);
    assert!(matches_selector(&selector, &labels));

    let empty_labels = BTreeMap::new();
    assert!(matches_selector(&selector, &empty_labels));
}

#[test]
fn test_match_labels_exact_match() {
    let match_labels = create_match_labels(&[("app", "web"), ("env", "prod")]);
    let selector = LabelSelector {
        match_labels: Some(match_labels),
        match_expressions: None,
    };

    let labels = create_labels(&[("app", "web"), ("env", "prod"), ("tier", "frontend")]);
    assert!(matches_selector(&selector, &labels));
}

#[test]
fn test_match_labels_missing_key() {
    let match_labels = create_match_labels(&[("app", "web"), ("env", "prod")]);
    let selector = LabelSelector {
        match_labels: Some(match_labels),
        match_expressions: None,
    };

    let labels = create_labels(&[("app", "web")]); // Missing "env"
    assert!(!matches_selector(&selector, &labels));
}

#[test]
fn test_match_labels_wrong_value() {
    let match_labels = create_match_labels(&[("app", "web")]);
    let selector = LabelSelector {
        match_labels: Some(match_labels),
        match_expressions: None,
    };

    let labels = create_labels(&[("app", "api")]); // Wrong value
    assert!(!matches_selector(&selector, &labels));
}

#[test]
fn test_match_expression_in_operator() {
    let expr = LabelSelectorRequirement {
        key: "env".to_string(),
        operator: "In".to_string(),
        values: Some(vec!["prod".to_string(), "staging".to_string()]),
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr]),
    };

    let labels_prod = create_labels(&[("env", "prod")]);
    assert!(matches_selector(&selector, &labels_prod));

    let labels_staging = create_labels(&[("env", "staging")]);
    assert!(matches_selector(&selector, &labels_staging));

    let labels_dev = create_labels(&[("env", "dev")]);
    assert!(!matches_selector(&selector, &labels_dev));

    let labels_missing = create_labels(&[("app", "web")]);
    assert!(!matches_selector(&selector, &labels_missing));
}

#[test]
fn test_match_expression_not_in_operator() {
    let expr = LabelSelectorRequirement {
        key: "env".to_string(),
        operator: "NotIn".to_string(),
        values: Some(vec!["dev".to_string(), "test".to_string()]),
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr]),
    };

    let labels_prod = create_labels(&[("env", "prod")]);
    assert!(matches_selector(&selector, &labels_prod));

    let labels_dev = create_labels(&[("env", "dev")]);
    assert!(!matches_selector(&selector, &labels_dev));

    // NotIn passes when label doesn't exist
    let labels_missing = create_labels(&[("app", "web")]);
    assert!(matches_selector(&selector, &labels_missing));
}

#[test]
fn test_match_expression_exists_operator() {
    let expr = LabelSelectorRequirement {
        key: "app".to_string(),
        operator: "Exists".to_string(),
        values: None,
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr]),
    };

    let labels_with_app = create_labels(&[("app", "web")]);
    assert!(matches_selector(&selector, &labels_with_app));

    let labels_without_app = create_labels(&[("env", "prod")]);
    assert!(!matches_selector(&selector, &labels_without_app));
}

#[test]
fn test_match_expression_does_not_exist_operator() {
    let expr = LabelSelectorRequirement {
        key: "deprecated".to_string(),
        operator: "DoesNotExist".to_string(),
        values: None,
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr]),
    };

    let labels_without_deprecated = create_labels(&[("app", "web")]);
    assert!(matches_selector(&selector, &labels_without_deprecated));

    let labels_with_deprecated = create_labels(&[("deprecated", "true")]);
    assert!(!matches_selector(&selector, &labels_with_deprecated));
}

#[test]
fn test_combined_match_labels_and_expressions() {
    let match_labels = create_match_labels(&[("app", "web")]);
    let expr = LabelSelectorRequirement {
        key: "env".to_string(),
        operator: "In".to_string(),
        values: Some(vec!["prod".to_string(), "staging".to_string()]),
    };
    let selector = LabelSelector {
        match_labels: Some(match_labels),
        match_expressions: Some(vec![expr]),
    };

    // Both conditions met
    let labels_pass = create_labels(&[("app", "web"), ("env", "prod")]);
    assert!(matches_selector(&selector, &labels_pass));

    // match_labels fails
    let labels_wrong_app = create_labels(&[("app", "api"), ("env", "prod")]);
    assert!(!matches_selector(&selector, &labels_wrong_app));

    // expression fails
    let labels_wrong_env = create_labels(&[("app", "web"), ("env", "dev")]);
    assert!(!matches_selector(&selector, &labels_wrong_env));
}

#[test]
fn test_multiple_expressions_all_must_match() {
    let expr1 = LabelSelectorRequirement {
        key: "app".to_string(),
        operator: "Exists".to_string(),
        values: None,
    };
    let expr2 = LabelSelectorRequirement {
        key: "env".to_string(),
        operator: "In".to_string(),
        values: Some(vec!["prod".to_string()]),
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr1, expr2]),
    };

    // Both expressions satisfied
    let labels_pass = create_labels(&[("app", "web"), ("env", "prod")]);
    assert!(matches_selector(&selector, &labels_pass));

    // First expression fails
    let labels_no_app = create_labels(&[("env", "prod")]);
    assert!(!matches_selector(&selector, &labels_no_app));

    // Second expression fails
    let labels_wrong_env = create_labels(&[("app", "web"), ("env", "dev")]);
    assert!(!matches_selector(&selector, &labels_wrong_env));
}

#[test]
fn test_unknown_operator_fails() {
    let expr = LabelSelectorRequirement {
        key: "app".to_string(),
        operator: "UnknownOperator".to_string(),
        values: Some(vec!["web".to_string()]),
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr]),
    };

    let labels = create_labels(&[("app", "web")]);
    assert!(!matches_selector(&selector, &labels));
}

#[test]
fn test_in_operator_empty_values() {
    let expr = LabelSelectorRequirement {
        key: "app".to_string(),
        operator: "In".to_string(),
        values: Some(vec![]),
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr]),
    };

    let labels = create_labels(&[("app", "web")]);
    // Can't be in empty set
    assert!(!matches_selector(&selector, &labels));
}

#[test]
fn test_not_in_operator_empty_values() {
    let expr = LabelSelectorRequirement {
        key: "app".to_string(),
        operator: "NotIn".to_string(),
        values: Some(vec![]),
    };
    let selector = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![expr]),
    };

    let labels = create_labels(&[("app", "web")]);
    // NotIn empty set always passes
    assert!(matches_selector(&selector, &labels));
}

#[test]
fn test_real_world_zone_selector() {
    // Simulate a DNSZone's recordsFrom selector
    let match_labels = create_match_labels(&[("zone", "example.com")]);
    let expr = LabelSelectorRequirement {
        key: "record-type".to_string(),
        operator: "In".to_string(),
        values: Some(vec!["public".to_string()]),
    };
    let selector = LabelSelector {
        match_labels: Some(match_labels),
        match_expressions: Some(vec![expr]),
    };

    // Record that should match
    let record_labels = create_labels(&[
        ("zone", "example.com"),
        ("record-type", "public"),
        ("owner", "team-a"),
    ]);
    assert!(matches_selector(&selector, &record_labels));

    // Record with wrong zone
    let wrong_zone = create_labels(&[("zone", "other.com"), ("record-type", "public")]);
    assert!(!matches_selector(&selector, &wrong_zone));

    // Record with wrong type
    let wrong_type = create_labels(&[("zone", "example.com"), ("record-type", "internal")]);
    assert!(!matches_selector(&selector, &wrong_type));
}

#[test]
fn test_real_world_instance_selector() {
    // Simulate a DNSZone's bind9InstancesFrom selector
    let match_labels = create_match_labels(&[("managed-by", "dns-operator")]);
    let expr1 = LabelSelectorRequirement {
        key: "tier".to_string(),
        operator: "In".to_string(),
        values: Some(vec!["production".to_string(), "staging".to_string()]),
    };
    let expr2 = LabelSelectorRequirement {
        key: "deprecated".to_string(),
        operator: "DoesNotExist".to_string(),
        values: None,
    };
    let selector = LabelSelector {
        match_labels: Some(match_labels),
        match_expressions: Some(vec![expr1, expr2]),
    };

    // Zone that should match
    let zone_labels = create_labels(&[
        ("managed-by", "dns-operator"),
        ("tier", "production"),
        ("team", "platform"),
    ]);
    assert!(matches_selector(&selector, &zone_labels));

    // Staging zone should also match
    let staging_zone = create_labels(&[("managed-by", "dns-operator"), ("tier", "staging")]);
    assert!(matches_selector(&selector, &staging_zone));

    // Dev zone should not match (tier not in list)
    let dev_zone = create_labels(&[("managed-by", "dns-operator"), ("tier", "dev")]);
    assert!(!matches_selector(&selector, &dev_zone));

    // Deprecated zone should not match
    let deprecated_zone = create_labels(&[
        ("managed-by", "dns-operator"),
        ("tier", "production"),
        ("deprecated", "true"),
    ]);
    assert!(!matches_selector(&selector, &deprecated_zone));
}
