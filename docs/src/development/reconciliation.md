# Reconciliation Logic

Detailed reconciliation logic for each resource type.

## Bind9Instance Reconciliation

```rust
async fn reconcile_bind9instance(instance: Bind9Instance) -> Result<()> {
    // 1. Build desired resources
    let configmap = build_configmap(&instance);
    let deployment = build_deployment(&instance);
    let service = build_service(&instance);
    
    // 2. Apply or update ConfigMap
    apply_configmap(configmap).await?;
    
    // 3. Apply or update Deployment
    apply_deployment(deployment).await?;
    
    // 4. Apply or update Service
    apply_service(service).await?;
    
    // 5. Update status
    update_status(&instance, "Ready").await?;
    
    Ok(())
}
```

## DNSZone Reconciliation

```rust
async fn reconcile_dnszone(zone: DNSZone) -> Result<()> {
    // 1. Find matching Bind9Instances
    let instances = find_instances(&zone.spec.instance_selector).await?;
    
    // 2. Generate zone file content
    let zone_content = generate_zone_file(&zone)?;
    
    // 3. Update zone on each instance
    for instance in instances {
        update_zone_on_instance(&instance, &zone_content).await?;
    }
    
    // 4. Update status
    update_zone_status(&zone, instances.len()).await?;
    
    Ok(())
}
```

## Record Reconciliation

All record types follow similar pattern:

```rust
async fn reconcile_record(record: Record) -> Result<()> {
    // 1. Get zone
    let zone = get_zone(&record.spec.zone).await?;
    
    // 2. Add record to zone
    zone_manager.add_record(&zone, &record)?;
    
    // 3. Update status
    update_record_status(&record, "Ready").await?;
    
    Ok(())
}
```
