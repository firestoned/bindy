// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! NS record management.

use super::super::types::RndcKeyData;
use super::{
    build_authenticated_client, build_delete_rrset_record, build_record_fqdn, effective_record_ttl,
    rrset_ttl_matches, should_update_record,
};
use anyhow::Result;
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;

/// Compare existing DNS `RRset` with the desired nameserver and TTL.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `nameserver` - Desired nameserver host from the spec
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if the existing `RRset` matches the desired state exactly (no changes
/// needed), `false` if an update is required (rdata or TTL differ).
fn compare_ns_rrset(existing_records: &[Record], nameserver: &str, desired_ttl: u32) -> bool {
    if existing_records.len() != 1 {
        return false;
    }
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }
    let RData::NS(existing_ns) = &existing_records[0].data else {
        return false;
    };
    existing_ns.0.to_string() == nameserver
}

/// Add an NS record using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// NS records managed here are used for delegations. If the existing `RRset`
/// differs from the desired state, the entire NS `RRset` for the name is
/// deleted and recreated so stale delegation rdata never lingers.
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_ns_record(
    zone_name: &str,
    name: &str,
    nameserver: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let ttl_value = effective_record_ttl(ttl);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::NS,
        "NS",
        server,
        |existing_records| compare_ns_rrset(existing_records, nameserver, ttl_value),
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone = Name::from_str(zone_name)?;
    let fqdn = build_record_fqdn(zone_name, name)?;
    let ns_name = Name::from_str(nameserver)?;

    let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::NS(rdata::NS(ns_name)));
    record.dns_class = DNSClass::IN;

    info!(
        "Adding NS record: {} -> {} (TTL: {})",
        fqdn, nameserver, ttl_value
    );

    let mut client = build_authenticated_client(server, key_data).await?;

    // Step 1: delete existing RRset (ignore errors — may not exist).
    let delete_record = build_delete_rrset_record(&fqdn, RecordType::NS);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    // Step 2: append the desired record to create the new RRset.
    let response = client.append(record, zone, false).await?;

    match response.metadata.response_code {
        ResponseCode::NoError => {
            info!("Successfully added NS record: {} -> {}", name, nameserver);
            Ok(())
        }
        code => Err(anyhow::anyhow!(
            "DNS update failed with response code: {code:?}"
        )),
    }
}

#[cfg(test)]
#[path = "ns_tests.rs"]
mod ns_tests;
