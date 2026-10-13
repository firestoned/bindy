// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

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

    /// The namespace targets to build `Api` handles from, one per watch.
    ///
    /// `None` means "cluster-wide" (build with `Api::all`); `Some(ns)` means
    /// "this namespace only" (build with `Api::namespaced`). The result is
    /// **never empty** — an empty target list would silently disable every
    /// watch, leaving the operator healthy but reconciling nothing.
    ///
    /// [`NamespaceScope::All`] deliberately yields exactly one `None` target so
    /// the default deployment keeps its current single-watch-per-kind shape,
    /// byte-for-byte. All the fan-out risk is confined to the opt-in scoped mode.
    #[must_use]
    pub fn api_targets(&self) -> Vec<Option<&str>> {
        match self {
            Self::All => vec![None],
            Self::Namespaces(ns) => ns.iter().map(|n| Some(n.as_str())).collect(),
        }
    }
}

/// Build an `Api` for a single namespace target.
///
/// `None` means cluster-wide (`Api::all`); `Some(ns)` means that namespace only
/// (`Api::namespaced`). Pair with [`NamespaceScope::api_targets`].
pub fn scoped_namespaced_api<K>(client: &kube::Client, target: Option<&str>) -> kube::Api<K>
where
    K: kube::Resource<Scope = kube::core::NamespaceResourceScope>,
    K::DynamicType: Default,
{
    match target {
        None => kube::Api::all(client.clone()),
        Some(ns) => kube::Api::namespaced(client.clone(), ns),
    }
}

/// Owned copies of the scope's namespace targets: `[None]` cluster-wide, or
/// one `Some(namespace)` per watched namespace.
///
/// [`NamespaceScope::api_targets`] borrows the scope; a controller needs
/// targets it can move into one controller per namespace.
#[must_use]
pub fn owned_targets(scope: &NamespaceScope) -> Vec<Option<String>> {
    scope
        .api_targets()
        .into_iter()
        .map(|t| t.map(ToString::to_string))
        .collect()
}

/// How logs and metrics name the cluster-wide target (`None`).
pub const ALL_NAMESPACES_LABEL: &str = "<all>";

/// The label of a namespace target: the namespace, or
/// [`ALL_NAMESPACES_LABEL`] when cluster-wide.
///
/// # Arguments
/// * `target` - One of [`owned_targets`]'s targets, borrowed
///
/// # Returns
/// The namespace name, or `<all>`.
#[must_use]
pub fn target_label(target: Option<&str>) -> &str {
    target.unwrap_or(ALL_NAMESPACES_LABEL)
}

/// Run one controller per namespace target and report the first that ends.
///
/// A target controller runs until the shutdown fires, so returning at all is
/// judged as [`crate::shutdown::supervise`] judges a whole controller: an
/// error is a failure; `Ok` after the shutdown fired is a drain; `Ok` before
/// it is an unexpected exit, and an error. Built on `try_join_all`, so the
/// first failure or unexpected exit is returned at once, without waiting for
/// the other targets; the drains are awaited until every target has drained.
///
/// # Arguments
/// * `targets` - The namespace targets, normally [`owned_targets`]
/// * `shutdown` - The shared shutdown signal
/// * `run` - Builds the controller future for one target
///
/// # Returns
/// `Ok(())` once every target has drained after the shutdown fired.
///
/// # Errors
/// Returns the first target's error, with the target named, an error for the
/// first target that exits before the shutdown fired, or an error when there
/// are no targets (nothing would run).
pub async fn run_per_target<I, F, Fut>(
    targets: I,
    shutdown: &crate::shutdown::ShutdownSignal,
    mut run: F,
) -> anyhow::Result<()>
where
    I: IntoIterator<Item = Option<String>>,
    F: FnMut(Option<String>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let controllers: Vec<_> = targets
        .into_iter()
        .map(|target| {
            let label = target_label(target.as_deref()).to_string();
            let controller = run(target);
            async move {
                match controller.await {
                    Err(e) => {
                        tracing::error!("controller for namespace {label} failed: {e:#}");
                        Err(e.context(format!("controller for namespace {label} failed")))
                    }
                    Ok(()) if shutdown.is_triggered() => {
                        tracing::debug!("controller for namespace {label} drained");
                        Ok(())
                    }
                    Ok(()) => {
                        tracing::error!("controller for namespace {label} exited unexpectedly");
                        Err(anyhow::anyhow!(
                            "controller for namespace {label} exited unexpectedly"
                        ))
                    }
                }
            }
        })
        .collect();

    if controllers.is_empty() {
        return Err(anyhow::anyhow!(
            "no namespace targets to run a controller for"
        ));
    }

    futures::future::try_join_all(controllers).await.map(|_| ())
}

#[cfg(test)]
#[path = "namespace_scope_tests.rs"]
mod namespace_scope_tests;
