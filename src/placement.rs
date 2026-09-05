// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Pod placement: topology spreading, node selection, tolerations, affinity.
//!
//! # Why this module exists
//!
//! A DNS server is not an anonymous replica. A zone's `NS` records name the
//! individual servers authoritative for it, so every primary needs a stable
//! identity and its own address. Bindy models that by giving each nameserver
//! its own `Bind9Instance`, its own Deployment, and its own Service — which
//! means a `Bind9Cluster` with `primary.replicas: 3` produces **three
//! single-Pod Deployments**, not one three-Pod Deployment.
//!
//! That shape breaks the obvious implementation of zone spreading. A
//! `topologySpreadConstraint` balances the set of Pods matched by its
//! `labelSelector`, counted per value of `topologyKey`. If the operator
//! generated a selector matching a Deployment's own Pods, the set would have
//! exactly one member — always trivially balanced, so the constraint would be
//! satisfied by any placement and all three primaries could still land in one
//! zone.
//!
//! The fix is [`SpreadScope`]: the selector is generated to match *sibling*
//! instances via `bindy.firestoned.io/cluster` + `bindy.firestoned.io/role`,
//! so the scheduler counts all primaries of a cluster as one set. Users never
//! write that selector themselves — they cannot know the operator's internal
//! Pod labels, and a wrong selector fails silently rather than loudly.
//!
//! # Defaults
//!
//! With no `placement` block anywhere, the operator emits a single **soft**
//! (`ScheduleAnyway`) zone-spread constraint whenever the resolved Pod set has
//! two or more members. Soft is deliberate: a hard constraint turns a
//! single-zone cluster — or a zone outage, the very thing this feature guards
//! against — into `Pending` DNS Pods, trading degraded availability for a
//! total outage.
//!
//! # Scope
//!
//! This module handles topology spreading and nothing else. It deliberately
//! does **not** accept `nodeSelector`, `tolerations`, or `affinity`: those are
//! general pod-spec passthrough, they inflated the generated CRDs by ~450KB,
//! and they are exactly the primitives a namespace tenant would need to place
//! an operator-credentialed Pod onto a control-plane node. Keeping them out
//! removes that threat model rather than mitigating it. See
//! `docs/adr/0003-pod-placement-and-zone-spreading.md`.
//!
//! What remains to validate is correctness, not security: rules Kubernetes
//! would reject, caught here so the user sees a clear condition on the CR they
//! edited instead of an opaque Deployment failure. Structural limits that the
//! CRD schema *can* express (rule count, label-key syntax, value ranges, and
//! the `minDomains`/`DoNotSchedule` pairing) are enforced at admission by the
//! generated schema; this module is the backstop.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::TopologySpreadConstraint;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use thiserror::Error;
use tracing::{debug, warn};

use crate::constants::{DEFAULT_SPREAD_MAX_SKEW, MAX_SPREAD_RULES, TOPOLOGY_KEY_ZONE};
use crate::crd::{
    Bind9Cluster, Bind9Instance, ClusterBind9Provider, NodeInclusionPolicy, PlacementConfig,
    ServerRole, SpreadRule, SpreadScope, WhenUnsatisfiable,
};
use crate::labels::{BINDY_CLUSTER_LABEL, BINDY_ROLE_LABEL, ROLE_PRIMARY, ROLE_SECONDARY};

// ============================================================================
// Resolved output
// ============================================================================

/// The Pod-spec field this module produces.
///
/// Built by [`build_pod_placement`] and applied onto the Deployment's Pod
/// template by `crate::bind9_resources::build_pod_spec`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedPlacement {
    /// Generated from `placement.spread` (or the operator default).
    pub topology_spread_constraints: Option<Vec<TopologySpreadConstraint>>,
}

impl ResolvedPlacement {
    /// True when nothing would be applied to the Pod spec.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.topology_spread_constraints.is_none()
    }
}

/// Everything [`build_pod_placement`] needs to know about the instance whose
/// Pod spec is being built.
#[derive(Clone, Debug)]
pub struct PlacementContext<'a> {
    /// Name of the `Bind9Instance` (also the Deployment name).
    pub instance_name: &'a str,
    /// Owning cluster / provider name, or `None` for a standalone instance.
    ///
    /// `Role` and `Cluster` spread scopes are only meaningful with a cluster:
    /// they select on `bindy.firestoned.io/cluster`, which a standalone
    /// instance's Pods do not carry.
    pub cluster_name: Option<&'a str>,
    /// Role of this instance.
    pub role: ServerRole,
    /// Pods in this instance's own Deployment (`spec.replicas`).
    pub instance_replicas: i32,
    /// How many instances of this role the owning cluster asks for.
    ///
    /// `1` for a standalone instance. This is what makes the default fire for
    /// a cluster of three single-Pod primaries, where `instance_replicas` on
    /// its own would always be 1 and never reach the threshold.
    pub role_instance_count: i32,
    /// Total instances the cluster asks for across both roles.
    ///
    /// Only used to decide whether a `Cluster`-scoped default is worth
    /// emitting.
    pub cluster_instance_count: i32,
    /// This Deployment's own Pod selector labels, used for `Instance` scope.
    pub instance_selector_labels: &'a BTreeMap<String, String>,
}

