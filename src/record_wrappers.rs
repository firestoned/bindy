// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Record reconciliation wrapper helpers and macro.
//!
//! This module provides helper functions and a macro to generate reconciliation
//! wrapper functions for all DNS record types, eliminating ~900 lines of duplicate code.

use crate::crd::RecordStatus;
use kube::runtime::controller::Action;
use std::time::Duration;

/// Requeue interval for resources that are ready (5 minutes)
pub const REQUEUE_WHEN_READY_SECS: u64 = 300;

/// Requeue interval for resources that are not ready (30 seconds)
pub const REQUEUE_WHEN_NOT_READY_SECS: u64 = 30;

/// Condition type for resource readiness
pub const CONDITION_TYPE_READY: &str = "Ready";

/// Condition status indicating ready state
pub const CONDITION_STATUS_TRUE: &str = "True";

/// Error type label for reconciliation errors
pub const ERROR_TYPE_RECONCILE: &str = "reconcile_error";

/// Check if a resource with status conditions is ready.
///
/// A resource is considered ready if it has a status with at least one condition
/// where type="Ready" and status="True".
///
/// # Arguments
///
/// * `status` - Optional status containing conditions
///
/// # Returns
///
/// `true` if the resource is ready, `false` otherwise
#[must_use]
pub fn is_resource_ready(status: &Option<RecordStatus>) -> bool {
    status.as_ref().is_some_and(|s| {
        s.conditions.first().is_some_and(|condition| {
            condition.r#type == CONDITION_TYPE_READY && condition.status == CONDITION_STATUS_TRUE
        })
    })
}

/// Determine requeue action based on readiness status.
///
/// # Arguments
///
/// * `is_ready` - Whether the resource is ready
///
/// # Returns
///
/// * `Action::requeue(5 minutes)` if ready
/// * `Action::requeue(30 seconds)` if not ready
#[must_use]
pub fn requeue_based_on_readiness(is_ready: bool) -> Action {
    if is_ready {
        Action::requeue(Duration::from_secs(REQUEUE_WHEN_READY_SECS))
    } else {
        Action::requeue(Duration::from_secs(REQUEUE_WHEN_NOT_READY_SECS))
    }
}

/// Macro to generate record reconciliation wrapper functions.
///
/// This eliminates ~900 lines of duplicate code by generating identical wrappers
/// for all 8 DNS record types with only the type and constant names changing.
///
/// # Generated Function Pattern
///
/// For each record type, generates an async function that:
/// 1. Tracks reconciliation timing
/// 2. Calls the type-specific reconcile function
/// 3. Records metrics (success/error)
/// 4. Checks resource readiness status
/// 5. Returns appropriate requeue action
///
/// # Example
///
/// ```ignore
/// generate_record_wrapper!(
///     reconcile_arecord_wrapper,  // Function name
///     ARecord,                     // Record type
///     reconcile_a_record,          // Reconcile function
///     KIND_A_RECORD,               // Metrics constant
///     "ARecord"                    // Display name
/// );
/// ```
#[macro_export]
macro_rules! generate_record_wrapper {
    ($wrapper_fn:ident, $record_type:ty, $reconcile_fn:path, $kind_const:path, $display_name:expr) => {
        pub async fn $wrapper_fn(
            record: ::std::sync::Arc<$record_type>,
            ctx: ::std::sync::Arc<(
                ::kube::Client,
                ::std::sync::Arc<$crate::bind9::Bind9Manager>,
            )>,
        ) -> ::std::result::Result<::kube::runtime::controller::Action, ReconcileError> {
            let start = ::std::time::Instant::now();

            let result = $reconcile_fn(ctx.0.clone(), (*record).clone()).await;
            let duration = start.elapsed();

            match result {
                Ok(()) => {
                    ::tracing::info!(
                        "Successfully reconciled {}: {}",
                        $display_name,
                        ::kube::ResourceExt::name_any(&*record)
                    );
                    $crate::metrics::record_reconciliation_success($kind_const, duration);

                    // Fetch the latest status to check if record is ready
                    let namespace = ::kube::ResourceExt::namespace(&*record).unwrap_or_default();
                    let name = ::kube::ResourceExt::name_any(&*record);
                    let api: ::kube::Api<$record_type> =
                        ::kube::Api::namespaced(ctx.0.clone(), &namespace);

                    let is_ready = if let Ok(updated_record) = api.get(&name).await {
                        $crate::record_wrappers::is_resource_ready(&updated_record.status)
                    } else {
                        false
                    };

                    Ok($crate::record_wrappers::requeue_based_on_readiness(
                        is_ready,
                    ))
                }
                Err(e) => {
                    ::tracing::error!("Failed to reconcile {}: {}", $display_name, e);
                    $crate::metrics::record_reconciliation_error($kind_const, duration);
                    $crate::metrics::record_error(
                        $kind_const,
                        $crate::record_wrappers::ERROR_TYPE_RECONCILE,
                    );
                    Err(e.into())
                }
            }
        }
    };
}
