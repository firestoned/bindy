use crate::crd::{
    AAAARecord, ARecord, CAARecord, CNAMERecord, MXRecord, NSRecord, SRVRecord, TXTRecord,
};
use anyhow::Result;
use kube::{client::Client, ResourceExt};
use tracing::info;

/// Reconcile an ARecord resource
pub async fn reconcile_a_record(
    client: Client,
    record: ARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling ARecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    zone_manager.add_a_record(&spec.zone, &spec.name, &spec.ipv4_address, spec.ttl)?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "A record created").await?;

    Ok(())
}

/// Reconcile a TXTRecord resource
pub async fn reconcile_txt_record(
    client: Client,
    record: TXTRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling TXTRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    zone_manager.add_txt_record(&spec.zone, &spec.name, &spec.text, spec.ttl)?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "TXT record created").await?;

    Ok(())
}

/// Reconcile an AAAARecord resource
pub async fn reconcile_aaaa_record(
    client: Client,
    record: AAAARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling AAAARecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    zone_manager.add_aaaa_record(&spec.zone, &spec.name, &spec.ipv6_address, spec.ttl)?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "AAAA record created").await?;

    Ok(())
}

/// Reconcile a CNAMERecord resource
pub async fn reconcile_cname_record(
    client: Client,
    record: CNAMERecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CNAMERecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    zone_manager.add_cname_record(&spec.zone, &spec.name, &spec.target, spec.ttl)?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "CNAME record created").await?;

    Ok(())
}

/// Reconcile an MXRecord resource
pub async fn reconcile_mx_record(
    client: Client,
    record: MXRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling MXRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    zone_manager.add_mx_record(
        &spec.zone,
        &spec.name,
        spec.priority,
        &spec.mail_server,
        spec.ttl,
    )?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "MX record created").await?;

    Ok(())
}

/// Reconcile an NSRecord resource
pub async fn reconcile_ns_record(
    client: Client,
    record: NSRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling NSRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    zone_manager.add_ns_record(&spec.zone, &spec.name, &spec.nameserver, spec.ttl)?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "NS record created").await?;

    Ok(())
}

/// Reconcile an SRVRecord resource
pub async fn reconcile_srv_record(
    client: Client,
    record: SRVRecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling SRVRecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    let srv_data = crate::bind9::SRVRecordData {
        priority: spec.priority,
        weight: spec.weight,
        port: spec.port,
        target: spec.target.clone(),
        ttl: spec.ttl,
    };
    zone_manager.add_srv_record(&spec.zone, &spec.name, &srv_data)?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "SRV record created").await?;

    Ok(())
}

/// Reconcile a CAARecord resource
pub async fn reconcile_caa_record(
    client: Client,
    record: CAARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Reconciling CAARecord: {}/{}", namespace, name);

    let spec = &record.spec;

    // Add the record to the zone file
    zone_manager.add_caa_record(
        &spec.zone,
        &spec.name,
        spec.flags,
        &spec.tag,
        &spec.value,
        spec.ttl,
    )?;

    // Update status
    update_record_status(&client, &record, "Ready", "True", "CAA record created").await?;

    Ok(())
}

/// Update the status of a DNS record (generic)
async fn update_record_status(
    _client: &Client,
    record: &impl ResourceExt,
    _condition_type: &str,
    _status: &str,
    _message: &str,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    // TODO: In production you'd want type-specific status implementations
    // For now, we just log the status update
    info!("Updated status for {}/{}", namespace, name);

    Ok(())
}