// ============================================================================
// Resolution (precedence)
// ============================================================================

/// Resolves which `placement` block applies to an instance.
///
/// Precedence, highest first:
///
/// 1. `Bind9Instance.spec.placement`
/// 2. `spec.primary.placement` / `spec.secondary.placement` on the owning
///    `Bind9Cluster` or `ClusterBind9Provider`
///
/// Resolution is **whole-block**: the more specific level wins outright and
/// the other is not merged into it. Merging would make "what will actually be
/// scheduled" unanswerable without mentally combining two blocks, and a
/// silently half-inherited scheduling rule is a bad failure mode — a Pod that
/// lands somewhere unexpected is hard to notice until a zone goes down.
#[must_use]
pub fn resolve_placement<'a>(
    instance: &'a Bind9Instance,
    cluster: Option<&'a Bind9Cluster>,
    cluster_provider: Option<&'a ClusterBind9Provider>,
) -> Option<&'a PlacementConfig> {
    // 1. Instance level.
    if let Some(p) = instance.spec.placement.as_ref() {
        debug!(instance = %instance.spec.cluster_ref, "Using instance-level placement");
        return Some(p);
    }

    // 2. Role level, then 3. cluster level — checked against the namespace-
    //    scoped cluster first, then the cluster-scoped provider. An instance
    //    only ever belongs to one of the two, so at most one arm contributes.
    let role_level = match instance.spec.role {
        ServerRole::Primary => cluster
            .and_then(|c| c.spec.common.primary.as_ref())
            .and_then(|p| p.placement.as_ref())
            .or_else(|| {
                cluster_provider
                    .and_then(|p| p.spec.common.primary.as_ref())
                    .and_then(|p| p.placement.as_ref())
            }),
        ServerRole::Secondary => cluster
            .and_then(|c| c.spec.common.secondary.as_ref())
            .and_then(|s| s.placement.as_ref())
            .or_else(|| {
                cluster_provider
                    .and_then(|p| p.spec.common.secondary.as_ref())
                    .and_then(|s| s.placement.as_ref())
            }),
    };
    role_level
}

// ============================================================================
// Building
// ============================================================================

/// Builds the Pod-spec placement fields for an instance.
///
/// `config` is the block returned by [`resolve_placement`]; `None` means no
/// user configuration at any level, in which case only the operator default
/// spread applies.
#[must_use]
pub fn build_pod_placement(
    config: Option<&PlacementConfig>,
    ctx: &PlacementContext<'_>,
) -> ResolvedPlacement {
    let constraints = match config.and_then(|c| c.spread.as_ref()) {
        // Explicit rules: exactly these, no default.
        Some(rules) if !rules.is_empty() => rules
            .iter()
            .filter_map(|rule| build_constraint(rule, ctx))
            .collect::<Vec<_>>(),
        // Explicit empty list: the user opted out.
        Some(_) => {
            debug!(
                instance = %ctx.instance_name,
                "placement.spread is an empty list; emitting no topology spread constraints"
            );
            Vec::new()
        }
        // Absent: operator default.
        None => default_constraints(ctx),
    };

    ResolvedPlacement {
        topology_spread_constraints: (!constraints.is_empty()).then_some(constraints),
    }
}

