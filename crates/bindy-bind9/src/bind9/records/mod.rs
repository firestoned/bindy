// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! DNS record management functions using dynamic DNS updates (RFC 2136).
//!
//! This module provides functions for managing DNS records via the nsupdate protocol.
//! Each record type has its own submodule that converts (and validates) its
//! spec to `RData`; every kind then goes through `write_rrset`, which
//! compares the live `RRset` by value and replaces it in one UPDATE
//! (ADR-0021).

pub mod a;
pub mod caa;
pub mod cname;
pub mod mx;
pub mod ns;
pub mod ptr;
pub mod srv;
pub mod txt;

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use hickory_net::client::{Client, ClientHandle};
use hickory_net::runtime::TokioRuntimeProvider;
use hickory_net::udp::UdpClientStream;
use hickory_net::DnsHandle;
use hickory_proto::op::{
    DnsRequest, Edns, Message, OpCode, Query, ResponseCode, UpdateMessage, DEFAULT_MAX_PAYLOAD_LEN,
};
use hickory_proto::rr::{rdata, DNSClass, Name, RData, Record, RecordType};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::{debug, warn};

use crate::bind9::rndc::create_tsig_signer;
use crate::bind9::types::RndcKeyData;
use crate::constants::DEFAULT_DNS_RECORD_TTL_SECS;

/// Fallback TTL (seconds) used only if [`DEFAULT_DNS_RECORD_TTL_SECS`] cannot be
/// converted to `u32`.
const FALLBACK_DNS_RECORD_TTL_SECS: u32 = 300;

/// CAA property tag authorising a CA to issue certificates (RFC 8659 4.2).
pub(crate) const CAA_TAG_ISSUE: &str = "issue";

/// CAA property tag authorising a CA to issue wildcard certificates (RFC 8659 4.3).
pub(crate) const CAA_TAG_ISSUEWILD: &str = "issuewild";

/// CAA property tag naming where to report invalid requests (RFC 8659 4.4).
pub(crate) const CAA_TAG_IODEF: &str = "iodef";

/// Resolve the effective TTL for a DNS record from an optional spec value.
///
/// Returns the spec TTL when present and representable as `u32`; otherwise falls
/// back to [`DEFAULT_DNS_RECORD_TTL_SECS`].
///
/// This is the single source of truth for the TTL written to DNS, and is also
/// used when diffing desired state against existing records so that TTL-only
/// spec changes trigger an update.
pub(crate) fn effective_record_ttl(ttl: Option<i32>) -> u32 {
    u32::try_from(ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS)).unwrap_or_else(|_| {
        u32::try_from(DEFAULT_DNS_RECORD_TTL_SECS).unwrap_or(FALLBACK_DNS_RECORD_TTL_SECS)
    })
}

/// Build the placeholder record used by RFC 2136 "delete `RRset`" operations.
///
/// `delete_rrset()` overwrites class/TTL/data based on `record_type`, so the
/// returned record only needs to carry the FQDN and the record type.
pub(crate) fn build_delete_rrset_record(fqdn: &Name, record_type: RecordType) -> Record {
    Record::from_rdata(fqdn.clone(), 0, RData::Update0(record_type))
}

/// Response codes that indicate an RFC 2136 delete succeeded (`NoError`) or the
/// target records already did not exist (`NXDomain`, `NXRRSet`): idempotent
/// success.
///
/// Every other code (e.g. `Refused`, `NotAuth`, `NotZone`, `ServFail`) is a real
/// failure and must be surfaced to the caller.
pub(crate) fn is_idempotent_delete_response_code(code: ResponseCode) -> bool {
    matches!(
        code,
        ResponseCode::NoError | ResponseCode::NXDomain | ResponseCode::NXRRSet
    )
}

/// Build an unauthenticated UDP DNS client for read-only queries.
async fn build_query_client(server_str: &str) -> Result<Client<TokioRuntimeProvider>> {
    let server_addr: SocketAddr = server_str
        .parse()
        .with_context(|| format!("Invalid server address: {server_str}"))?;
    let stream = UdpClientStream::builder(server_addr, TokioRuntimeProvider::default()).build();
    let (client, bg) = Client::<TokioRuntimeProvider>::from_sender(stream);
    tokio::spawn(bg);
    Ok(client)
}

