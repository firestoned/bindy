// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Status management and event creation for DNS record resources.

#[allow(clippy::wildcard_imports)]
use super::types::*;

pub(super) async fn create_event<T>(
    client: &Client,
    record: &T,
    event_type: &str,
    reason: &str,
    message: &str,
) -> Result<()>
where
    T: Resource<DynamicType = ()> + ResourceExt,
{
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();
    let event_api: Api<Event> = Api::namespaced(client.clone(), &namespace);

    let now = Time(k8s_openapi::jiff::Timestamp::now());
    let event = Event {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            generate_name: Some(format!("{name}-")),
            namespace: Some(namespace.clone()),
            ..Default::default()
        },
        involved_object: ObjectReference {
            api_version: Some(T::api_version(&()).to_string()),
            kind: Some(T::kind(&()).to_string()),
            name: Some(name.clone()),
            namespace: Some(namespace),
            uid: record.meta().uid.clone(),
            ..Default::default()
        },
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
        type_: Some(event_type.to_string()),
        first_timestamp: Some(now.clone()),
        last_timestamp: Some(now),
        count: Some(1),
        ..Default::default()
    };

    match event_api.create(&PostParams::default(), &event).await {
        Ok(_) => Ok(()),
        Err(e) => {
            warn!("Failed to create event for {}: {}", name, e);
            Ok(()) // Don't fail reconciliation if event creation fails
        }
    }
}

/// What one reconcile wants a record's `Ready` condition and status to say.
///
/// `None` in an optional field means "leave the stored value alone": the
/// field is omitted from the merge patch, so the API server keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordStatusUpdate<'a> {
    /// `Ready` condition status: `"True"` or `"False"`
    pub status: &'a str,
    /// `CamelCase` reason, e.g. `ReconcileSucceeded`, `ZoneNotFound`
    pub reason: &'a str,
    /// Human-readable message
    pub message: &'a str,
    /// Generation to record as observed (defaults to the record's own)
    pub observed_generation: Option<i64>,
    /// Hash of the published spec, set on success
    pub record_hash: Option<String>,
    /// When the record was last published, set on success
    pub last_updated: Option<String>,
    /// Display addresses (A/AAAA), set on success
    pub addresses: Option<String>,
    /// DNS name just published (rename detection), set on success
    pub published_name: Option<String>,
}

impl<'a> RecordStatusUpdate<'a> {
    /// A `Ready=False` update that leaves every other status field alone.
    ///
    /// # Arguments
    ///
    /// * `reason` - `CamelCase` reason
    /// * `message` - Human-readable message
    /// * `observed_generation` - Generation to record as observed
    #[must_use]
    pub(crate) fn not_ready(
        reason: &'a str,
        message: &'a str,
        observed_generation: Option<i64>,
    ) -> Self {
        Self {
            status: CONDITION_FALSE,
            reason,
            message,
            observed_generation,
            record_hash: None,
            last_updated: None,
            addresses: None,
            published_name: None,
        }
    }
}

/// The record's `Ready` condition type.
const CONDITION_TYPE_READY: &str = "Ready";

/// Condition status for a satisfied condition.
const CONDITION_TRUE: &str = "True";

/// Condition status for an unsatisfied condition.
const CONDITION_FALSE: &str = "False";

/// Event type for a record that reached `Ready=True`.
const EVENT_TYPE_NORMAL: &str = "Normal";

/// Event type for any other outcome.
const EVENT_TYPE_WARNING: &str = "Warning";

/// The status of a record as the reconcile received it (the watch cache).
///
/// # Arguments
///
/// * `record` - The record being reconciled
///
/// # Returns
///
/// The parsed `status`, or `None` when the record has none (or it does not
/// parse as a [`RecordStatus`]).
#[must_use]
pub(crate) fn cached_record_status<T: serde::Serialize>(record: &T) -> Option<RecordStatus> {
    let json = serde_json::to_value(record).ok()?;
    let status = json.get("status")?.clone();
    serde_json::from_value(status).ok()
}