/// The operator's default spread: one soft zone rule for **primaries only**,
/// when the Pod set is large enough for spreading to mean anything.
///
/// # Why primaries only
///
/// Issue #467 asked for primaries, and the asymmetry is real: a primary is
/// authoritative and holds the writable copy of a zone, whereas a secondary
/// can be re-created from a primary at any time. Losing every primary to one
/// zone outage is the failure this feature exists to prevent.
///
/// The other half of the reasoning is about upgrades. Applying a default to
/// secondaries would silently change where already-running secondary Pods are
/// scheduled the moment an operator is upgraded — an unannounced scheduling
/// policy change on a live cluster, which is not something an operator should
/// do on a user's behalf. Secondaries opt in via `secondary.placement.spread`.
fn default_constraints(ctx: &PlacementContext<'_>) -> Vec<TopologySpreadConstraint> {
    if ctx.role != ServerRole::Primary {
        debug!(
            instance = %ctx.instance_name,
            "Default zone spread applies to primaries only; set secondary.placement.spread to opt in"
        );
        return Vec::new();
    }

    let default_rule = SpreadRule {
        topology_key: TOPOLOGY_KEY_ZONE.to_string(),
        max_skew: None,
        when_unsatisfiable: None,
        scope: None,
        min_domains: None,
        node_affinity_policy: None,
        node_taints_policy: None,
    };

    let scope = effective_scope(&default_rule, ctx);
    let set_size = pod_set_size(scope, ctx);

    if set_size < 2 {
        debug!(
            instance = %ctx.instance_name,
            set_size,
            "Resolved Pod set has fewer than 2 members; skipping default zone spread"
        );
        return Vec::new();
    }

    debug!(
        instance = %ctx.instance_name,
        set_size,
        scope = ?scope,
        "Applying default soft zone spread"
    );
    build_constraint(&default_rule, ctx).into_iter().collect()
}

/// Number of Pods the scheduler will count for a given scope.
///
/// Used only to decide whether the *default* is worth emitting. Explicit user
/// rules are always honoured, however small the set — the user asked for them.
fn pod_set_size(scope: SpreadScope, ctx: &PlacementContext<'_>) -> i32 {
    let replicas = ctx.instance_replicas.max(0);
    match scope {
        SpreadScope::Instance => replicas,
        SpreadScope::Role => ctx.role_instance_count.max(1).saturating_mul(replicas),
        SpreadScope::Cluster => ctx.cluster_instance_count.max(1).saturating_mul(replicas),
    }
}

/// Picks the scope for a rule: explicit if set, else `Role` for a
/// cluster-managed instance and `Instance` for a standalone one.
fn effective_scope(rule: &SpreadRule, ctx: &PlacementContext<'_>) -> SpreadScope {
    match rule.scope {
        Some(scope) => scope,
        None => {
            if ctx.cluster_name.is_some() {
                SpreadScope::Role
            } else {
                SpreadScope::Instance
            }
        }
    }
}

/// Turns one [`SpreadRule`] into a Kubernetes `TopologySpreadConstraint`.
///
/// Returns `None` when the rule cannot produce a meaningful constraint — a
/// cluster-scoped rule on a standalone instance, for instance, whose Pods
/// carry no cluster label to select on.
fn build_constraint(
    rule: &SpreadRule,
    ctx: &PlacementContext<'_>,
) -> Option<TopologySpreadConstraint> {
    let requested = effective_scope(rule, ctx);
    let scope = match (requested, ctx.cluster_name) {
        // Role / Cluster scope needs the cluster label, which only exists on
        // Pods belonging to a cluster. Fall back rather than emitting a
        // constraint whose selector matches nothing.
        (SpreadScope::Role | SpreadScope::Cluster, None) => {
            warn!(
                instance = %ctx.instance_name,
                requested_scope = ?requested,
                "Spread scope requires an owning Bind9Cluster; falling back to Instance scope"
            );
            SpreadScope::Instance
        }
        (scope, _) => scope,
    };

    let label_selector = build_selector(scope, ctx);

    let when_unsatisfiable = rule
        .when_unsatisfiable
        .unwrap_or(WhenUnsatisfiable::ScheduleAnyway);

    Some(TopologySpreadConstraint {
        topology_key: rule.topology_key.clone(),
        max_skew: rule.max_skew.unwrap_or(DEFAULT_SPREAD_MAX_SKEW),
        when_unsatisfiable: when_unsatisfiable_str(when_unsatisfiable).to_string(),
        label_selector: Some(label_selector),
        // `minDomains` only has meaning for a hard constraint; the API server
        // rejects it alongside ScheduleAnyway.
        min_domains: match when_unsatisfiable {
            WhenUnsatisfiable::DoNotSchedule => rule.min_domains,
            WhenUnsatisfiable::ScheduleAnyway => None,
        },
        node_affinity_policy: rule.node_affinity_policy.map(node_policy_str_owned),
        node_taints_policy: rule.node_taints_policy.map(node_policy_str_owned),
        match_label_keys: None,
    })
}

