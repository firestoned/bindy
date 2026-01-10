// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Generic DNS record controller implementation.
//!
//! This module provides a generic controller pattern for all DNS record types,
//! eliminating code duplication across A, AAAA, TXT, CNAME, MX, NS, SRV, and CAA records.

use crate::bind9::Bind9Manager;
use crate::context::Context;
use crate::crd::{DNSZone, RecordStatus};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::Api;
use kube::core::NamespaceResourceScope;
use kube::runtime::controller::Action;
use kube::runtime::finalizer;
use kube::runtime::watcher::Config as WatcherConfig;
use kube::runtime::Controller;
use kube::{Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

/// Reconciliation error wrapper
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ReconcileError(#[from] anyhow::Error);

/// Error policy for record controllers.
///
/// Returns an action to requeue the resource after a delay when reconciliation fails.
#[allow(clippy::needless_pass_by_value)] // Signature required by kube::runtime::Controller
fn error_policy<T, C>(resource: Arc<T>, err: &ReconcileError, _ctx: Arc<C>) -> Action
where
    T: Debug,
{
    error!(
        error = %err,
        resource = ?resource,
        "Reconciliation error - will retry in {}s",
        crate::constants::ERROR_REQUEUE_DURATION_SECS
    );
    Action::requeue(Duration::from_secs(
        crate::constants::ERROR_REQUEUE_DURATION_SECS,
    ))
}

/// Trait for DNS record types that can be reconciled with a generic controller.
///
/// This trait abstracts over the common operations needed for all DNS record types,
/// allowing a single controller implementation to handle all record types.
pub trait DnsRecordType:
    Resource<DynamicType = (), Scope = NamespaceResourceScope>
    + Clone
    + Debug
    + DeserializeOwned
    + Serialize
    + Send
    + Sync
    + 'static
{
    /// The record type kind (e.g., `ARecord`, `TXTRecord`)
    const KIND: &'static str;

    /// The finalizer name for this record type
    const FINALIZER: &'static str;

    /// The DNS record type string (e.g., `A`, `TXT`)
    const RECORD_TYPE_STR: &'static str;

    /// Get the `hickory_client` `RecordType` value
    fn hickory_record_type() -> hickory_client::rr::RecordType;

    /// Reconcile this record (create/update in BIND9)
    fn reconcile_record(
        context: Arc<Context>,
        record: Self,
    ) -> impl std::future::Future<Output = Result<(), ReconcileError>> + Send;

    /// Get the metadata for this resource
    fn metadata(&self) -> &ObjectMeta;

    /// Get the status for this resource
    fn status(&self) -> &Option<RecordStatus>;
}

/// Run a generic DNS record controller.
///
/// This function creates a controller that watches both the record type and `DNSZone` resources,
/// triggering reconciliation when zones discover new records that need configuration.
///
/// # Arguments
///
/// * `context` - The controller context with API client and stores
/// * `bind9_manager` - The BIND9 manager for zone operations
///
/// # Errors
///
/// Returns an error if the controller fails to start or encounters a fatal error.
pub async fn run_generic_record_controller<T>(
    context: Arc<Context>,
    bind9_manager: Arc<Bind9Manager>,
) -> Result<()>
where
    T: DnsRecordType,
{
    info!("Starting {} controller", T::KIND);

    let client = context.client.clone();
    let api = Api::<T>::all(client.clone());
    let dnszone_api = Api::<DNSZone>::all(client.clone());

    // Configure controller to watch for ALL changes including status updates
    let watcher_config = WatcherConfig::default().any_semantic();

    // Create controller context tuple
    let ctx = Arc::new((context.clone(), bind9_manager));

    Controller::new(api, watcher_config.clone())
        .watches(dnszone_api, watcher_config, |zone| {
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
        .run(
            move |record: Arc<T>, ctx_clone: Arc<(Arc<Context>, Arc<Bind9Manager>)>| {
                reconcile_wrapper(record, ctx_clone)
            },
            error_policy,
            ctx,
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
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
    ctx: Arc<(Arc<Context>, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError>
where
    T: DnsRecordType,
{
    let start = std::time::Instant::now();
    let context = ctx.0.clone();
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

                info!("Successfully reconciled {}: {}", T::KIND, rec.name_any());

                // Re-fetch to get updated status
                let updated_record = api
                    .get(&rec.name_any())
                    .await
                    .map_err(|e| ReconcileError::from(anyhow::Error::from(e)))?;

                // Check readiness
                let is_ready = crate::record_wrappers::is_resource_ready(updated_record.status());

                Ok(crate::record_wrappers::requeue_based_on_readiness(is_ready))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                use crate::reconcilers::delete_record;

                delete_record(
                    &client,
                    &*rec,
                    T::RECORD_TYPE_STR,
                    T::hickory_record_type(),
                    &context.stores,
                )
                .await
                .map_err(ReconcileError::from)?;

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

    result.map_err(|e: finalizer::Error<ReconcileError>| match e {
        finalizer::Error::ApplyFailed(err) | finalizer::Error::CleanupFailed(err) => err,
        finalizer::Error::AddFinalizer(err) | finalizer::Error::RemoveFinalizer(err) => {
            ReconcileError::from(anyhow!("Finalizer error: {err}"))
        }
        finalizer::Error::UnnamedObject => ReconcileError::from(anyhow!("{} has no name", T::KIND)),
        finalizer::Error::InvalidFinalizer => {
            ReconcileError::from(anyhow!("Invalid finalizer for {}", T::KIND))
        }
    })
}