/// Build the status merge patch for `update`, or `None` when the stored status
/// already says it.
///
/// Decides from `current`, the status the reconcile already holds, never from
/// a fresh GET (ADR-0016). The patch carries the `Ready` condition, the
/// observed generation and only the optional fields `update` sets. It never
/// carries `zone` or `zoneRef`: the `DNSZone` controller owns those, and a
/// merge patch that omits them cannot overwrite them.
///
/// The update is skipped when `current` was observed at `record_generation`
/// and its `Ready` condition already has the same status, reason and message.
/// The condition's `lastTransitionTime` is kept while its status value holds
/// and set to `now` when it flips.
///
/// # Arguments
///
/// * `current` - The cached status, if any
/// * `record_generation` - The record's `metadata.generation`
/// * `update` - What the reconcile wants the status to say
/// * `now` - RFC 3339 timestamp for a new transition
///
/// # Returns
///
/// `Some(patch)` to send, or `None` when nothing would change.
#[must_use]
pub(crate) fn record_status_patch(
    current: Option<&RecordStatus>,
    record_generation: Option<i64>,
    update: &RecordStatusUpdate<'_>,
    now: &str,
) -> Option<serde_json::Value> {
    let existing = current.and_then(|status| {
        status
            .conditions
            .iter()
            .find(|condition| condition.r#type == CONDITION_TYPE_READY)
    });

    let observed_current = current.and_then(|status| status.observed_generation);
    let unchanged = observed_current.is_some()
        && observed_current == record_generation
        && existing.is_some_and(|condition| {
            condition.status == update.status
                && condition.reason.as_deref() == Some(update.reason)
                && condition.message.as_deref() == Some(update.message)
        });
    if unchanged {
        return None;
    }

    let last_transition_time = existing
        .filter(|condition| condition.status == update.status)
        .and_then(|condition| condition.last_transition_time.clone())
        .unwrap_or_else(|| now.to_string());

    let condition = Condition {
        r#type: CONDITION_TYPE_READY.to_string(),
        status: update.status.to_string(),
        reason: Some(update.reason.to_string()),
        message: Some(update.message.to_string()),
        last_transition_time: Some(last_transition_time),
    };

    // zone and zoneRef stay None, so they are not serialized: the patch can
    // never clobber what the DNSZone controller wrote.
    #[allow(deprecated)] // the deprecated `zone` field is deliberately left out
    let status = RecordStatus {
        conditions: vec![condition],
        observed_generation: update.observed_generation.or(record_generation),
        zone: None,
        zone_ref: None,
        record_hash: update.record_hash.clone(),
        last_updated: update.last_updated.clone(),
        addresses: update.addresses.clone(),
        published_name: update.published_name.clone(),
    };

    Some(json!({ "status": status }))
}

/// Updates the status of a DNS record resource.
///
/// Builds the patch with [`record_status_patch`] from the record the
/// reconcile already holds (no GET), sends it as a merge patch to the status
/// subresource when something changed, and records a Kubernetes Event for the
/// new condition.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource, as the reconcile received it
/// * `update` - What the status should say
///
/// # Errors
///
/// Returns an error if the status patch fails.
pub(crate) async fn update_record_status<T>(
    client: &Client,
    record: &T,
    update: &RecordStatusUpdate<'_>,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let current = cached_record_status(record);
    let Some(status_patch) = record_status_patch(
        current.as_ref(),
        record.meta().generation,
        update,
        &Utc::now().to_rfc3339(),
    ) else {
        // Status is already correct; skip the write so it cannot wake anything.
        return Ok(());
    };

    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();
    let api: Api<T> = Api::namespaced(client.clone(), &namespace);
    api.patch_status(&name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .context("Failed to update record status")?;

    debug!(
        "Updated status for {}/{}: {} = {}",
        namespace, name, CONDITION_TYPE_READY, update.status
    );

    let event_type = if update.status == CONDITION_TRUE {
        EVENT_TYPE_NORMAL
    } else {
        EVENT_TYPE_WARNING
    };
    create_event(client, record, event_type, update.reason, update.message).await?;

    Ok(())
}

#[cfg(test)]
#[path = "status_helpers_tests.rs"]
mod status_helpers_tests;