/// Generates the `labelSelector` that defines the balanced Pod set.
///
/// This is the crux of the whole feature: get the selector wrong and the
/// constraint is silently satisfied by every placement.
fn build_selector(scope: SpreadScope, ctx: &PlacementContext<'_>) -> LabelSelector {
    let match_labels = match scope {
        // This Deployment's own Pods.
        SpreadScope::Instance => ctx.instance_selector_labels.clone(),
        // Every Pod of this role across the cluster — i.e. all the sibling
        // single-Pod Deployments the cluster controller created.
        SpreadScope::Role => {
            let mut m = BTreeMap::new();
            if let Some(cluster) = ctx.cluster_name {
                m.insert(BINDY_CLUSTER_LABEL.to_string(), cluster.to_string());
            }
            m.insert(BINDY_ROLE_LABEL.to_string(), role_str(ctx.role).to_string());
            m
        }
        // Every DNS Pod of the cluster, both roles together.
        SpreadScope::Cluster => {
            let mut m = BTreeMap::new();
            if let Some(cluster) = ctx.cluster_name {
                m.insert(BINDY_CLUSTER_LABEL.to_string(), cluster.to_string());
            }
            m
        }
    };

    LabelSelector {
        match_labels: Some(match_labels),
        ..Default::default()
    }
}

fn role_str(role: ServerRole) -> &'static str {
    match role {
        ServerRole::Primary => ROLE_PRIMARY,
        ServerRole::Secondary => ROLE_SECONDARY,
    }
}

fn when_unsatisfiable_str(value: WhenUnsatisfiable) -> &'static str {
    match value {
        WhenUnsatisfiable::DoNotSchedule => "DoNotSchedule",
        WhenUnsatisfiable::ScheduleAnyway => "ScheduleAnyway",
    }
}

fn node_policy_str_owned(value: NodeInclusionPolicy) -> String {
    match value {
        NodeInclusionPolicy::Honor => "Honor".to_string(),
        NodeInclusionPolicy::Ignore => "Ignore".to_string(),
    }
}

// ============================================================================
// Validation
// ============================================================================

/// Rejection reasons returned by [`validate_placement`].
///
/// These are **correctness** checks, not security checks. Since `placement`
/// accepts topology spreading only, there is no scheduling primitive here that
/// could place a Pod somewhere it is not otherwise allowed — see the module
/// docs. Each variant carries enough context for the reconciler to render an
/// actionable `Ready=False` condition on the offending CR.
///
/// Most of these are also enforced structurally by the generated CRD schema
/// (`maxItems`, the `topologyKey` pattern, value ranges, and an
/// `x-kubernetes-validations` rule for the `minDomains` pairing), so they are
/// normally rejected at admission. This validator remains the backstop for
/// clusters running an older CRD revision, and covers the one rule a
/// structural schema cannot express cheaply: uniqueness across rules.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlacementRejection {
    #[error(
        "placement.spread has {count} rules, which exceeds the maximum of {MAX_SPREAD_RULES}; \
         every rule is evaluated on each scheduling attempt"
    )]
    TooManySpreadRules { count: usize },

    #[error(
        "placement.spread[{index}].topologyKey {key:?} is not a valid Kubernetes label key: {reason}"
    )]
    InvalidTopologyKey {
        index: usize,
        key: String,
        reason: &'static str,
    },

    #[error(
        "placement.spread has two rules with the same topologyKey {key:?} and whenUnsatisfiable \
         {when:?}; Kubernetes requires this pair to be unique across constraints"
    )]
    DuplicateSpreadRule { key: String, when: String },

    #[error("placement.spread[{index}].maxSkew must be greater than 0, got {value}")]
    InvalidMaxSkew { index: usize, value: i32 },

    #[error("placement.spread[{index}].minDomains must be greater than 0, got {value}")]
    InvalidMinDomains { index: usize, value: i32 },

    #[error(
        "placement.spread[{index}] sets minDomains, which Kubernetes only permits together with \
         whenUnsatisfiable: DoNotSchedule"
    )]
    MinDomainsRequiresHardConstraint { index: usize },
}

/// Validates a user-supplied `placement` block.
///
/// Catches inputs Kubernetes itself would reject, so the user sees a clear
/// condition on their CR instead of a Deployment that silently fails to apply.
///
/// # Errors
///
/// Returns the first [`PlacementRejection`] encountered.
pub fn validate_placement(config: &PlacementConfig) -> Result<(), PlacementRejection> {
    if let Some(rules) = config.spread.as_ref() {
        validate_spread_rules(rules)?;
    }
    Ok(())
}

/// Convenience wrapper for the common `Option<&PlacementConfig>` shape.
///
/// # Errors
///
/// Returns the first [`PlacementRejection`] encountered, or `Ok(())` when the
/// block is absent.
pub fn validate_optional_placement(
    config: Option<&PlacementConfig>,
) -> Result<(), PlacementRejection> {
    config.map_or(Ok(()), validate_placement)
}

