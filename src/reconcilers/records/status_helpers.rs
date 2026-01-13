// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

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

    let now = Time(Utc::now());
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

/// Updates the status of a DNS record resource.
///
/// Updates the status subresource with appropriate conditions following
/// Kubernetes conventions. Also creates a Kubernetes Event for visibility.
///
/// # Arguments
///
/// * `client` - Kubernetes API client
/// * `record` - The DNS record resource to update
/// * `condition_type` - Type of condition (e.g., "Ready", "Failed")
/// * `status` - Status value (e.g., "True", "False", "Unknown")
/// * `reason` - Short reason code (e.g., "`ReconcileSucceeded`", "`ZoneNotFound`")
/// * `message` - Human-readable message describing the status
/// * `observed_generation` - Optional generation to set in status (defaults to record's current generation)
/// * `record_hash` - Optional hash of the record spec for change detection
/// * `last_updated` - Optional timestamp of last update
///
/// # Errors
///
/// Returns an error if the status update fails.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub(super) async fn update_record_status<T>(
    client: &Client,
    record: &T,
    condition_type: &str,
    status: &str,
    reason: &str,
    message: &str,
    observed_generation: Option<i64>,
    record_hash: Option<String>,
    last_updated: Option<String>,
) -> Result<()>
where
    T: Resource<DynamicType = (), Scope = k8s_openapi::NamespaceResourceScope>
        + ResourceExt
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>,
{
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();
    let api: Api<T> = Api::namespaced(client.clone(), &namespace);

    // Fetch current resource to check existing status
    let current = api
        .get(&name)
        .await
        .context("Failed to fetch current resource")?;

    // Check if we need to update
    // Extract status from the current resource using json
    let current_json = serde_json::to_value(&current)?;
    let needs_update = if let Some(current_status) = current_json.get("status") {
        if let Some(observed_gen) = current_status.get("observedGeneration") {
            // If observed generation matches current generation and condition hasn't changed, skip update
            if observed_gen == &json!(record.meta().generation) {
                if let Some(conditions) =
                    current_status.get("conditions").and_then(|c| c.as_array())
                {
                    // Find the condition with matching type (not just first condition)
                    let matching_condition = conditions.iter().find(|cond| {
                        cond.get("type").and_then(|t| t.as_str()) == Some(condition_type)
                    });

                    if let Some(cond) = matching_condition {
                        let status_matches =
                            cond.get("status").and_then(|s| s.as_str()) == Some(status);
                        let reason_matches =
                            cond.get("reason").and_then(|r| r.as_str()) == Some(reason);
                        let message_matches =
                            cond.get("message").and_then(|m| m.as_str()) == Some(message);
                        // Only update if any field has changed
                        !(status_matches && reason_matches && message_matches)
                    } else {
                        true // Condition type not found, need to add it
                    }
                } else {
                    true // No conditions array, need to update
                }
            } else {
                true // Generation changed, need to update
            }
        } else {
            true // No observed generation, need to update
        }
    } else {
        true // No status, need to update
    };

    if !needs_update {
        // Status is already correct, skip update to avoid reconciliation loop
        return Ok(());
    }

    // Determine last_transition_time
    let last_transition_time = if let Some(current_status) = current_json.get("status") {
        if let Some(conditions) = current_status.get("conditions").and_then(|c| c.as_array()) {
            // Find the condition with matching type (same as above)
            let matching_condition = conditions
                .iter()
                .find(|cond| cond.get("type").and_then(|t| t.as_str()) == Some(condition_type));

            if let Some(cond) = matching_condition {
                let status_changed = cond.get("status").and_then(|s| s.as_str()) != Some(status);
                if status_changed {
                    // Status changed, use current time
                    Utc::now().to_rfc3339()
                } else {
                    // Status unchanged, preserve existing timestamp
                    cond.get("lastTransitionTime")
                        .and_then(|t| t.as_str())
                        .unwrap_or(&Utc::now().to_rfc3339())
                        .to_string()
                }
            } else {
                // Condition type not found, use current time
                Utc::now().to_rfc3339()
            }
        } else {
            Utc::now().to_rfc3339()
        }
    } else {
        Utc::now().to_rfc3339()
    };

    let condition = Condition {
        r#type: condition_type.to_string(),
        status: status.to_string(),
        reason: Some(reason.to_string()),
        message: Some(message.to_string()),
        last_transition_time: Some(last_transition_time),
    };

    // Preserve existing zone field if it exists (set by DNSZone controller)
    let zone = current_json
        .get("status")
        .and_then(|s| s.get("zone"))
        .and_then(|z| z.as_str())
        .map(ToString::to_string);

    // Preserve existing zone_ref field if it exists (set by DNSZone controller)
    let zone_ref = current_json
        .get("status")
        .and_then(|s| s.get("zoneRef"))
        .and_then(|z| serde_json::from_value::<crate::crd::ZoneReference>(z.clone()).ok());

    #[allow(deprecated)] // Maintain backward compatibility with deprecated zone field
    let record_status = RecordStatus {
        conditions: vec![condition],
        observed_generation: observed_generation.or(record.meta().generation),
        zone,
        zone_ref, // Preserved from existing status (set by DNSZone controller)
        record_hash,
        last_updated,
    };

    let status_patch = json!({
        "status": record_status
    });

    api.patch_status(&name, &PatchParams::default(), &Patch::Merge(&status_patch))
        .await
        .context("Failed to update record status")?;

    info!(
        "Updated status for {}/{}: {} = {}",
        namespace, name, condition_type, status
    );

    // Create event for visibility
    let event_type = if status == "True" {
        "Normal"
    } else {
        "Warning"
    };
    create_event(client, record, event_type, reason, message).await?;

    Ok(())
}
