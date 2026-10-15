// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! The Secret audit trail (ADR-0024).
//!
//! Every create, replace, patch, delete or read of a Secret's data that bindy
//! performs emits exactly one structured event on the [`AUDIT_TARGET`]
//! tracing target, after the API call returns, and increments
//! [`crate::metrics::SECRET_OPERATIONS_TOTAL`]. Go through [`audited`], which
//! performs the call and records whichever outcome it returns, so a call site
//! cannot record success and forget failure.
//!
//! No field ever carries Secret data, a key, a token, or an API error message
//! (which can quote request content): a failure is summarised by the API
//! status code and reason only ([`kube_error_summary`]).

use std::future::Future;

/// The tracing target every audit event is emitted on.
pub const AUDIT_TARGET: &str = "bindy::audit";

/// The filter directive that keeps the audit trail on (ADR-0024 Decision 4).
const AUDIT_FILTER_DIRECTIVE: &str = "bindy::audit=info";

/// The filter used when `RUST_LOG` is unset or empty.
const DEFAULT_LOG_FILTER: &str = "info";

/// What was done to the Secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretOperation {
    /// The Secret was created.
    Create,
    /// The whole Secret was replaced.
    Replace,
    /// Part of the Secret was patched (metadata, or a server-side apply).
    Patch,
    /// The Secret was deleted.
    Delete,
    /// The Secret's data was read.
    Read,
}

impl SecretOperation {
    /// The value written to the `operation` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
            Self::Patch => "patch",
            Self::Delete => "delete",
            Self::Read => "read",
        }
    }
}

/// What the Secret is to bindy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretPurpose {
    /// An instance's operator-generated `{instance}-rndc-key`.
    RndcKey,
    /// An instance's `{instance}-rndc-key` mirrored from a `secretRef`
    /// (ADR-0023).
    RndcKeyMirror,
    /// The user Secret a `rndcKey.secretRef` names.
    RndcKeySource,
    /// A CA bundle used to verify a bindcar TLS endpoint.
    BindcarCa,
    /// The remote cluster kubeconfig `bindy scout` reads.
    ScoutKubeconfig,
    /// A `ServiceAccount` token Secret `bindy bootstrap` writes and reads.
    ServiceAccountToken,
}

impl SecretPurpose {
    /// The value written to the `purpose` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RndcKey => "rndc-key",
            Self::RndcKeyMirror => "rndc-key-mirror",
            Self::RndcKeySource => "rndc-key-source",
            Self::BindcarCa => "bindcar-ca",
            Self::ScoutKubeconfig => "scout-kubeconfig",
            Self::ServiceAccountToken => "serviceaccount-token",
        }
    }
}

/// Why the operation happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditReason {
    /// The Secret did not exist yet.
    Initial,
    /// Scheduled key rotation, or the rotation bookkeeping on the Secret.
    Rotation,
    /// A `secretRef` mirror following its source (ADR-0023).
    MirrorSync,
    /// An existing key read to check it before reuse.
    Verify,
    /// A generated key recreated because the configured algorithm changed.
    RecreateAlgorithm,
    /// A generated key recreated because the Secret was malformed.
    RecreateMalformed,
    /// The instance switched between a generated key and a `secretRef`.
    ModeSwitch,
    /// The owning instance is being deleted.
    InstanceDeleted,
    /// A key loaded to sign a DNS UPDATE or call bindcar.
    KeyLoad,
    /// A Secret read to build a client (TLS trust, a remote kubeconfig).
    ClientBuild,
    /// `bindy bootstrap mc revoke` removing the access it granted.
    Revoke,
}

impl AuditReason {
    /// The value written to the `reason` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Rotation => "rotation",
            Self::MirrorSync => "mirror-sync",
            Self::Verify => "verify",
            Self::RecreateAlgorithm => "recreate-algorithm",
            Self::RecreateMalformed => "recreate-malformed",
            Self::ModeSwitch => "mode-switch",
            Self::InstanceDeleted => "instance-deleted",
            Self::KeyLoad => "key-load",
            Self::ClientBuild => "client-build",
            Self::Revoke => "revoke",
        }
    }
}

/// The component that performed the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    /// The `Bind9Instance` controller.
    Bind9Instance,
    /// The `Bind9Cluster` controller.
    Bind9Cluster,
    /// `bindy scout`.
    Scout,
    /// `bindy bootstrap`.
    Bootstrap,
    /// The BIND9 domain layer (`bindy-bind9`: key loader, TLS client).
    Bind9,
}

impl Actor {
    /// The value written to the `actor` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bind9Instance => "bind9instance",
            Self::Bind9Cluster => "bind9cluster",
            Self::Scout => "scout",
            Self::Bootstrap => "bootstrap",
            Self::Bind9 => "bindy-bind9",
        }
    }
}

