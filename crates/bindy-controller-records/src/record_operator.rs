// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Generic DNS record operator implementation.
//!
//! This module provides a generic operator pattern for all DNS record types,
//! eliminating code duplication across A, AAAA, TXT, CNAME, MX, NS, SRV, and CAA records.

use crate::context::Context;
use crate::crd::{DNSZone, RecordStatus};
use crate::record_wrappers::ReadyState;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::Api;
use kube::runtime::controller::Action;
use kube::runtime::finalizer;
use kube::runtime::Controller;
use kube::ResourceExt;
use serde::Serialize;
use std::sync::Arc;
use tracing::{info, warn};

use bindy_controller_sdk::error::error_policy;
pub use bindy_controller_sdk::error::ReconcileError;

/// Trait for DNS record types that can be reconciled with a generic operator.
///
/// This trait abstracts over the common operations needed for all DNS record types,
/// allowing a single operator implementation to handle all record types.
/// The kind name comes from the supertrait
/// [`RecordKind`](crate::context::RecordKind), so each record type declares it
/// once for the stores, the watch layer and this controller; the BIND9 write
/// comes from [`ReconcilableRecord`](bindy_bind9::record_push::ReconcilableRecord).
pub trait DnsRecordType:
    crate::context::RecordKind + bindy_bind9::record_push::ReconcilableRecord + Serialize
{
    /// The finalizer name for this record type
    const FINALIZER: &'static str;

    /// The DNS record type string (e.g., `A`, `TXT`)
    const RECORD_TYPE_STR: &'static str;

    /// Get the `hickory_proto` `RecordType` value
    fn hickory_record_type() -> hickory_proto::rr::RecordType;

    /// Reconcile this record (create/update in BIND9).
    ///
    /// Every kind goes through the one generic path
    /// ([`crate::records::reconcile_record`]), so this default is the only
    /// implementation; the nine per-kind `reconcile_*_record` wrappers that used
    /// to sit in front of it are gone (roadmap 01 Phase D).
    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send {
        async move {
            crate::records::reconcile_record(context, record)
                .await
                .map_err(ReconcileError::from)
        }
    }

    /// Get the metadata for this resource
    fn metadata(&self) -> &ObjectMeta;

    /// Get the status for this resource
    fn status(&self) -> &Option<RecordStatus>;
}

/// Run a generic DNS record operator.
///
/// This function creates an operator that watches both the record type and `DNSZone` resources,
/// triggering reconciliation when zones discover new records that need configuration.
///
/// # Arguments
///
/// * `context` - The operator context with API client and stores
///
/// # Errors
///
/// Returns an error if the operator fails to start or encounters a fatal error.
pub async fn run_generic_record_operator<T>(context: Arc<Context>) -> Result<()>
where
    T: DnsRecordType,
{
    info!("Starting {} operator", T::KIND);

    // Record kinds are namespaced: one controller per watched namespace, all sharing
    // the reconciler and context. Cluster-wide mode yields exactly one.
    let targets = bindy_controller_sdk::namespace_scope::owned_targets(&context.namespace_scope);
    futures::future::join_all(
        targets
            .into_iter()
            .map(|target| run_generic_record_controller::<T>(context.clone(), target)),
    )
    .await;

    Ok(())
}