/// Build a TSIG-authenticated UDP DNS client for RFC 2136 dynamic updates.
pub(crate) async fn build_authenticated_client(
    server_str: &str,
    key_data: &RndcKeyData,
) -> Result<Client<TokioRuntimeProvider>> {
    let server_addr: SocketAddr = server_str
        .parse()
        .with_context(|| format!("Invalid server address: {server_str}"))?;
    let signer = create_tsig_signer(key_data)?;
    let stream = UdpClientStream::builder(server_addr, TokioRuntimeProvider::default())
        .with_signer(Some(signer))
        .build();
    let (client, bg) = Client::<TokioRuntimeProvider>::from_sender(stream);
    tokio::spawn(bg);
    Ok(client)
}

/// Bytes reserved for the TSIG record the signer appends to an UPDATE: the
/// key and algorithm names, the time, fudge, MAC (up to 64 bytes for
/// HMAC-SHA512) and the trailing fields, with room to spare.
const TSIG_RECORD_RESERVE_BYTES: usize = 256;

/// How long a TCP connection to BIND9 may take, and how long one UPDATE
/// waits for its response.
const UPDATE_TCP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The transport an UPDATE is sent on (ADR-0021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateTransport {
    /// Fits the EDNS UDP payload.
    Udp,
    /// Too large for UDP: hickory's UDP client has no TCP fallback.
    Tcp,
}

/// Choose the transport for an UPDATE of `encoded_len` bytes (before TSIG).
///
/// One UPDATE carries a whole RRset, so a large one can exceed the EDNS UDP
/// payload, and hickory's UDP client would send an oversized datagram rather
/// than fall back to TCP. The TSIG record counts toward the size.
///
/// # Arguments
/// * `encoded_len` - The encoded UPDATE message, unsigned
///
/// # Returns
/// [`UpdateTransport::Tcp`] when the signed message would exceed
/// `DEFAULT_MAX_PAYLOAD_LEN`, [`UpdateTransport::Udp`] otherwise.
#[must_use]
pub(crate) fn update_transport(encoded_len: usize) -> UpdateTransport {
    if encoded_len + TSIG_RECORD_RESERVE_BYTES > usize::from(DEFAULT_MAX_PAYLOAD_LEN) {
        return UpdateTransport::Tcp;
    }
    UpdateTransport::Udp
}

/// The wire size of a DNS message.
///
/// # Errors
/// Returns an error if the message cannot be encoded.
pub(crate) fn encoded_len(message: &Message) -> Result<usize> {
    Ok(message
        .to_vec()
        .context("Failed to encode the DNS UPDATE")?
        .len())
}

/// Build a TSIG-authenticated TCP DNS client for an UPDATE too large for UDP.
///
/// # Errors
/// Returns an error if the address or key is invalid, or the TCP connection
/// to BIND9 cannot be made.
pub(crate) async fn build_authenticated_tcp_client(
    server_str: &str,
    key_data: &RndcKeyData,
) -> Result<Client<TokioRuntimeProvider>> {
    use hickory_net::tcp::TcpClientStream;
    use hickory_net::xfer::DnsMultiplexer;

    let server_addr: SocketAddr = server_str
        .parse()
        .with_context(|| format!("Invalid server address: {server_str}"))?;
    let signer = create_tsig_signer(key_data)?;
    let (connect, handle) = TcpClientStream::new(
        server_addr,
        None,
        Some(UPDATE_TCP_TIMEOUT),
        TokioRuntimeProvider::default(),
    );
    let stream = connect
        .await
        .with_context(|| format!("Failed to open a TCP connection to {server_str}"))?;
    let multiplexer = DnsMultiplexer::new(stream, handle)
        .with_timeout(UPDATE_TCP_TIMEOUT)
        .with_signer(signer);
    let (client, bg) = Client::<TokioRuntimeProvider>::from_sender(multiplexer);
    tokio::spawn(bg);
    Ok(client)
}

/// Build the fully-qualified record name for a given (zone, name).
///
/// `@` or empty `name` produces the zone apex; otherwise the name is concatenated with the zone.
pub(crate) fn build_record_fqdn(zone_name: &str, name: &str) -> Result<Name> {
    if name == "@" || name.is_empty() {
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))
    } else {
        Name::from_str(&format!("{name}.{zone_name}"))
            .with_context(|| format!("Invalid record name: {name}.{zone_name}"))
    }
}

