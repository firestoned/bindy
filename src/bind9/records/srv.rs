// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! SRV record management.

use super::super::types::{RndcKeyData, SRVRecordData};
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

/// Compare existing DNS `RRset` with the desired SRV fields and TTL.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `priority` - Desired SRV priority from the spec
/// * `weight` - Desired SRV weight from the spec
/// * `port` - Desired SRV port from the spec
/// * `target` - Desired SRV target host from the spec
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if the existing `RRset` matches the desired state exactly (no changes
/// needed), `false` if an update is required (rdata or TTL differ).
fn compare_srv_rrset(
    existing_records: &[Record],
    priority: u16,
    weight: u16,
    port: u16,
    target: &str,
    desired_ttl: u32,
) -> bool {
    if existing_records.len() != 1 {
        return false;
    }
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }
    let RData::SRV(existing_srv) = &existing_records[0].data else {
        return false;
    };
    existing_srv.priority == priority
        && existing_srv.weight == weight
        && existing_srv.port == port
        && existing_srv.target.to_string() == target
}

/// Add an SRV record using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// If the existing `RRset` differs from the desired state, the entire SRV
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
pub async fn add_srv_record(
    zone_name: &str,
    name: &str,
    srv_data: &SRVRecordData,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let priority_u16 = u16::try_from(srv_data.priority)
        .context(format!("Invalid SRV priority: {}", srv_data.priority))?;
    let weight_u16 = u16::try_from(srv_data.weight)
        .context(format!("Invalid SRV weight: {}", srv_data.weight))?;
    let port_u16 =
        u16::try_from(srv_data.port).context(format!("Invalid SRV port: {}", srv_data.port))?;
    let ttl_value = effective_record_ttl(srv_data.ttl);

    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::SRV,
        "SRV",
        server,
        |existing_records| {
            compare_srv_rrset(
                existing_records,
                priority_u16,
                weight_u16,
                port_u16,
                &srv_data.target,
                ttl_value,
            )
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone =
        Name::from_str(zone_name).context(format!("Invalid zone name for SRV: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;
    let target_name = Name::from_str(&srv_data.target).context(format!(
        "Invalid target for SRV record: {}",
        srv_data.target
    ))?;

    let record_data = rdata::SRV::new(priority_u16, weight_u16, port_u16, target_name);
    let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::SRV(record_data));
    record.dns_class = DNSClass::IN;

    let mut client = build_authenticated_client(server, key_data).await?;

    // Step 1: delete existing RRset (ignore errors — may not exist).
    let delete_record = build_delete_rrset_record(&fqdn, RecordType::SRV);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    // Step 2: append the desired record to create the new RRset.
    let response = client
        .append(record, zone, false)
        .await
        .context(format!("Failed to send SRV record update for {fqdn}"))?;

    match response.metadata.response_code {
        ResponseCode::NoError => {
            info!(
                "Successfully added SRV record: {} -> {}:{} (priority: {}, weight: {}, TTL: {})",
                fqdn, srv_data.target, srv_data.port, srv_data.priority, srv_data.weight, ttl_value
            );
            Ok(())
        }
        code => {
            anyhow::bail!("DNS server rejected SRV record update for {fqdn}: {code:?}");
        }
    }
}

#[cfg(test)]
#[path = "srv_tests.rs"]
mod srv_tests;
