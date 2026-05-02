// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! MX record management.

use super::super::types::RndcKeyData;
use super::{build_authenticated_client, build_record_fqdn, should_update_record};
use anyhow::Result;
use hickory_net::client::ClientHandle;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::str::FromStr;
use tracing::info;

use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add an MX record using dynamic DNS update (RFC 2136).
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
    let mail_server_for_comparison = mail_server.to_string();
    let priority_u16 = u16::try_from(priority).unwrap_or(10);
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::MX,
        "MX",
        server,
        |existing_records| {
            if existing_records.len() == 1 {
                if let RData::MX(existing_mx) = &existing_records[0].data {
                    return existing_mx.preference == priority_u16
                        && existing_mx.exchange.to_string() == mail_server_for_comparison;
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