/// Generic DNS record query function.
///
/// Queries a DNS server for records of a specific type and returns the results.
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name
/// * `name` - The record name (e.g., "www" for www.example.com, or "@" for apex)
/// * `record_type` - The DNS record type (A, AAAA, TXT, MX, etc.)
/// * `server` - The DNS server address (IP:port)
///
/// # Returns
///
/// Returns `Ok(vec)` with matching records (empty if none exist),
/// or an error if the query fails.
///
/// # Errors
///
/// Returns an error if the DNS query fails or cannot be parsed.
pub async fn query_dns_record(
    zone_name: &str,
    name: &str,
    record_type: RecordType,
    server: &str,
) -> Result<Vec<Record>> {
    let mut client = build_query_client(server).await?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    let response = client
        .query(fqdn.clone(), DNSClass::IN, record_type)
        .await
        .with_context(|| format!("Failed to query {record_type:?} record for {fqdn}"))?;

    let records: Vec<Record> = response
        .answers
        .iter()
        .filter(|r| r.record_type() == record_type)
        .cloned()
        .collect();

    Ok(records)
}

/// Build the RFC 2136 UPDATE message that replaces the `record_type` `RRset`
/// at `name` with exactly `desired` (ADR-0021).
///
/// The message carries, in order: the zone section (the zone's SOA, class
/// IN), no prerequisites, then the update section: one "delete an `RRset`"
/// (RFC 2136 2.5.2: class ANY, TTL 0, empty rdata, built exactly like
/// hickory-proto's `update_message::delete_rrset`) followed by one "add to an
/// `RRset`" per desired value (2.5.1: class IN, the given TTL), in the order
/// given. The server applies the update section in order and atomically
/// (RFC 2136 3.4 and 3.7), so a query sees the old `RRset` or the new one,
/// never an empty or partial one. An empty `desired` is just the delete.
///
/// # Arguments
///
/// * `zone` - The zone origin (the SOA name)
/// * `name` - The owner name of the `RRset`, inside `zone`
/// * `record_type` - The type of the `RRset` to replace
/// * `ttl` - The TTL of every added record
/// * `desired` - The exact values the `RRset` must hold afterwards
/// * `use_edns` - Add an EDNS OPT record, as the hickory builders do
///
/// # Errors
///
/// Returns an error if `name` is not inside `zone`, or if a desired value is
/// not of `record_type`.
pub(crate) fn replace_rrset_message(
    zone: &Name,
    name: &Name,
    record_type: RecordType,
    ttl: u32,
    desired: Vec<RData>,
    use_edns: bool,
) -> Result<Message> {
    if !zone.zone_of(name) {
        return Err(anyhow!(
            "{record_type:?} record {name} is not inside zone {zone}"
        ));
    }
    if let Some(wrong) = desired.iter().find(|r| r.record_type() != record_type) {
        return Err(anyhow!(
            "{:?} value {wrong} cannot be written to the {record_type:?} RRset for {name}",
            wrong.record_type()
        ));
    }

    let mut zone_section = Query::new();
    zone_section
        .set_name(zone.clone())
        .set_query_class(DNSClass::IN)
        .set_query_type(RecordType::SOA);

    let mut message = Message::query();
    message.metadata.op_code = OpCode::Update;
    message.metadata.recursion_desired = false;
    message.add_zone(zone_section);

    let mut delete = build_delete_rrset_record(name, record_type);
    delete.dns_class = DNSClass::ANY;
    message.add_update(delete);

    for value in desired {
        let mut add = Record::from_rdata(name.clone(), ttl, value);
        add.dns_class = DNSClass::IN;
        message.add_update(add);
    }

    if use_edns {
        message
            .edns
            .get_or_insert_with(Edns::new)
            .set_max_payload(DEFAULT_MAX_PAYLOAD_LEN)
            .set_version(0);
    }

    Ok(message)
}

/// Classify the server's answer to an `RRset` replacement.
///
/// `NoError` is the only success: the message carries no prerequisites, so a
/// delete of an `RRset` that does not exist is silently ignored by the server
/// and still answers `NoError`. Any other code (e.g. `Refused`, `NotAuth`,
/// `NotZone`, `ServFail`) fails the write, and the reconcile retries the whole
/// message (ADR-0015 failure semantics, no partial retries).
///
/// # Errors
///
/// Returns an error naming the response code, the record type and the name.
pub(crate) fn check_replace_response(
    code: ResponseCode,
    name: &Name,
    record_type: RecordType,
) -> Result<()> {
    if code == ResponseCode::NoError {
        return Ok(());
    }
    Err(anyhow!(
        "DNS UPDATE replacing the {record_type:?} RRset for {name} rejected \
         with response code: {code:?}"
    ))
}

