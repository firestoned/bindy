// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! SRV record management.

use super::super::types::{RndcKeyData, SRVRecordData};
use super::{build_authenticated_client, build_record_fqdn, should_update_record};
use anyhow::{Context, Result};
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;

use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add an SRV record using dynamic DNS update (RFC 2136).
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
    let srv_data_for_comparison = srv_data.clone();
    let priority_u16 = u16::try_from(srv_data.priority)
        .context(format!("Invalid SRV priority: {}", srv_data.priority))?;
    let weight_u16 = u16::try_from(srv_data.weight)
        .context(format!("Invalid SRV weight: {}", srv_data.weight))?;
    let port_u16 =
        u16::try_from(srv_data.port).context(format!("Invalid SRV port: {}", srv_data.port))?;

    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::SRV,
        "SRV",
        server,
        |existing_records| {
            if existing_records.len() == 1 {
                if let RData::SRV(existing_srv) = &existing_records[0].data {
                    let priority_match = existing_srv.priority
                        == u16::try_from(srv_data_for_comparison.priority).unwrap_or(0);
                    let weight_match = existing_srv.weight
                        == u16::try_from(srv_data_for_comparison.weight).unwrap_or(0);
                    let port_match = existing_srv.port
                        == u16::try_from(srv_data_for_comparison.port).unwrap_or(0);
                    let target_match =
                        existing_srv.target.to_string() == srv_data_for_comparison.target;
                    return priority_match && weight_match && port_match && target_match;
                }
            }
            false
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let ttl_value = u32::try_from(srv_data.ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));

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
