# Reconciliation Logic

Detailed reconciliation logic for each resource type.

## Status Update Optimization

All reconcilers implement status change detection to prevent tight reconciliation loops. Before updating the status subresource, each reconciler checks if the status has actually changed. This prevents unnecessary API calls and reconciliation cycles.

**Status is only updated when:**
- Condition type changes
- Status value changes
- Message changes
- Status doesn't exist yet

This optimization is implemented in:
- `Bind9Cluster` reconciler ([src/reconcilers/bind9cluster.rs:394-430](https://github.com/firestoned/bindy/blob/main/src/reconcilers/bind9cluster.rs#L394-L430))
- `Bind9Instance` reconciler ([src/reconcilers/bind9instance.rs:736-758](https://github.com/firestoned/bindy/blob/main/src/reconcilers/bind9instance.rs#L736-L758))
- `DNSZone` reconciler ([src/reconcilers/dnszone.rs:535-565](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone.rs#L535-L565))
- All record reconcilers ([src/reconcilers/records.rs:1032-1072](https://github.com/firestoned/bindy/blob/main/src/reconcilers/records.rs#L1032-L1072))

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

DNSZone reconciliation uses granular status updates to provide real-time progress visibility and better error reporting. The reconciliation follows a multi-phase approach with status updates at each phase.

### Module Structure

The DNSZone reconciler has been refactored into a modular architecture for better maintainability and testability. As of v0.3.0, the reconciler is organized into focused modules:

**Main Orchestration:**
- [dnszone.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone.rs) - Main reconciliation entry point and orchestration logic

**Core Modules:**
- [dnszone/validation.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/validation.rs) - Zone validation (duplicate detection, selector matching)
- [dnszone/discovery.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/discovery.rs) - Instance and resource discovery helpers
- [dnszone/primary.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/primary.rs) - Primary zone configuration logic
- [dnszone/secondary.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/secondary.rs) - Secondary zone configuration logic

**Support Modules:**
- [dnszone/bind9_config.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/bind9_config.rs) - BIND9 configuration generation
- [dnszone/status_helpers.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/status_helpers.rs) - Status update helpers
- [dnszone/helpers.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/helpers.rs) - Shared utility functions
- [dnszone/cleanup.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/cleanup.rs) - Resource cleanup and finalizer logic

**Shared:**
- [dnszone/types.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/types.rs) - Common types (DuplicateZoneInfo, EndpointAddress, etc.)
- [dnszone/constants.rs](https://github.com/firestoned/bindy/blob/main/src/reconcilers/dnszone/constants.rs) - Shared constants (finalizer names, timeouts)

**Benefits:**
- **Code Organization**: Related functionality grouped logically (validation, discovery, configuration)
- **Testability**: Each module can be tested independently with focused test files
- **Maintainability**: Changes isolated to specific modules (e.g., validation logic separate from configuration)
- **Readability**: Smaller, focused files (~200-500 lines) vs. monolithic reconciler (2000+ lines)

This refactoring was completed in Phases 1.1 and 1.2, extracting 66% of code from the main reconciler into focused modules.

### Reconciliation Flow

```rust
async fn reconcile_dnszone(zone: DNSZone) -> Result<()> {
    // Phase 1: Set Progressing status before primary reconciliation
    update_condition(&zone, "Progressing", "True", "PrimaryReconciling",
                     "Configuring zone on primary instances").await?;

    // Phase 2: Configure zone on primary instances
    let primary_count = add_dnszone(client, &zone, zone_manager).await
        .map_err(|e| {
            // On failure: Set Degraded status (primary failure is fatal)
            update_condition(&zone, "Degraded", "True", "PrimaryFailed",
                           &format!("Failed to configure zone on primaries: {}", e)).await?;
            e
        })?;

    // Phase 3: Set Progressing status after primary success
    update_condition(&zone, "Progressing", "True", "PrimaryReconciled",
                     &format!("Configured on {} primary server(s)", primary_count)).await?;

    // Phase 4: Set Progressing status before secondary reconciliation
    let secondary_msg = format!("Configured on {} primary server(s), now configuring secondaries", primary_count);
    update_condition(&zone, "Progressing", "True", "SecondaryReconciling", &secondary_msg).await?;

    // Phase 5: Configure zone on secondary instances (non-fatal if fails)
    match add_dnszone_to_secondaries(client, &zone, zone_manager).await {
        Ok(secondary_count) => {
            // Phase 6: Success - Set Ready status
            let msg = format!("Configured on {} primary server(s) and {} secondary server(s)",
                            primary_count, secondary_count);
            update_status_with_secondaries(&zone, "Ready", "True", "ReconcileSucceeded",
                                          &msg, secondary_ips).await?;
        }
        Err(e) => {
            // Phase 6: Partial success - Set Degraded status (primaries work, secondaries failed)
            let msg = format!("Configured on {} primary server(s), but secondary configuration failed: {}",
                            primary_count, e);
            update_status_with_secondaries(&zone, "Degraded", "True", "SecondaryFailed",
                                          &msg, secondary_ips).await?;
        }
    }

    Ok(())
}
```

### Status Conditions

DNSZone reconciliation uses three condition types:

- **`Progressing`** - During reconciliation phases
  - Reason: `PrimaryReconciling` - Before primary configuration
  - Reason: `PrimaryReconciled` - After primary configuration succeeds
  - Reason: `SecondaryReconciling` - Before secondary configuration
  - Reason: `SecondaryReconciled` - After secondary configuration succeeds

- **`Ready`** - Successful reconciliation
  - Reason: `ReconcileSucceeded` - All phases completed successfully

- **`Degraded`** - Partial or complete failure
  - Reason: `PrimaryFailed` - Primary configuration failed (fatal, reconciliation aborts)
  - Reason: `SecondaryFailed` - Secondary configuration failed (non-fatal, primaries still work)

### Benefits

1. **Real-time progress visibility** - Users can see which phase is running
2. **Better error reporting** - Know exactly which phase failed (primary vs secondary)
3. **Graceful degradation** - Secondary failures don't break the zone (primaries still work)
4. **Accurate status** - Endpoint counts reflect actual configured servers

## Record Reconciliation

All record types (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA) follow a consistent pattern with granular status updates for better observability.

### Reconciliation Flow

```rust
async fn reconcile_record(record: Record) -> Result<()> {
    // Phase 1: Set Progressing status before configuration
    update_record_status(&record, "Progressing", "True", "RecordReconciling",
                        "Configuring A record on zone endpoints").await?;

    // Phase 2: Get zone and configure record on all endpoints
    let zone = get_zone(&record.spec.zone).await?;

    match add_record_to_all_endpoints(&zone, &record).await {
        Ok(endpoint_count) => {
            // Phase 3: Success - Set Ready status with endpoint count
            let msg = format!("Record configured on {} endpoint(s)", endpoint_count);
            update_record_status(&record, "Ready", "True", "ReconcileSucceeded", &msg).await?;
        }
        Err(e) => {
            // Phase 3: Failure - Set Degraded status with error details
            let msg = format!("Failed to configure record: {}", e);
            update_record_status(&record, "Degraded", "True", "RecordFailed", &msg).await?;
            return Err(e);
        }
    }

    Ok(())
}
```

### Status Conditions

All DNS record types use three condition types:

- **`Progressing`** - During record configuration
  - Reason: `RecordReconciling` - Before adding record to zone endpoints

- **`Ready`** - Successful configuration
  - Reason: `ReconcileSucceeded` - Record configured on all endpoints
  - Message includes count of configured endpoints (e.g., "Record configured on 3 endpoint(s)")

- **`Degraded`** - Configuration failure
  - Reason: `RecordFailed` - Failed to configure record (includes error details)

### Benefits

1. **Real-time progress** - See when records are being configured
2. **Better debugging** - Know immediately if/why a record failed
3. **Accurate reporting** - Status shows exact number of endpoints configured
4. **Consistent with zones** - Same status pattern as DNSZone reconciliation

### Supported Record Types

All 8 record types use this granular status approach:
- **A** - IPv4 address records
- **AAAA** - IPv6 address records
- **CNAME** - Canonical name (alias) records
- **MX** - Mail exchange records
- **TXT** - Text records (SPF, DKIM, DMARC, etc.)
- **NS** - Nameserver delegation records
- **SRV** - Service location records
- **CAA** - Certificate authority authorization records