/// Send a built RRset-replacing UPDATE ([`replace_rrset_message`]) and check
/// its response (ADR-0021).
///
/// # Arguments
/// * `client` - A TSIG-signed client on the transport [`update_transport`] chose
/// * `message` - The UPDATE
/// * `name` - The RRset's owner name, for errors
/// * `record_type` - The RRset's type, for errors
///
/// # Errors
/// Returns an error if the UPDATE cannot be sent, gets no response, or the
/// response is anything other than `NOERROR`.
pub(crate) async fn send_rrset_update(
    client: &Client<TokioRuntimeProvider>,
    message: Message,
    name: &Name,
    record_type: RecordType,
) -> Result<()> {
    let mut responses = client.send(DnsRequest::from(message));
    let response = responses
        .next()
        .await
        .ok_or_else(|| {
            anyhow!("No response to the DNS UPDATE replacing the {record_type:?} RRset for {name}")
        })?
        .with_context(|| {
            format!("Failed to send DNS UPDATE replacing the {record_type:?} RRset for {name}")
        })?;
    check_replace_response(response.metadata.response_code, name, record_type)
}

/// Normalise a domain name for comparison: lowercase and fully qualified.
///
/// On the wire every name is absolute, so `target.example.com` and
/// `Target.Example.Com.` are the same value.
fn normalize_name(name: &Name) -> Name {
    let mut normalized = name.to_lowercase();
    normalized.set_fqdn(true);
    normalized
}

/// Normalise a CAA value for comparison: the tag in lowercase and, for the
/// `issue` and `issuewild` properties, the issuer domain in lowercase without
/// a trailing dot. Any other property is compared byte for byte.
fn normalize_caa(caa: &rdata::CAA) -> rdata::CAA {
    let mut normalized = caa.clone();
    normalized.tag = caa.tag.to_ascii_lowercase();
    let wildcard = match normalized.tag.as_str() {
        CAA_TAG_ISSUE => false,
        CAA_TAG_ISSUEWILD => true,
        _ => return normalized,
    };
    let Ok((issuer, options)) = caa.value_as_issue() else {
        return normalized;
    };
    let issuer = issuer.map(|n| {
        let mut n = n.to_lowercase();
        n.set_fqdn(false);
        n
    });
    let rebuilt = if wildcard {
        rdata::CAA::new_issuewild(caa.issuer_critical, issuer, options)
    } else {
        rdata::CAA::new_issue(caa.issuer_critical, issuer, options)
    };
    normalized.value = rebuilt.value;
    normalized
}

/// Normalise one value for comparison: every domain name inside it (CNAME,
/// NS and PTR targets, the MX exchange, the SRV target, the CAA issuer) is
/// lowercased and made fully qualified. Other values compare as they are.
fn normalize_rdata(data: &RData) -> RData {
    match data {
        RData::CNAME(cname) => RData::CNAME(rdata::CNAME(normalize_name(&cname.0))),
        RData::NS(ns) => RData::NS(rdata::NS(normalize_name(&ns.0))),
        RData::PTR(ptr) => RData::PTR(rdata::PTR(normalize_name(&ptr.0))),
        RData::MX(mx) => RData::MX(rdata::MX::new(mx.preference, normalize_name(&mx.exchange))),
        RData::SRV(srv) => RData::SRV(rdata::SRV::new(
            srv.priority,
            srv.weight,
            srv.port,
            normalize_name(&srv.target),
        )),
        RData::CAA(caa) => RData::CAA(normalize_caa(caa)),
        other => other.clone(),
    }
}

/// Decide whether the live `RRset` must be replaced to hold `desired` (ADR-0021).
///
/// Returns `false` only when every live record carries `ttl` and the live
/// values equal `desired` as a SET: order does not matter, and domain names
/// are normalised (case, trailing dot) before comparing. A live `RRset` that
/// is empty while `desired` is empty needs nothing.
#[must_use]
pub(crate) fn needs_update(current: &[Record], desired: &[RData], ttl: u32) -> bool {
    if current.iter().any(|record| record.ttl != ttl) {
        return true;
    }
    let live: HashSet<RData> = current
        .iter()
        .map(|record| normalize_rdata(&record.data))
        .collect();
    let wanted: HashSet<RData> = desired.iter().map(normalize_rdata).collect();
    live != wanted
}

/// Query the live `RRset` and decide whether it must be written.
///
/// A failed query is not fatal: as before ADR-0021, it is logged and the
/// write is attempted anyway.
async fn rrset_needs_write(
    zone_name: &str,
    name: &str,
    record_type: RecordType,
    ttl: u32,
    desired: &[RData],
    server: &str,
) -> bool {
    match query_dns_record(zone_name, name, record_type, server).await {
        Ok(current) => {
            let write = needs_update(&current, desired, ttl);
            if !write {
                debug!(
                    "{:?} record {} already holds the desired values - no changes needed",
                    record_type, name
                );
            }
            write
        }
        Err(e) => {
            warn!(
                "Failed to query existing {:?} record {} (will attempt update anyway): {}",
                record_type, name, e
            );
            true
        }
    }
}