fn validate_spread_rules(rules: &[SpreadRule]) -> Result<(), PlacementRejection> {
    if rules.len() > MAX_SPREAD_RULES {
        return Err(PlacementRejection::TooManySpreadRules { count: rules.len() });
    }

    let mut seen: Vec<(String, String)> = Vec::with_capacity(rules.len());

    for (index, rule) in rules.iter().enumerate() {
        if let Err(reason) = validate_label_key(&rule.topology_key) {
            return Err(PlacementRejection::InvalidTopologyKey {
                index,
                key: rule.topology_key.clone(),
                reason,
            });
        }

        if let Some(skew) = rule.max_skew {
            if skew <= 0 {
                return Err(PlacementRejection::InvalidMaxSkew { index, value: skew });
            }
        }

        let when = rule
            .when_unsatisfiable
            .unwrap_or(WhenUnsatisfiable::ScheduleAnyway);

        if let Some(min_domains) = rule.min_domains {
            if min_domains <= 0 {
                return Err(PlacementRejection::InvalidMinDomains {
                    index,
                    value: min_domains,
                });
            }
            if when != WhenUnsatisfiable::DoNotSchedule {
                return Err(PlacementRejection::MinDomainsRequiresHardConstraint { index });
            }
        }

        // Kubernetes requires (topologyKey, whenUnsatisfiable) to be unique
        // across a Pod's constraints and rejects the Pod otherwise. Catching
        // it here turns an opaque Deployment-level failure into a condition on
        // the CR the user actually edited.
        let key = (
            rule.topology_key.clone(),
            when_unsatisfiable_str(when).to_string(),
        );
        if seen.contains(&key) {
            return Err(PlacementRejection::DuplicateSpreadRule {
                key: key.0,
                when: key.1,
            });
        }
        seen.push(key);
    }

    Ok(())
}

/// Validates a Kubernetes label key, matching `IsQualifiedName` in
/// `k8s.io/apimachinery/pkg/util/validation`.
///
/// An optional lowercase RFC 1123 subdomain prefix of at most 253 characters,
/// a `/`, and a name segment of at most 63 characters — so 317 overall.
///
/// # Deliberately not enforced: a 63-character cap per DNS label
///
/// RFC 1123 caps a single DNS label at 63 octets, but Kubernetes'
/// `IsDNS1123Subdomain` checks only the 253-character total and the subdomain
/// charset — it does not bound individual labels. A key such as
/// `<64 a's>/zone` is therefore accepted by the API server, both as a node
/// label and as a `topologyKey` on a Pod (verified against a live cluster).
/// Rejecting it here would make Bindy refuse input Kubernetes accepts, which
/// is worse than mirroring the platform's own looseness.
fn validate_label_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("must not be empty");
    }
    // 253 (prefix) + 1 ('/') + 63 (name). An earlier revision used 316 here,
    // which rejected a maximally-sized but perfectly valid key.
    if key.len() > 317 {
        return Err("must be at most 317 characters");
    }

    let name = match key.split_once('/') {
        Some((prefix, name)) => {
            if prefix.is_empty() {
                return Err("prefix before '/' must not be empty");
            }
            if prefix.len() > 253 {
                return Err("prefix before '/' must be at most 253 characters");
            }
            if !prefix.split('.').all(is_dns1123_label) {
                return Err(
                    "prefix before '/' must be a lowercase RFC 1123 subdomain: dot-separated \
                     segments of alphanumerics and '-', each starting and ending with an \
                     alphanumeric",
                );
            }
            name
        }
        None => key,
    };

    if name.is_empty() {
        return Err("name segment must not be empty");
    }
    if name.len() > 63 {
        return Err("name segment must be at most 63 characters");
    }
    if !name.chars().all(is_label_name_char) {
        return Err("name segment may contain only alphanumerics, '-', '_' and '.'");
    }
    if !starts_and_ends_alphanumeric(name) {
        return Err("name segment must start and end with an alphanumeric character");
    }

    Ok(())
}

/// One dot-separated segment of an RFC 1123 subdomain, as Kubernetes validates
/// it: non-empty, lowercase alphanumerics and `-`, starting and ending with an
/// alphanumeric. Length is bounded by the caller's 253-character prefix cap;
/// see `validate_label_key` for why there is no per-segment cap.
fn is_dns1123_label(part: &str) -> bool {
    !part.is_empty() && part.chars().all(is_dns_label_char) && starts_and_ends_alphanumeric(part)
}

fn starts_and_ends_alphanumeric(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && value
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric())
}

fn is_dns_label_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

fn is_label_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod placement_tests;