/// One Secret operation to audit.
#[derive(Clone, Debug)]
pub struct SecretAudit {
    /// What is done
    pub operation: SecretOperation,
    /// What the Secret is to bindy
    pub purpose: SecretPurpose,
    /// Why it is done
    pub reason: AuditReason,
    /// Who does it
    pub actor: Actor,
    /// The Secret's namespace
    pub namespace: String,
    /// The Secret's name
    pub name: String,
    /// The object the operation serves, e.g. `Bind9Instance dns/primary-0`
    pub owner: Option<String>,
}

impl SecretAudit {
    /// Emits the audit event for `result` and counts it.
    pub fn record<T>(&self, result: &Result<T, kube::Error>) {
        let outcome = if result.is_ok() { "success" } else { "failure" };
        crate::metrics::SECRET_OPERATIONS_TOTAL
            .with_label_values(&[self.operation.as_str(), self.purpose.as_str(), outcome])
            .inc();
        let owner = self.owner.as_deref().unwrap_or_default();
        match result {
            Ok(_) => tracing::info!(
                target: AUDIT_TARGET,
                kind = "audit",
                resource = "Secret",
                operation = self.operation.as_str(),
                outcome,
                namespace = %self.namespace,
                name = %self.name,
                purpose = self.purpose.as_str(),
                reason = self.reason.as_str(),
                actor = self.actor.as_str(),
                owner,
                "Secret operation"
            ),
            Err(e) => tracing::warn!(
                target: AUDIT_TARGET,
                kind = "audit",
                resource = "Secret",
                operation = self.operation.as_str(),
                outcome,
                error = %kube_error_summary(e),
                namespace = %self.namespace,
                name = %self.name,
                purpose = self.purpose.as_str(),
                reason = self.reason.as_str(),
                actor = self.actor.as_str(),
                owner,
                "Secret operation failed"
            ),
        }
    }
}

/// Performs a Secret API call and records its outcome (ADR-0024).
///
/// # Arguments
///
/// * `audit` - What the call is
/// * `call` - The Kubernetes API call
///
/// # Errors
///
/// Returns the call's own error, unchanged.
pub async fn audited<T, F>(audit: SecretAudit, call: F) -> Result<T, kube::Error>
where
    F: Future<Output = Result<T, kube::Error>>,
{
    let result = call.await;
    audit.record(&result);
    result
}

/// The purpose, reason and owner of one Secret operation.
#[derive(Clone, Debug)]
pub struct SecretUse {
    /// What the Secret is to bindy
    pub purpose: SecretPurpose,
    /// Why the operation happens
    pub reason: AuditReason,
    /// The object the operation serves, e.g. `Bind9Instance dns/primary-0`
    pub owner: Option<String>,
}

impl SecretUse {
    /// A use with no owning object.
    #[must_use]
    pub fn new(purpose: SecretPurpose, reason: AuditReason) -> Self {
        Self {
            purpose,
            reason,
            owner: None,
        }
    }

    /// The same use, serving `owner` (e.g. `Bind9Instance dns/primary-0`).
    #[must_use]
    pub fn owned_by(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }
}

/// The only way bindy touches a Secret (ADR-0024 Decision 2): an
/// `Api<Secret>` whose data methods take the operation's [`SecretUse`] and
/// record it, and whose metadata methods return no data and record nothing.
///
/// A test (`no_code_outside_the_audit_module_builds_a_secret_api`) fails when
/// other non-test code names `Api<Secret>`; an `Api` whose `Secret` type is
/// only inferred escapes that text scan, so review still checks new
/// `Api::namespaced` calls next to a `Secret`.
#[derive(Clone)]
pub struct SecretApi {
    api: kube::Api<k8s_openapi::api::core::v1::Secret>,
    namespace: String,
    actor: Actor,
}

impl SecretApi {
    /// Secrets in `namespace`, used by `actor`.
    #[must_use]
    pub fn namespaced(client: kube::Client, namespace: &str, actor: Actor) -> Self {
        Self {
            api: kube::Api::namespaced(client, namespace),
            namespace: namespace.to_string(),
            actor,
        }
    }

    fn audit(&self, operation: SecretOperation, name: &str, secret_use: &SecretUse) -> SecretAudit {
        SecretAudit {
            operation,
            purpose: secret_use.purpose,
            reason: secret_use.reason,
            actor: self.actor,
            namespace: self.namespace.clone(),
            name: name.to_string(),
            owner: secret_use.owner.clone(),
        }
    }