/// Write an `RRset` declaratively: compare the live `RRset` with `desired`
/// by value and, if they differ, replace it in one UPDATE (ADR-0021).
///
/// Every record kind's writer converts (and validates) its spec to `desired`
/// first, then calls this.
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name
/// * `name` - The record name (`@` or empty for the apex)
/// * `record_type` - The type of the `RRset`
/// * `ttl` - The effective TTL
/// * `desired` - The exact values the `RRset` must hold
/// * `server` - The DNS server address (IP:port)
/// * `key_data` - TSIG key for authentication
///
/// # Errors
///
/// Returns an error if the zone or record name is invalid, the client cannot
/// be built, or the replacement fails (see [`send_rrset_update`]).
pub(crate) async fn write_rrset(
    zone_name: &str,
    name: &str,
    record_type: RecordType,
    ttl: u32,
    desired: Vec<RData>,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let zone =
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    if !rrset_needs_write(zone_name, name, record_type, ttl, &desired, server).await {
        return Ok(());
    }

    debug!(
        "Replacing {:?} RRset {} with {} value(s) (TTL: {})",
        record_type,
        fqdn,
        desired.len(),
        ttl
    );
    let message = replace_rrset_message(&zone, &fqdn, record_type, ttl, desired, true)?;
    let client = match update_transport(encoded_len(&message)?) {
        UpdateTransport::Udp => build_authenticated_client(server, key_data).await?,
        UpdateTransport::Tcp => build_authenticated_tcp_client(server, key_data).await?,
    };
    send_rrset_update(&client, message, &fqdn, record_type).await?;
    debug!("Replaced {:?} RRset {}", record_type, fqdn);
    Ok(())
}

/// Delete a DNS record of any type using dynamic DNS update (RFC 2136).
///
/// This function sends an RFC 2136 DELETE operation to remove ALL records
/// of the specified type for the given name.
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name (e.g., "example.com")
/// * `name` - The record name (e.g., "www" for www.example.com, or "@" for apex)
/// * `record_type` - The DNS record type to delete (A, AAAA, TXT, MX, etc.)
/// * `server` - The DNS server address (IP:port, e.g., "10.0.0.1:53")
/// * `key_data` - TSIG key for authentication
///
/// # Returns
///
/// Returns `Ok(())` if deletion succeeded (`NoError`) or the record already did
/// not exist (`NXDomain`/`NXRRSet` — idempotent success).
///
/// # Errors
///
/// Returns an error if the connection fails or the DNS server rejects the
/// update with any other response code (e.g. `Refused`, `NotAuth`, `NotZone`,
/// `ServFail`), so TSIG/ACL failures are never silently swallowed.
pub async fn delete_dns_record(
    zone_name: &str,
    name: &str,
    record_type: RecordType,
    server: &str,
    key_data: &RndcKeyData,
) -> Result<()> {
    let mut client = build_authenticated_client(server, key_data).await?;
    let zone =
        Name::from_str(zone_name).with_context(|| format!("Invalid zone name: {zone_name}"))?;
    let fqdn = build_record_fqdn(zone_name, name)?;

    debug!(
        "Deleting {:?} record: {} from zone {}",
        record_type, fqdn, zone_name
    );

    // Build a placeholder record. delete_rrset() overwrites class/ttl/data based on record_type.
    let dummy_record = build_delete_rrset_record(&fqdn, record_type);

    let response = client
        .delete_rrset(dummy_record, zone)
        .await
        .with_context(|| {
            format!("Failed to send DNS UPDATE to delete {record_type:?} record {fqdn}")
        })?;

    let code = response.metadata.response_code;
    if !is_idempotent_delete_response_code(code) {
        return Err(anyhow::anyhow!(
            "DNS DELETE for {record_type:?} record {fqdn} in zone {zone_name} \
             rejected with response code: {code:?}"
        ));
    }

    if code == ResponseCode::NoError {
        debug!(
            "Successfully deleted {:?} record: {} from zone {}",
            record_type, name, zone_name
        );
        return Ok(());
    }

    // NXDomain/NXRRSet: the record did not exist — deletion is idempotent.
    warn!(
        "DNS DELETE for {:?} record {fqdn} returned code: {:?} (record did not exist)",
        record_type, code
    );
    Ok(())
}

#[cfg(test)]
mod mod_tests;
