// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! TXT record management.

use super::super::types::RndcKeyData;
use super::should_update_record;
use anyhow::Result;
use hickory_client::client::{Client, SyncClient};
use hickory_client::op::ResponseCode;
use hickory_client::rr::{rdata, DNSClass, Name, RData, Record};
use hickory_client::udp::UdpClientConnection;
use std::str::FromStr;
use tracing::info;

use crate::bind9::rndc::create_tsig_signer;
use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Add a TXT record using dynamic DNS update (RFC 2136).
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
    use hickory_client::rr::RecordType;

    // Check if update is needed using declarative reconciliation pattern
    let texts_for_comparison = texts.to_vec();
    let should_update = should_update_record(
        zone_name,
        name,
        RecordType::TXT,
        "TXT",
        server,
        |existing_records| {
            // Compare: should return true if records match desired state
            if existing_records.len() == 1 {
                if let Some(RData::TXT(existing_txt)) = existing_records[0].data() {
                    // Extract text data from TXT record
                    let existing_texts: Vec<String> = existing_txt
                        .txt_data()
                        .iter()
                        .map(|bytes| String::from_utf8_lossy(bytes).to_string())
                        .collect();
                    return existing_texts == texts_for_comparison;
                }
            }
            false
        },
    )
    .await?;

    if !should_update {
        return Ok(());
    }

    let zone_name_str = zone_name.to_string();
    let name_str = name.to_string();
    let texts_vec: Vec<String> = texts.to_vec();
    let server_str = server.to_string();
    let ttl_value = u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS))
        .unwrap_or(u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(300));
    let key_data = key_data.clone();

    tokio::task::spawn_blocking(move || {
        let server_addr = server_str.parse::<std::net::SocketAddr>()?;
        let conn = UdpClientConnection::new(server_addr)?;
        let signer = create_tsig_signer(&key_data)?;
        let client = SyncClient::with_tsigner(conn, signer);

        let zone = Name::from_str(&zone_name_str)?;
        let fqdn = if name_str == "@" || name_str.is_empty() {
            zone.clone()
        } else {
            Name::from_str(&format!("{name_str}.{zone_name_str}"))?
        };

        let txt_rdata = rdata::TXT::new(texts_vec.clone());
        let mut record = Record::from_rdata(fqdn.clone(), ttl_value, RData::TXT(txt_rdata));
        record.set_dns_class(DNSClass::IN);

        info!(
            "Adding TXT record: {} -> {:?} (TTL: {})",
            record.name(),
            texts_vec,
            ttl_value
        );
        // Use append for idempotent operation (must_exist=false for no prerequisites)
        let response = client.append(record, zone, false)?;

        match response.response_code() {
            ResponseCode::NoError => {
                info!("Successfully added TXT record: {}", name_str);
                Ok(())
            }
            code => Err(anyhow::anyhow!(
                "DNS update failed with response code: {code:?}"
            )),
        }
    })
    .await?
}

#[cfg(test)]
#[path = "txt_tests.rs"]
mod txt_tests;
