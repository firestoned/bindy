// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Gateway addresses from Scout's watch caches (ADR-0028).
//!
//! A route with no IP annotation publishes the addresses of the Gateway it
//! attaches to: the Gateway's `status.addresses`, or, when those are empty,
//! the `LoadBalancer` Service(s) the `--gateway-service` target of its class
//! names. Everything here reads Scout's Service and Gateway watch caches and
//! does no I/O, so the route controllers can also use it to decide which
//! routes a Service or Gateway change must wake (Decision 4).

use crate::scout::{is_loadbalancer_service, Gateway, GatewayServiceTarget, NamespacedName};
use anyhow::{anyhow, Result};
use k8s_openapi::api::core::v1::Service;
use kube::core::{Expression, Selector, SelectorExt};
use kube::ResourceExt;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The label Gateway API implementations put on every resource they generate
/// for a Gateway, its value the Gateway's name (GEP-1762, Standard: MUST).
pub(crate) const GATEWAY_NAME_LABEL: &str = "gateway.networking.k8s.io/gateway-name";

/// Parses an equality-based Kubernetes label selector: comma-separated
/// `key=value`, `key==value`, `key!=value`, `key` (exists) and `!key` (does
/// not exist). Set-based expressions (`in`, `notin`) are not accepted.
///
/// # Errors
/// Returns an error for an empty selector, an empty key or value, a value
/// containing `=`, or a set-based expression, so a malformed
/// `--gateway-service` entry fails Scout at startup.
pub(crate) fn parse_label_selector(selector: &str) -> Result<Selector> {
    let mut expressions = Vec::new();
    for raw in selector.split(',') {
        let term = raw.trim();
        if term.is_empty() {
            return Err(anyhow!("empty term in label selector {selector:?}"));
        }
        if term.contains('(') || term.contains(' ') {
            return Err(anyhow!(
                "set-based label selector {term:?} is not supported; use key=value terms"
            ));
        }
        let expression = if let Some((key, value)) = term.split_once("!=") {
            Expression::NotEqual(valid_part(key, selector)?, valid_part(value, selector)?)
        } else if let Some((key, value)) = term.split_once("==").or_else(|| term.split_once('=')) {
            Expression::Equal(valid_part(key, selector)?, valid_part(value, selector)?)
        } else if let Some(key) = term.strip_prefix('!') {
            Expression::DoesNotExist(valid_part(key, selector)?)
        } else {
            Expression::Exists(valid_part(term, selector)?)
        };
        expressions.push(expression);
    }
    Ok(expressions.into_iter().collect())
}

/// A key or value of a selector term: not empty, no `=` or `!` left in it.
fn valid_part(part: &str, selector: &str) -> Result<String> {
    let part = part.trim();
    if part.is_empty() || part.contains('=') || part.contains('!') {
        return Err(anyhow!("malformed label selector {selector:?}"));
    }
    Ok(part.to_string())
}

/// Whether `target` selects `svc`: the same namespace and name for a name
/// target, the same namespace and a matching label selector for a labeled
/// one. A selector that does not parse matches nothing (startup refuses it).
pub(crate) fn target_matches(target: &GatewayServiceTarget, svc: &Service) -> bool {
    let namespace = svc.namespace().unwrap_or_default();
    match target {
        GatewayServiceTarget::Name(name) => {
            name.namespace == namespace && name.name == svc.name_any()
        }
        GatewayServiceTarget::Labeled {
            namespace: target_namespace,
            selector,
        } => {
            *target_namespace == namespace
                && parse_label_selector(selector).is_ok_and(|s| s.matches(svc.labels()))
        }
    }
}

/// The `LoadBalancer` Services `target` gives the Gateway `gateway`
/// (Decisions 1 and 2).
///
/// The `LoadBalancer` Services the target selects that are not being deleted
/// are the candidates. When any candidate carries [`GATEWAY_NAME_LABEL`], only
/// the candidates labelled for `gateway` count (each Gateway its own
/// Service); otherwise every candidate does (one Service shared by the
/// class). Their IPv4 addresses are what the Gateway serves; a Service with
/// no address yet adds nothing.
pub(crate) fn gateway_lb_services<'a>(
    target: &GatewayServiceTarget,
    gateway: &NamespacedName,
    services: &'a [Arc<Service>],
) -> Vec<&'a Arc<Service>> {
    let candidates: Vec<&Arc<Service>> = services
        .iter()
        .filter(|svc| svc.metadata.deletion_timestamp.is_none())
        .filter(|svc| is_loadbalancer_service(svc))
        .filter(|svc| target_matches(target, svc))
        .collect();
    let scoped = candidates
        .iter()
        .any(|svc| svc.labels().contains_key(GATEWAY_NAME_LABEL));
    candidates
        .into_iter()
        .filter(|svc| !scoped || svc.labels().get(GATEWAY_NAME_LABEL) == Some(&gateway.name))
        .collect()
}

/// The Gateways a change of `svc` concerns (Decision 4): those whose class
/// maps to a target that selects `svc`.
pub(crate) fn gateways_for_service(
    svc: &Service,
    targets: &BTreeMap<String, GatewayServiceTarget>,
    gateways: &[Arc<Gateway>],
) -> Vec<NamespacedName> {
    let classes: BTreeSet<&str> = targets
        .iter()
        .filter(|(_, target)| target_matches(target, svc))
        .map(|(class, _)| class.as_str())
        .collect();
    if classes.is_empty() {
        return Vec::new();
    }
    gateways
        .iter()
        .filter(|gw| classes.contains(gw.spec.gateway_class_name.as_str()))
        .map(|gw| NamespacedName {
            namespace: gw.namespace().unwrap_or_default(),
            name: gw.name_any(),
        })
        .collect()
}

/// The routes, of `(route, parent gateways)` pairs, that attach to any of
/// `gateways` (Decision 4).
pub(crate) fn routes_for_gateways(
    routes: &[(NamespacedName, Vec<NamespacedName>)],
    gateways: &[NamespacedName],
) -> Vec<NamespacedName> {
    routes
        .iter()
        .filter(|(_, parents)| parents.iter().any(|p| gateways.contains(p)))
        .map(|(route, _)| route.clone())
        .collect()
}

#[cfg(test)]
#[path = "gateway_targets_tests.rs"]
mod gateway_targets_tests;
