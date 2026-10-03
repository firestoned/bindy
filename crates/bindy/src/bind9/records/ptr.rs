// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! PTR record management.

use super::super::types::{PTRRecordData, RndcKeyData};
use super::{
    build_authenticated_client, build_delete_rrset_record, build_record_fqdn, effective_record_ttl,
    rrset_ttl_matches, should_update_record,
};
use anyhow::{Context, Result};
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;

/// Compare existing DNS `RRset` with the desired PTR target and TTL.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `target` - Desired PTR target host from the spec
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if the existing `RRset` matches the desired state exactly (no changes
/// needed), `false` if an update is required (rdata or TTL differ).
fn compare_ptr_rrset(existing_records: &[Record], target: &str, desired_ttl: u32) -> bool {
    if existing_records.len() != 1 {
        return false;
    }
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }
    let RData::PTR(existing_ptr) = &existing_records[0].data else {
        return false;
    };
    existing_ptr.0.to_string() == target
}

/// Add a PTR record using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// If the existing `RRset` differs from the desired state, the entire PTR
/// `RRset` for the name is deleted and recreated so stale rdata never lingers.
///
/// # Errors
///
/// Returns an error if:
/// - DNS server connection fails
/// - TSIG signer creation fails
/// - DNS update is rejected by the server
/// - Invalid domain name or target
#[allow(clippy::too_many_arguments)]
pub async fn add_ptr_record(
    zone_name: &str,
    name: &str,
    ptr_data: &PTRRecordData,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let ttl_value = effective_record_ttl(ptr_data.ttl);

    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::PTR,
        "PTR",
        server,
        |existing_records| compare_ptr_rrset(existing_records, &ptr_data.target, ttl_value),
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone =
        Name::from_str(zone_name).context(format!("Invalid zone name for PTR: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;
    let target_name = Name::from_str(&ptr_data.target).context(format!(
        "Invalid target for PTR record: {}",
        ptr_data.target
    ))?;

    let mut record =
        Record::from_rdata(fqdn.clone(), ttl_value, RData::PTR(rdata::PTR(target_name)));
    record.dns_class = DNSClass::IN;

    let mut client = build_authenticated_client(server, key_data).await?;

    // Step 1: delete existing RRset (ignore errors — may not exist).
    let delete_record = build_delete_rrset_record(&fqdn, RecordType::PTR);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    // Step 2: append the desired record to create the new RRset.
    let response = client
        .append(record, zone, false)
        .await
        .context(format!("Failed to send PTR record update for {fqdn}"))?;

    match response.metadata.response_code {
        ResponseCode::NoError => {
            info!(
                "Successfully added PTR record: {} -> {} (TTL: {})",
                fqdn, ptr_data.target, ttl_value
            );
            Ok(())
        }
        code => {
            anyhow::bail!("DNS server rejected PTR record update for {fqdn}: {code:?}");
        }
    }
}

#[cfg(test)]
#[path = "ptr_tests.rs"]
mod ptr_tests;