/// Run the record controller for one record kind in a single namespace target.
///
/// `target` is `None` for cluster-wide, or `Some(namespace)`.
async fn run_generic_record_controller<T>(context: Arc<Context>, target: Option<String>)
where
    T: DnsRecordType,
{
    tracing::debug!(
        kind = T::KIND,
        namespace = target.as_deref().unwrap_or("<all>"),
        "Starting record controller"
    );

    // The record kind and DNSZone streams come from the shared WatchSet, which
    // forwards every change, status updates included (ADR-0009 §3).
    let ws = context.watch.clone();
    let target = target.as_deref();

    Controller::for_stream(ws.subscribe::<T>(target), ws.store::<T>(target))
        .watches_stream(ws.subscribe::<DNSZone>(target), |zone| {
            // When DNSZone.status.records[] changes, trigger reconciliation
            // for records that have lastReconciledAt == None (need configuration).
            let Some(namespace) = zone.namespace() else {
                return vec![];
            };

            // Get records from zone.status.records[] that need reconciliation
            let empty_vec = Vec::new();
            let records = zone.status.as_ref().map_or(&empty_vec, |s| &s.records);

            records
                .iter()
                .filter(|record_ref| {
                    // Only reconcile records of this type with lastReconciledAt == None
                    record_ref.kind == T::KIND
                        && record_ref.last_reconciled_at.is_none()
                        && record_ref.namespace == namespace
                })
                .map(|record_ref| {
                    kube::runtime::reflector::ObjectRef::new(&record_ref.name)
                        .within(&record_ref.namespace)
                })
                .collect::<Vec<_>>()
        })
        .graceful_shutdown_on(context.shutdown.wait())
        .run(reconcile_wrapper::<T>, error_policy, context.clone())
        .for_each(|_| futures::future::ready(()))
        .await;
}

/// Generic reconciliation wrapper with finalizer support.
///
/// This function handles the common reconciliation pattern for all DNS record types:
/// 1. Finalizer management (add on apply, remove on cleanup)
/// 2. Reconciliation logic (create/update or delete)
/// 3. Metrics recording
/// 4. Error handling
async fn reconcile_wrapper<T>(
    record: Arc<T>,
    context: Arc<Context>,
) -> Result<Action, ReconcileError>
where
    T: DnsRecordType,
{
    let start = std::time::Instant::now();

    let client = context.client.clone();
    let namespace = record
        .metadata()
        .namespace
        .as_ref()
        .ok_or_else(|| ReconcileError::from(anyhow!("{} has no namespace", T::KIND)))?;
    let api: Api<T> = Api::namespaced(client.clone(), namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, T::FINALIZER, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                T::reconcile_record(context.clone(), (*rec).clone()).await?;

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // A failed BIND9 write is reported through the record's own
                // conditions, not through the return value above: the reconcile
                // swallows it so the status can be written. The outcome therefore
                // has to be read back before this can claim the record was
                // published, or the log contradicts the status it just set.
                let state = crate::record_wrappers::ready_state(updated_record.status());
                match state {
                    ReadyState::Ready => {
                        info!("Successfully reconciled {}: {}", T::KIND, rec.name_any());
                    }
                    ReadyState::NotReady { reason, message } => {
                        warn!(
                            "Reconciled {} {} but it is not Ready — {reason}: {message}",
                            T::KIND,
                            rec.name_any()
                        );
                    }
                    ReadyState::Unknown => {
                        warn!(
                            "Reconciled {} {} but it reports no Ready condition",
                            T::KIND,
                            rec.name_any()
                        );
                    }
                }

                Ok(crate::record_wrappers::requeue_based_on_readiness(
                    matches!(state, ReadyState::Ready),
                ))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use crate::records::delete_record;

                delete_record(
                    &client,
                    &*rec,
                    T::RECORD_TYPE_STR,
                    T::hickory_record_type(),
                    &context.stores,
                )
                .await
                .map_err(ReconcileError::from)?;

                // The object is gone; a recreated one must not inherit its
                // rejection cooldown.
                bindy_controller_sdk::retry::clear_rejected_write(&format!(
                    "{}/{}/{}",
                    T::KIND,
                    rec.namespace().unwrap_or_default(),
                    rec.name_any()
                ));

                info!(
                    "Successfully deleted {} from BIND9: {}",
                    T::KIND,
                    rec.name_any()
                );
                crate::metrics::record_resource_deleted(T::KIND);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        crate::metrics::record_reconciliation_success(T::KIND, duration);
    } else {
        crate::metrics::record_reconciliation_error(T::KIND, duration);
        crate::metrics::record_error(T::KIND, crate::record_wrappers::ERROR_TYPE_RECONCILE);
    }

    result.map_err(|e| bindy_controller_sdk::reconcile::finalizer_error(T::KIND, e))
}
