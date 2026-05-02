// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! NS record management.

use super::super::types::RndcKeyData;
use super::{build_authenticated_client, build_record_fqdn, should_update_record};
use anyhow::Result;
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;

use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add an NS record using dynamic DNS update (RFC 2136).
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
    let nameserver_for_comparison = nameserver.to_string();
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::NS,
        "NS",
        server,
        |existing_records| {
            if existing_records.len() == 1 {
                if let RData::NS(existing_ns) = &existing_records[0].data {
                    return existing_ns.0.to_string() == nameserver_for_comparison;
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
    let ns_name = Name::from_str(nameserver)?;

    let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::NS(rdata::NS(ns_name)));
    record.dns_class = DNSClass::IN;

    info!(
        "Adding NS record: {} -> {} (TTL: {})",
        fqdn, nameserver, ttl_value
    );

    let mut client = build_authenticated_client(server, key_data).await?;
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
