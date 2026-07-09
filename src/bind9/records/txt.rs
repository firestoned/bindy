// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! TXT record management.

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

/// Compare existing DNS `RRset` with the desired TXT strings and TTL.
///
/// # Arguments
///
/// * `existing_records` - Records currently in DNS (from query)
/// * `texts` - Desired TXT strings from the spec
/// * `desired_ttl` - Effective TTL from the spec
///
/// # Returns
///
/// `true` if the existing `RRset` matches the desired state exactly (no changes
/// needed), `false` if an update is required (rdata or TTL differ).
fn compare_txt_rrset(existing_records: &[Record], texts: &[String], desired_ttl: u32) -> bool {
    if existing_records.len() != 1 {
        return false;
    }
    if !rrset_ttl_matches(existing_records, desired_ttl) {
        return false;
    }
    let RData::TXT(existing_txt) = &existing_records[0].data else {
        return false;
    };
    let existing_texts: Vec<String> = existing_txt
        .txt_data
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes).to_string())
        .collect();
    existing_texts == texts
}

/// Add a TXT record using dynamic DNS update (RFC 2136) with `RRset` synchronization.
///
/// If the existing `RRset` differs from the desired state, the entire TXT
/// `RRset` for the name is deleted and recreated so stale rdata never lingers.
///
/// # Errors
///
/// Returns an error if the DNS update fails or the server rejects it.
#[allow(clippy::too_many_arguments)]
pub async fn add_txt_record(
    zone_name: &str,
    name: &str,
    texts: &[String],
    ttl: Option<i32>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let ttl_value = effective_record_ttl(ttl);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::TXT,
        "TXT",
        server,
        |existing_records| compare_txt_rrset(existing_records, texts, ttl_value),
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone = Name::from_str(zone_name)?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    let mut record = Record::from_rdata(
        fqdn.clone(),
        ttl_value,
        RData::TXT(rdata::TXT::new(texts.to_vec())),
    );
    record.dns_class = DNSClass::IN;

    info!(
        "Adding TXT record: {} -> {:?} (TTL: {})",
        record.name, texts, ttl_value
    );

    let mut client = build_authenticated_client(server, key_data).await?;

    // Step 1: delete existing RRset (ignore errors — may not exist).
    let delete_record = build_delete_rrset_record(&fqdn, RecordType::TXT);
    let _ = client.delete_rrset(delete_record, zone.clone()).await;

    // Step 2: append the desired record to create the new RRset.
    let response = client.append(record, zone, false).await?;

    match response.metadata.response_code {
        ResponseCode::NoError => {
            info!("Successfully added TXT record: {}", name);
            Ok(())
        }
        code => Err(anyhow::anyhow!(
            "DNS update failed with response code: {code:?}"
        )),
    }
}

#[cfg(test)]
#[path = "txt_tests.rs"]
mod txt_tests;
