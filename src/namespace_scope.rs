// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Operator namespace scoping.
//!
//! Controls the set of namespaces the operator watches and manages, driven by
//! the `BINDY_WATCH_NAMESPACES` environment variable. This is the foundation
//! of the least-privilege deployment model that closes audit findings C2
//! (operator can create workloads cluster-wide) and H3 (operator can read all
//! Secrets cluster-wide): when the scope is restricted to specific namespaces,
//! the operator builds its watches with `Api::namespaced` and only needs
//! per-namespace RBAC (RoleBindings) instead of a cluster-wide
//! ClusterRoleBinding.
//!
//! The default is [`NamespaceScope::All`] (cluster-wide) so existing single
//! cluster-wide installs keep working unchanged.

use std::collections::HashSet;

/// Environment variable naming the namespaces the operator should watch.
///
/// A comma-separated list (e.g. `bindy-system,tenant-a,tenant-b`). Unset or
/// empty means watch every namespace cluster-wide.
pub const WATCH_NAMESPACES_ENV: &str = "BINDY_WATCH_NAMESPACES";

/// The set of namespaces the operator watches and manages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamespaceScope {
    /// Watch every namespace cluster-wide. Requires cluster-wide RBAC
    /// (ClusterRole + ClusterRoleBinding). This is the default.
    All,
    /// Watch only the listed namespaces. Requires only per-namespace RBAC
    /// (Role + RoleBinding in each namespace). Never empty.
    Namespaces(Vec<String>),
}

impl NamespaceScope {
    /// Parse a [`WATCH_NAMESPACES_ENV`] value into a scope.
    ///
    /// - `None`, empty, or whitespace-only → [`NamespaceScope::All`]
    ///   (backward compatible: the operator keeps watching cluster-wide).
    /// - A comma-separated list → [`NamespaceScope::Namespaces`] with each
    ///   entry trimmed, empty entries dropped, and duplicates removed while
    ///   preserving first-seen order. If every entry is empty it falls back to
    ///   [`NamespaceScope::All`].
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw else {
            return Self::All;
        };

        let mut seen = HashSet::new();
        let mut namespaces = Vec::new();
        for entry in raw.split(',') {
            let ns = entry.trim();
            if ns.is_empty() {
                continue;
            }
            if seen.insert(ns.to_string()) {
                namespaces.push(ns.to_string());
            }
        }

        if namespaces.is_empty() {
            Self::All
        } else {
            Self::Namespaces(namespaces)
        }
    }

    /// Load the scope from the [`WATCH_NAMESPACES_ENV`] environment variable.
    #[must_use]
    pub fn from_env() -> Self {
        Self::parse(std::env::var(WATCH_NAMESPACES_ENV).ok().as_deref())
    }

    /// Whether the operator watches cluster-wide.
    #[must_use]
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// The watched namespaces, or an empty slice when cluster-wide.
    #[must_use]
    pub fn namespaces(&self) -> &[String] {
        match self {
            Self::All => &[],
            Self::Namespaces(ns) => ns,
        }
    }
}

#[cfg(test)]
#[path = "namespace_scope_tests.rs"]
mod namespace_scope_tests;