    /// Reads a Secret, data included. Audited.
    ///
    /// # Errors
    /// Returns the API error, a 404 included.
    pub async fn get(
        &self,
        name: &str,
        secret_use: &SecretUse,
    ) -> Result<k8s_openapi::api::core::v1::Secret, kube::Error> {
        audited(
            self.audit(SecretOperation::Read, name, secret_use),
            self.api.get(name),
        )
        .await
    }

    /// Reads a Secret if it exists. Audited when it does or the call fails;
    /// a Secret that does not exist gives up no key and records nothing.
    ///
    /// # Errors
    /// Returns the API error for anything but a 404.
    pub async fn get_opt(
        &self,
        name: &str,
        secret_use: &SecretUse,
    ) -> Result<Option<k8s_openapi::api::core::v1::Secret>, kube::Error> {
        let result = self.api.get_opt(name).await;
        if !matches!(result, Ok(None)) {
            self.audit(SecretOperation::Read, name, secret_use)
                .record(&result);
        }
        result
    }

    /// Reads a Secret's metadata only: no data, not audited.
    ///
    /// # Errors
    /// Returns the API error for anything but a 404.
    pub async fn get_metadata_opt(
        &self,
        name: &str,
    ) -> Result<
        Option<kube::core::PartialObjectMeta<k8s_openapi::api::core::v1::Secret>>,
        kube::Error,
    > {
        self.api.get_metadata_opt(name).await
    }

    /// Creates a Secret. Audited.
    ///
    /// # Errors
    /// Returns the API error.
    pub async fn create(
        &self,
        secret: &k8s_openapi::api::core::v1::Secret,
        secret_use: &SecretUse,
    ) -> Result<k8s_openapi::api::core::v1::Secret, kube::Error> {
        let name = secret.metadata.name.clone().unwrap_or_default();
        audited(
            self.audit(SecretOperation::Create, &name, secret_use),
            self.api.create(&kube::api::PostParams::default(), secret),
        )
        .await
    }

    /// Replaces a Secret. Audited.
    ///
    /// # Errors
    /// Returns the API error.
    pub async fn replace(
        &self,
        name: &str,
        secret: &k8s_openapi::api::core::v1::Secret,
        secret_use: &SecretUse,
    ) -> Result<k8s_openapi::api::core::v1::Secret, kube::Error> {
        audited(
            self.audit(SecretOperation::Replace, name, secret_use),
            self.api
                .replace(name, &kube::api::PostParams::default(), secret),
        )
        .await
    }

    /// Patches a Secret (metadata, or a server-side apply). Audited.
    ///
    /// # Errors
    /// Returns the API error.
    pub async fn patch<P: serde::Serialize + std::fmt::Debug>(
        &self,
        name: &str,
        params: &kube::api::PatchParams,
        patch: &kube::api::Patch<P>,
        secret_use: &SecretUse,
    ) -> Result<k8s_openapi::api::core::v1::Secret, kube::Error> {
        audited(
            self.audit(SecretOperation::Patch, name, secret_use),
            self.api.patch(name, params, patch),
        )
        .await
    }

    /// Deletes a Secret. Audited.
    ///
    /// # Errors
    /// Returns the API error, a 404 included.
    pub async fn delete(&self, name: &str, secret_use: &SecretUse) -> Result<(), kube::Error> {
        audited(
            self.audit(SecretOperation::Delete, name, secret_use),
            async {
                self.api
                    .delete(name, &kube::api::DeleteParams::default())
                    .await
                    .map(|_| ())
            },
        )
        .await
    }
}

/// A Kubernetes client error, summarised without its message: the API status
/// code and reason, or the error's category.
#[must_use]
pub fn kube_error_summary(e: &kube::Error) -> String {
    match e {
        kube::Error::Api(status) => format!("{} {}", status.code, status.reason),
        kube::Error::Service(_) => "service error".to_string(),
        kube::Error::HyperError(_) | kube::Error::HttpError(_) => "transport error".to_string(),
        kube::Error::SerdeError(_) => "decode error".to_string(),
        kube::Error::Auth(_) => "auth error".to_string(),
        _ => "client error".to_string(),
    }
}

/// The log filter to install: `RUST_LOG` (or `info` when it is unset or
/// empty), with [`AUDIT_TARGET`] at `info` added unless `RUST_LOG` already
/// names it (ADR-0024 Decision 4). An explicit `bindy::audit=off` is kept.
#[must_use]
pub fn audit_log_filter(rust_log: Option<&str>) -> String {
    let base = rust_log
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LOG_FILTER);
    if base.contains(AUDIT_TARGET) {
        return base.to_string();
    }
    format!("{base},{AUDIT_FILTER_DIRECTIVE}")
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;
