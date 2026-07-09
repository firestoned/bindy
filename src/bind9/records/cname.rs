// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CNAME record management.

use super::super::types::RndcKeyData;
use super::{
    build_authenticated_client, build_record_fqdn, effective_record_ttl, rrset_ttl_matches,
    should_update_record,
};
use anyhow::Result;
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;

/// Compare existing DNS `RRset` with the desired CNAME target and TTL.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `target` - Desired CNAME target from the spec
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if the existing `RRset` matches the desired state exactly (no changes
/// needed), `false` if an update is required (rdata or TTL differ).
fn compare_cname_rrset(existing_records: &[Record], target: &str, desired_ttl: u32) -> bool {
    if existing_records.len() != 1 {
        return false;
    }
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }
    let RData::CNAME(existing_cname) = &existing_records[0].data else {
        return false;
    };
    existing_cname.0.to_string() == target
}

/// Add a CNAME record using dynamic DNS update (RFC 2136).
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_cname_record(
    zone_name: &str,
    name: &str,
    target: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let ttl_value = effective_record_ttl(ttl);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::CNAME,
        "CNAME",
        server,
        |existing_records| compare_cname_rrset(existing_records, target, ttl_value),
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone = Name::from_str(zone_name)?;
    let fqdn = build_record_fqdn(zone_name, name)?;
    let target_name = Name::from_str(target)?;

    let mut record = Record::from_rdata(
        fqdn.clone(),
        ttl_value,
        RData::CNAME(rdata::CNAME(target_name)),
    );
    record.dns_class = DNSClass::IN;

    info!(
        "Adding CNAME record: {} -> {} (TTL: {})",
        record.name, target, ttl_value
    );

    let mut client = build_authenticated_client(server, key_data).await?;
    let response = client.append(record, zone, false).await?;

    match response.metadata.response_code {
        ResponseCode::NoError => {
            info!("Successfully added CNAME record: {} -> {}", name, target);
            Ok(())
        }
        code => Err(anyhow::anyhow!(
            "DNS update failed with response code: {code:?}"
        )),
    }
}

#[cfg(test)]
#[path = "cname_tests.rs"]
mod cname_tests;
