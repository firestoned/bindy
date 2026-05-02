// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CNAME record management.

use super::super::types::RndcKeyData;
use super::{build_authenticated_client, build_record_fqdn, should_update_record};
use anyhow::Result;
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;

use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

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
    let target_for_comparison = target.to_string();
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::CNAME,
        "CNAME",
        server,
        |existing_records| {
            if existing_records.len() == 1 {
                if let RData::CNAME(existing_cname) = &existing_records[0].data {
                    return existing_cname.0.to_string() == target_for_comparison;
                }
            }
            false
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

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
