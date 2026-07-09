// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! MX record management.

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

/// Default MX preference used when the spec priority cannot be represented as `u16`.
const DEFAULT_MX_PREFERENCE: u16 = 10;

/// Compare existing DNS `RRset` with the desired MX preference, exchange, and TTL.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `preference` - Desired MX preference (priority) from the spec
/// * `mail_server` - Desired mail exchange host from the spec
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if the existing `RRset` matches the desired state exactly (no changes
/// needed), `false` if an update is required (rdata or TTL differ).
fn compare_mx_rrset(
    existing_records: &[Record],
    preference: u16,
    mail_server: &str,
    desired_ttl: u32,
) -> bool {
    if existing_records.len() != 1 {
        return false;
    }
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }
    let RData::MX(existing_mx) = &existing_records[0].data else {
        return false;
    };
    existing_mx.preference == preference && existing_mx.exchange.to_string() == mail_server
}

/// Add an MX record using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// If the existing `RRset` differs from the desired state, the entire MX
/// `RRset` for the name is deleted and recreated so stale rdata (e.g. an old
/// mail server) never lingers.
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_mx_record(
    zone_name: &str,
    name: &str,
    priority: i32,
    mail_server: &str,
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let priority_u16 = u16::try_from(priority).unwrap_or(DEFAULT_MX_PREFERENCE);
    let ttl_value = effective_record_ttl(ttl);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::MX,
        "MX",
        server,
        |existing_records| compare_mx_rrset(existing_records, priority_u16, mail_server, ttl_value),
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone = Name::from_str(zone_name)?;
    let fqdn = build_record_fqdn(zone_name, name)?;
    let mx_name = Name::from_str(mail_server)?;

    let mut record = Record::from_rdata(
        fqdn.clone(),
        ttl_value,
        RData::MX(rdata::MX::new(priority_u16, mx_name)),
    );
    record.dns_class = DNSClass::IN;

    info!(
        "Adding MX record: {} -> {} (priority: {}, TTL: {})",
        fqdn, mail_server, priority_u16, ttl_value
    );

    let mut client = build_authenticated_client(server, key_data).await?;

    // Step 1: delete existing RRset (ignore errors — may not exist).
    let delete_record = build_delete_rrset_record(&fqdn, RecordType::MX);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    // Step 2: append the desired record to create the new RRset.
    let response = client.append(record, zone, false).await?;

    match response.metadata.response_code {
        ResponseCode::NoError => {
            info!("Successfully added MX record: {} -> {}", name, mail_server);
            Ok(())
        }
        code => Err(anyhow::anyhow!(
            "DNS update failed with response code: {code:?}"
        )),
    }
}

#[cfg(test)]
#[path = "mx_tests.rs"]
mod mx_tests;
