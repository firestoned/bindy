# Roadmap: Bidirectional DNS Record Synchronization

**Status:** 📋 Planning
**Priority:** High
**Impact:** Enables true Kubernetes declarative state for DNS records
**Author:** Erick Bourgeois
**Date:** 2025-12-26

---

## Executive Summary

This roadmap adds **bidirectional synchronization** for DNS records, ensuring that records in BIND9 always match what's declared in Kubernetes. When records are deleted from Kubernetes (even while Bindy is down), they will be automatically removed from BIND9 on the next reconciliation loop.

### Current State (One-Way Sync)
```
Kubernetes → BIND9 ✅ (create/update only)
Kubernetes ← BIND9 ❌ (no deletion cleanup)
```

### Target State (Two-Way Sync)
```
Kubernetes → BIND9 ✅ (create/update)
Kubernetes ← BIND9 ✅ (delete orphaned records)
```

---

## Problem Statement

### Scenario: Records Deleted While Bindy is Down

```
1. User creates ARecord "www.example.com" → Record created in BIND9 ✅
2. Bindy pod crashes or is scaled to 0
3. User deletes ARecord from Kubernetes
4. Record is removed from etcd
5. Bindy comes back up
6. ❌ Record STILL EXISTS in BIND9 (orphaned)
7. ❌ No mechanism to detect or clean up orphaned record
8. Result: Stale DNS data, potential security risk
```

### Current Limitations

1. **No Finalizers on Record CRDs**
   - Records can be deleted from K8s without cleanup
   - No blocking until BIND9 is updated
   - Deletion happens immediately

2. **No Orphaned Record Detection**
   - Reconcilers only create/update records
   - No querying of existing BIND9 records
   - No comparison between K8s state and BIND9 state

3. **No Record Deletion Logic**
   - No `delete_*_record()` functions
   - No RFC 2136 DELETE operations
   - No cleanup of manually created records

---

## Solution Architecture

### Phase 1: Enable Record Queries via ACL

**Goal:** Allow Bindy to query DNS records from localhost

#### Changes to BIND9 Configuration

**File: `src/bind9_resources.rs` (named.conf.options generation)**

Add trusted ACL for localhost queries:

```bind
acl trusted-any-clients {
    127.0.0.1;       # IPv4 localhost
    ::1;             # IPv6 localhost
};

options {
    directory "/var/cache/bind";

    // Existing options...
    recursion no;
    allow-query { any; };
    allow-transfer { trusted-transfer; };

    // NEW: Allow ANY queries from localhost for record enumeration
    allow-query-any { trusted-any-clients; };

    // Bindcar sidecar configuration
    listen-on { any; };
    listen-on-v6 { any; };
}
```

**Why this is safe:**
- `allow-query-any` only permits localhost (127.0.0.1, ::1)
- Bindcar sidecar runs in the same pod (shares localhost)
- No external access to ANY queries
- Prevents DNS amplification attacks

#### Implementation Details

**Constant Definition** (`src/constants.rs`):
```rust
/// ACL name for localhost clients allowed to query ANY records
pub const ACL_TRUSTED_ANY_CLIENTS: &str = "trusted-any-clients";

/// ACL members for trusted ANY query clients (localhost only)
pub const ACL_TRUSTED_ANY_MEMBERS: &[&str] = &["127.0.0.1", "::1"];
```

**ConfigMap Generation** (`src/bind9_resources.rs`):
```rust
fn generate_named_conf_options(spec: &Bind9InstanceSpec) -> String {
    let mut config = String::new();

    // Add trusted-any-clients ACL
    config.push_str("acl trusted-any-clients {\n");
    for member in crate::constants::ACL_TRUSTED_ANY_MEMBERS {
        config.push_str(&format!("    {};\n", member));
    }
    config.push_str("};\n\n");

    // Add existing ACLs...
    config.push_str(&generate_trusted_transfer_acl(spec));

    config.push_str("options {\n");
    config.push_str("    directory \"/var/cache/bind\";\n");
    // ... existing options ...
    config.push_str("    allow-query-any { trusted-any-clients; };\n");
    config.push_str("};\n");

    config
}
```

---

### Phase 2: Query Existing Records via DNS

**Goal:** Enumerate all records in a zone to find orphans

#### DNS Query Strategy

Use **hickory-client** (already a dependency) to query records:

```rust
use hickory_client::client::{Client, SyncClient};
use hickory_client::udp::UdpClientConnection;
use hickory_client::rr::{DNSClass, Name, RecordType};

/// Query all records of a specific type in a zone
async fn query_zone_records(
    zone_name: &str,
    record_type: RecordType,
    nameserver: &str,
) -> Result<Vec<String>> {
    let address = format!("{}:53", nameserver).parse()?;
    let conn = UdpClientConnection::new(address)?;
    let client = SyncClient::new(conn);

    let name = Name::from_utf8(zone_name)?;
    let response = client.query(&name, DNSClass::IN, record_type)?;

    let mut records = Vec::new();
    for answer in response.answers() {
        if let Some(rdata) = answer.data() {
            records.push(format!("{}", rdata));
        }
    }

    Ok(records)
}
```

#### Query Execution Points

**Location:** Record reconcilers (`src/reconcilers/records.rs`)

**When to query:**
1. **On DNSZone reconciliation** - More efficient, zone-scoped
2. **Periodically** - Every 5 minutes via reconciliation loop
3. **After pod restarts** - Check on first reconciliation

**Preferred approach: Query during DNSZone reconciliation**

Rationale:
- DNSZone already knows all its primary instances
- Can query once per zone, not once per record
- Natural place for zone-level consistency checks
- Reduces query overhead

---

### Phase 3: Implement Record Deletion Logic

**Goal:** Delete records from BIND9 when not in Kubernetes

#### RFC 2136 DELETE Operations

**File: `src/bind9/records/delete.rs` (NEW)**

```rust
// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! DNS record deletion via RFC 2136 dynamic updates.

use crate::bind9::RndcKeyData;
use anyhow::{anyhow, Result};
use hickory_client::client::{Client, SyncClient};
use hickory_client::op::UpdateMessage;
use hickory_client::rr::{DNSClass, Name, RData, Record, RecordType};
use hickory_client::udp::UdpClientConnection;
use tracing::{debug, info, warn};

/// Delete an A record from a DNS zone using RFC 2136 dynamic update.
///
/// # Arguments
///
/// * `zone_name` - The DNS zone name (e.g., "example.com")
/// * `record_name` - The record name (e.g., "www" for www.example.com)
/// * `ipv4_address` - The IPv4 address to delete
/// * `nameserver` - The nameserver endpoint (e.g., "10.0.1.10")
/// * `rndc_key` - TSIG authentication key for dynamic updates
///
/// # Returns
///
/// * `Ok(())` - If record was deleted successfully or didn't exist
/// * `Err(_)` - If deletion failed due to network or authentication errors
///
/// # Errors
///
/// Returns an error if:
/// - TSIG authentication fails
/// - Nameserver is unreachable
/// - Zone is not configured for dynamic updates
pub async fn delete_a_record(
    zone_name: &str,
    record_name: &str,
    ipv4_address: &str,
    nameserver: &str,
    rndc_key: &RndcKeyData,
) -> Result<()> {
    let fqdn = if record_name == "@" {
        zone_name.to_string()
    } else {
        format!("{}.{}", record_name, zone_name)
    };

    info!(
        "Deleting A record: {} -> {} from zone {} via {}",
        fqdn, ipv4_address, zone_name, nameserver
    );

    // Parse names
    let zone = Name::from_utf8(zone_name)?;
    let name = Name::from_utf8(&fqdn)?;

    // Create DELETE update
    let mut update = UpdateMessage::new();
    update.set_zone(zone.clone(), DNSClass::IN);

    // DELETE specific A record (name + rdata must match)
    let rdata = RData::A(ipv4_address.parse()?);
    let record = Record::from_rdata(name.clone(), 0, rdata);
    update.delete_record(record);

    // Send authenticated update
    send_tsig_update(update, nameserver, rndc_key).await?;

    info!(
        "Successfully deleted A record: {} -> {} from zone {}",
        fqdn, ipv4_address, zone_name
    );

    Ok(())
}

/// Send a TSIG-authenticated RFC 2136 update to the nameserver.
async fn send_tsig_update(
    mut update: UpdateMessage,
    nameserver: &str,
    rndc_key: &RndcKeyData,
) -> Result<()> {
    // Create TSIG signer
    let key_name = Name::from_utf8(&rndc_key.key_name)?;
    let key_bytes = base64::decode(&rndc_key.secret)?;
    let signer = hickory_client::rr::dnssec::tsig::TSig::new(
        key_name,
        hickory_client::rr::dnssec::tsig::TsigAlgorithm::HmacSha256,
        key_bytes,
    )?;

    // Sign update message
    update.sign(&signer, Utc::now().timestamp() as u64)?;

    // Send update
    let address = format!("{}:53", nameserver).parse()?;
    let conn = UdpClientConnection::new(address)?;
    let client = SyncClient::new(conn);

    let response = client.send(update)?;

    if response.response_code() != ResponseCode::NoError {
        return Err(anyhow!(
            "DELETE failed: {:?}",
            response.response_code()
        ));
    }

    Ok(())
}

// TODO: Implement delete functions for other record types:
// - delete_aaaa_record()
// - delete_txt_record()
// - delete_cname_record()
// - delete_mx_record()
// - delete_ns_record()
// - delete_srv_record()
// - delete_caa_record()
```

**File: `src/bind9/mod.rs`** (add wrapper methods):

```rust
impl Bind9Manager {
    /// Delete an A record from a DNS zone
    pub async fn delete_a_record(
        &self,
        zone_name: &str,
        record_name: &str,
        ipv4_address: &str,
        nameserver: &str,
        rndc_key: &RndcKeyData,
    ) -> Result<()> {
        records::delete::delete_a_record(
            zone_name,
            record_name,
            ipv4_address,
            nameserver,
            rndc_key,
        )
        .await
    }

    // ... similar wrappers for other record types
}
```

---

### Phase 4: Detect and Delete Orphaned Records

**Goal:** Find records in BIND9 not in Kubernetes and delete them

#### DNSZone Reconciler Enhancement

**File: `src/reconcilers/dnszone.rs`**

Add orphaned record cleanup at the end of DNSZone reconciliation:

```rust
/// Reconciles a `DNSZone` resource.
pub async fn reconcile_dnszone(
    client: Client,
    dnszone: DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    // ... existing zone creation/update logic ...

    // NEW: Clean up orphaned records
    cleanup_orphaned_records(&client, &dnszone, zone_manager).await?;

    // ... existing status update logic ...

    Ok(())
}

/// Find and delete records that exist in BIND9 but not in Kubernetes.
///
/// This implements bidirectional synchronization, ensuring BIND9 state
/// matches Kubernetes state by removing orphaned records.
///
/// # Arguments
///
/// * `client` - Kubernetes API client for querying record CRDs
/// * `dnszone` - The DNSZone to check for orphaned records
/// * `zone_manager` - BIND9 manager for deleting records
///
/// # How It Works
///
/// 1. Query all record types in BIND9 for this zone (via DNS queries to localhost)
/// 2. List all record CRDs in Kubernetes that match this zone's selector
/// 3. Compare: Find records in BIND9 that don't have corresponding K8s CRDs
/// 4. Delete orphaned records via RFC 2136 dynamic updates
///
/// # Errors
///
/// Returns an error if DNS queries fail or record deletion fails.
async fn cleanup_orphaned_records(
    client: &Client,
    dnszone: &DNSZone,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let zone_name = &dnszone.spec.zone_name;

    info!(
        "Checking for orphaned records in zone {} (namespace: {})",
        zone_name, namespace
    );

    // Get primary instances for this zone
    let cluster_ref = get_cluster_ref_from_spec(&dnszone.spec, &namespace, &dnszone.name_any())?;
    let is_cluster_provider = dnszone.spec.cluster_provider_ref.is_some();

    let primary_pods = find_all_primary_pods(
        client,
        &namespace,
        &cluster_ref,
        is_cluster_provider,
    )
    .await?;

    if primary_pods.is_empty() {
        debug!("No primary instances found for orphaned record check");
        return Ok(());
    }

    // Use first primary as query target (all primaries should have same records)
    let primary_pod = &primary_pods[0];
    let nameserver = &primary_pod.pod_ip;

    // Load RNDC key for deletions
    let rndc_key = load_rndc_key(
        client,
        &primary_pod.namespace,
        &primary_pod.instance_name,
    )
    .await?;

    // Check each record type
    cleanup_orphaned_a_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;
    cleanup_orphaned_aaaa_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;
    cleanup_orphaned_txt_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;
    cleanup_orphaned_cname_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;
    cleanup_orphaned_mx_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;
    cleanup_orphaned_ns_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;
    cleanup_orphaned_srv_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;
    cleanup_orphaned_caa_records(client, dnszone, zone_name, nameserver, &rndc_key).await?;

    Ok(())
}

/// Find and delete orphaned A records.
async fn cleanup_orphaned_a_records(
    client: &Client,
    dnszone: &DNSZone,
    zone_name: &str,
    nameserver: &str,
    rndc_key: &RndcKeyData,
) -> Result<()> {
    use hickory_client::rr::RecordType;

    // Query all A records in BIND9 for this zone
    let bind9_records = query_zone_records(zone_name, RecordType::A, nameserver).await?;

    // Get all ARecords from Kubernetes that match this zone's selector
    let k8s_records = find_matching_a_records(client, dnszone).await?;

    // Build set of expected records (name -> IP)
    let mut expected: HashMap<String, String> = HashMap::new();
    for record in k8s_records {
        let name = &record.spec.name;
        let ip = &record.spec.ipv4_address;
        expected.insert(name.clone(), ip.clone());
    }

    // Find orphaned records (exist in BIND9 but not in K8s)
    let mut orphaned_count = 0;
    for bind9_record in bind9_records {
        // Parse record: "www.example.com. 300 IN A 192.168.1.10"
        if let Some((record_name, ip_address)) = parse_a_record(&bind9_record, zone_name) {
            // Check if this record exists in Kubernetes
            if !expected.contains_key(&record_name) || expected[&record_name] != ip_address {
                warn!(
                    "Found orphaned A record in BIND9: {} -> {} (not in Kubernetes)",
                    record_name, ip_address
                );

                // Delete orphaned record
                zone_manager
                    .delete_a_record(zone_name, &record_name, &ip_address, nameserver, rndc_key)
                    .await?;

                orphaned_count += 1;
            }
        }
    }

    if orphaned_count > 0 {
        info!(
            "Cleaned up {} orphaned A record(s) from zone {}",
            orphaned_count, zone_name
        );
    }

    Ok(())
}

/// Find all ARecords in Kubernetes that match this DNSZone's selector.
async fn find_matching_a_records(
    client: &Client,
    dnszone: &DNSZone,
) -> Result<Vec<ARecord>> {
    let namespace = dnszone.namespace().unwrap_or_default();
    let api: Api<ARecord> = Api::namespaced(client.clone(), &namespace);

    // If zone has recordsFrom selectors, use them
    if let Some(records_from) = &dnszone.spec.records_from {
        let mut matching_records = Vec::new();

        for selector_config in records_from {
            if let Some(selector) = &selector_config.selector {
                // Convert LabelSelector to label selector string
                let label_selector = selector_to_string(selector);
                let lp = ListParams::default().labels(&label_selector);

                let records = api.list(&lp).await?;
                matching_records.extend(records.items);
            }
        }

        Ok(matching_records)
    } else {
        // No selector - no records should be in this zone
        Ok(Vec::new())
    }
}

/// Parse an A record from BIND9 query response.
///
/// Input: "www.example.com. 300 IN A 192.168.1.10"
/// Output: Some(("www", "192.168.1.10"))
fn parse_a_record(record_line: &str, zone_name: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = record_line.split_whitespace().collect();
    if parts.len() < 5 || parts[3] != "A" {
        return None;
    }

    let fqdn = parts[0].trim_end_matches('.');
    let ip = parts[4];

    // Extract record name (remove zone suffix)
    let zone_suffix = format!(".{}", zone_name);
    let record_name = if fqdn == zone_name {
        "@".to_string() // Zone apex
    } else if let Some(name) = fqdn.strip_suffix(&zone_suffix) {
        name.to_string()
    } else {
        return None; // Record not in this zone
    };

    Some((record_name, ip.to_string()))
}

// TODO: Implement similar cleanup functions for other record types:
// - cleanup_orphaned_aaaa_records()
// - cleanup_orphaned_txt_records()
// - cleanup_orphaned_cname_records()
// - cleanup_orphaned_mx_records()
// - cleanup_orphaned_ns_records()
// - cleanup_orphaned_srv_records()
// - cleanup_orphaned_caa_records()
```

---

### Phase 5: Add Finalizers to Record CRDs

**Goal:** Block record deletion until BIND9 cleanup completes

#### Finalizer Implementation

**File: `src/labels.rs`** (add finalizer constants):

```rust
/// Finalizer for DNS record cleanup
pub const FINALIZER_A_RECORD: &str = "arecord.bindy.firestoned.io/finalizer";
pub const FINALIZER_AAAA_RECORD: &str = "aaaarecord.bindy.firestoned.io/finalizer";
pub const FINALIZER_TXT_RECORD: &str = "txtrecord.bindy.firestoned.io/finalizer";
pub const FINALIZER_CNAME_RECORD: &str = "cnamerecord.bindy.firestoned.io/finalizer";
pub const FINALIZER_MX_RECORD: &str = "mxrecord.bindy.firestoned.io/finalizer";
pub const FINALIZER_NS_RECORD: &str = "nsrecord.bindy.firestoned.io/finalizer";
pub const FINALIZER_SRV_RECORD: &str = "srvrecord.bindy.firestoned.io/finalizer";
pub const FINALIZER_CAA_RECORD: &str = "caarecord.bindy.firestoned.io/finalizer";
```

**File: `src/reconcilers/records.rs`** (add deletion functions):

```rust
/// Delete an A record from BIND9.
///
/// Called by finalizer when ARecord is deleted from Kubernetes.
/// Removes the record from all primary BIND9 instances.
pub async fn delete_a_record(
    client: Client,
    record: ARecord,
    zone_manager: &crate::bind9::Bind9Manager,
) -> Result<()> {
    let namespace = record.namespace().unwrap_or_default();
    let name = record.name_any();

    info!("Deleting ARecord: {}/{}", namespace, name);

    // Get zone from annotation
    let Some(zone_fqdn) = get_zone_from_annotation(&record) else {
        info!("ARecord {}/{} has no zone annotation, skipping deletion", namespace, name);
        return Ok(());
    };

    // Get zone info
    let (zone_name, cluster_ref, is_cluster_provider) =
        get_zone_info(&client, &namespace, &zone_fqdn).await?;

    // Get all primary instances
    let primary_pods = find_all_primary_pods(
        &client,
        &namespace,
        &cluster_ref,
        is_cluster_provider,
    )
    .await?;

    if primary_pods.is_empty() {
        warn!("No primary instances found for ARecord deletion");
        return Ok(());
    }

    // Delete from all primaries
    for pod in primary_pods {
        let rndc_key = load_rndc_key(&client, &pod.namespace, &pod.instance_name).await?;

        zone_manager
            .delete_a_record(
                &zone_name,
                &record.spec.name,
                &record.spec.ipv4_address,
                &pod.pod_ip,
                &rndc_key,
            )
            .await?;

        info!(
            "Deleted A record {} from primary {} (pod: {})",
            record.spec.name, pod.instance_name, pod.pod_ip
        );
    }

    info!("Successfully deleted ARecord {}/{} from all primaries", namespace, name);

    Ok(())
}

// TODO: Implement delete functions for other record types
```

**File: `src/main.rs`** (update controller to use finalizers):

```rust
/// Run the `ARecord` controller with finalizer support
async fn run_arecord_controller(client: Client, bind9_manager: Arc<Bind9Manager>) -> Result<()> {
    use bindy::labels::FINALIZER_A_RECORD;

    info!("Starting ARecord controller with finalizer support");

    let api = Api::<ARecord>::all(client.clone());
    let watcher_config = Config::default().any_semantic();

    Controller::new(api, watcher_config)
        .run(
            |record, ctx| async move {
                reconcile_arecord_with_finalizer(record, ctx).await
            },
            error_policy,
            Arc::new((client, bind9_manager)),
        )
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

/// Reconcile wrapper for `ARecord` with finalizer support
async fn reconcile_arecord_with_finalizer(
    record: Arc<ARecord>,
    ctx: Arc<(Client, Arc<Bind9Manager>)>,
) -> Result<Action, ReconcileError> {
    use bindy::constants::KIND_A_RECORD;
    use bindy::labels::FINALIZER_A_RECORD;
    use bindy::reconcilers::{delete_a_record, reconcile_a_record};

    const FINALIZER_NAME: &str = FINALIZER_A_RECORD;
    let start = std::time::Instant::now();

    let client = ctx.0.clone();
    let bind9_manager = ctx.1.clone();
    let namespace = record.namespace().unwrap_or_default();
    let api: Api<ARecord> = Api::namespaced(client.clone(), &namespace);

    // Handle deletion with finalizer
    let result = finalizer(&api, FINALIZER_NAME, record.clone(), |event| async {
        match event {
            finalizer::Event::Apply(rec) => {
                // Create or update the record
                reconcile_a_record(client.clone(), (*rec).clone())
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully reconciled ARecord: {}", rec.name_any());
                Ok(Action::requeue(Duration::from_secs(300)))
            }
            finalizer::Event::Cleanup(rec) => {
                // Delete the record from BIND9
                delete_a_record(client.clone(), (*rec).clone(), &bind9_manager)
                    .await
                    .map_err(ReconcileError::from)?;
                info!("Successfully deleted ARecord from BIND9: {}", rec.name_any());
                metrics::record_resource_deleted(KIND_A_RECORD);
                Ok(Action::await_change())
            }
        }
    })
    .await;

    let duration = start.elapsed();
    if result.is_ok() {
        metrics::record_reconciliation_success(KIND_A_RECORD, duration);
    } else {
        metrics::record_reconciliation_error(KIND_A_RECORD, duration);
    }

    result
}

// TODO: Update all other record controllers similarly
```

---

## Implementation Phases

### Phase 1: ACL Configuration (Week 1)
**Estimated effort:** 2-3 days

- [ ] Add `ACL_TRUSTED_ANY_CLIENTS` constant
- [ ] Update `named.conf.options` generation
- [ ] Add `allow-query-any { trusted-any-clients; }` directive
- [ ] Update tests for ConfigMap generation
- [ ] Update documentation

**Deliverables:**
- Updated BIND9 ConfigMaps with ACL
- Unit tests passing
- Documentation updated

### Phase 2: DNS Query Implementation (Week 1-2)
**Estimated effort:** 3-4 days

- [ ] Add `query_zone_records()` function using hickory-client
- [ ] Implement record parsing helpers (parse_a_record, etc.)
- [ ] Add unit tests for DNS queries
- [ ] Add integration tests with real BIND9
- [ ] Document query strategy

**Deliverables:**
- Working DNS query implementation
- Tests passing
- Query documentation

### Phase 3: Record Deletion Functions (Week 2-3)
**Estimated effort:** 5-7 days

- [ ] Create `src/bind9/records/delete.rs`
- [ ] Implement `delete_a_record()` with RFC 2136
- [ ] Implement deletion for all 8 record types
- [ ] Add TSIG authentication for deletes
- [ ] Unit tests for deletion logic
- [ ] Integration tests with BIND9

**Deliverables:**
- Complete deletion module
- All tests passing
- Deletion documented

### Phase 4: Orphaned Record Cleanup (Week 3-4)
**Estimated effort:** 7-10 days

- [ ] Add `cleanup_orphaned_records()` to DNSZone reconciler
- [ ] Implement `find_matching_*_records()` for each type
- [ ] Add orphan detection logic
- [ ] Implement deletion of orphaned records
- [ ] Add metrics for orphaned record cleanup
- [ ] Add logging and observability
- [ ] Integration tests for full bidirectional sync

**Deliverables:**
- Bidirectional sync working
- Orphaned records cleaned up
- Metrics and logging in place
- Integration tests passing

### Phase 5: Finalizers (Week 4-5)
**Estimated effort:** 5-7 days

- [ ] Add finalizer constants for all record types
- [ ] Implement `delete_*_record()` reconcilers
- [ ] Update all 8 record controllers to use finalizers
- [ ] Add finalizer removal logic
- [ ] Test finalizer blocking behavior
- [ ] Test deletion while Bindy is down
- [ ] Update CRD documentation

**Deliverables:**
- Finalizers on all record types
- Deletion blocking until cleanup
- Tests for down-time scenarios
- Updated documentation

---

## Testing Strategy

### Unit Tests

**File: `src/bind9_resources_tests.rs`**
```rust
#[test]
fn test_named_conf_options_includes_trusted_any_acl() {
    let config = generate_named_conf_options(&default_spec());
    assert!(config.contains("acl trusted-any-clients"));
    assert!(config.contains("127.0.0.1"));
    assert!(config.contains("::1"));
    assert!(config.contains("allow-query-any { trusted-any-clients; }"));
}
```

**File: `src/bind9/records/delete_tests.rs`**
```rust
#[tokio::test]
async fn test_delete_a_record_success() {
    // Mock BIND9 server
    // Send DELETE request
    // Verify record removed
}

#[tokio::test]
async fn test_delete_nonexistent_record() {
    // Should succeed (idempotent)
}
```

### Integration Tests

**File: `tests/orphaned_records_integration.rs`**

```rust
/// Test orphaned record cleanup workflow
#[tokio::test]
async fn test_orphaned_record_cleanup() {
    // 1. Create DNSZone and ARecord in K8s
    // 2. Verify record created in BIND9
    // 3. Manually add extra record to BIND9 (orphan)
    // 4. Trigger DNSZone reconciliation
    // 5. Verify orphaned record was deleted
    // 6. Verify K8s record still exists
}

/// Test record deletion while Bindy is down
#[tokio::test]
async fn test_deletion_while_bindy_down() {
    // 1. Create ARecord in K8s
    // 2. Verify created in BIND9
    // 3. Stop Bindy controller
    // 4. Delete ARecord from K8s (should be blocked by finalizer)
    // 5. Start Bindy controller
    // 6. Verify finalizer cleanup runs
    // 7. Verify record deleted from BIND9
    // 8. Verify finalizer removed from ARecord
    // 9. Verify ARecord deleted from K8s
}
```

### Manual Testing Checklist

- [ ] Deploy DNSZone and records
- [ ] Verify ACL in named.conf.options
- [ ] Query records from localhost (dig @127.0.0.1)
- [ ] Delete record from K8s while Bindy running
- [ ] Verify record deleted from BIND9
- [ ] Manually add record to BIND9
- [ ] Trigger reconciliation
- [ ] Verify orphan deleted
- [ ] Scale Bindy to 0
- [ ] Delete record from K8s
- [ ] Scale Bindy back up
- [ ] Verify finalizer cleanup runs
- [ ] Verify record deleted from BIND9

---

## Security Considerations

### ACL Restrictions
- ✅ `allow-query-any` only permits localhost (127.0.0.1, ::1)
- ✅ No external access to ANY queries
- ✅ Prevents DNS amplification attacks
- ✅ Bindcar sidecar shares pod network namespace

### TSIG Authentication
- ✅ All DELETE operations require TSIG authentication
- ✅ RNDC keys stored in Kubernetes Secrets
- ✅ Keys loaded per-instance, not shared
- ✅ Failed auth prevents deletions

### Deletion Safety
- ❓ What if orphan detection has a bug?
  - **Mitigation:** Add dry-run mode with logging only
  - **Mitigation:** Require manual approval for first run
  - **Mitigation:** Add metrics for deletion counts

- ❓ What if we delete SOA/NS records?
  - **Mitigation:** Blacklist SOA, NS records in deletion logic
  - **Mitigation:** Only delete records matching CRD types

- ❓ What if BIND9 is temporarily unreachable?
  - **Mitigation:** Retry with exponential backoff
  - **Mitigation:** Don't fail zone reconciliation on query errors
  - **Mitigation:** Log warnings, continue reconciliation

---

## Metrics and Observability

### New Metrics

```rust
// Orphaned record metrics
bindy_orphaned_records_detected_total{zone="example.com",type="A"} 3
bindy_orphaned_records_deleted_total{zone="example.com",type="A"} 3
bindy_orphaned_records_delete_failures_total{zone="example.com",type="A"} 0

// Record deletion metrics (finalizers)
bindy_record_deletions_total{type="ARecord"} 10
bindy_record_deletion_duration_seconds{type="ARecord"} 0.234

// Query metrics
bindy_dns_queries_total{zone="example.com",type="A"} 5
bindy_dns_query_failures_total{zone="example.com",type="A"} 0
```

### Logging

```rust
// Orphan detection
info!("Checking for orphaned records in zone {}", zone_name);
warn!("Found orphaned A record: {} -> {} (not in Kubernetes)", name, ip);
info!("Cleaned up {} orphaned record(s) from zone {}", count, zone_name);

// Deletion
info!("Deleting ARecord: {}/{}", namespace, name);
info!("Deleted A record {} from primary {}", name, instance_name);
info!("Successfully deleted ARecord from all primaries");

// Finalizer
info!("Finalizer cleanup started for ARecord: {}", name);
info!("Successfully deleted ARecord from BIND9: {}", name);
```

---

## Documentation Updates

### User Guide Updates

**File: `docs/src/guide/records-guide.md`**

Add section:
```markdown
## Record Deletion and Cleanup

Bindy ensures bidirectional synchronization between Kubernetes and BIND9:

### Automatic Deletion
When you delete a DNS record from Kubernetes, Bindy automatically:
1. Blocks the deletion using a finalizer
2. Removes the record from all primary BIND9 instances
3. Waits for confirmation from BIND9
4. Removes the finalizer
5. Allows Kubernetes to complete the deletion

### Orphaned Record Cleanup
Bindy automatically detects and removes orphaned records:
- Records created manually in BIND9
- Records left behind after Bindy downtime
- Records from previous misconfigurations

This happens during DNSZone reconciliation (every 5 minutes by default).
```

### Operations Guide Updates

**File: `docs/src/operations/troubleshooting.md`**

Add troubleshooting section:
```markdown
## Record Deletion Issues

### Finalizer Stuck on Record
If a record is stuck deleting with a finalizer:

1. Check Bindy logs for errors
2. Verify BIND9 instances are reachable
3. Manually remove record from BIND9 if needed
4. Remove finalizer to complete deletion

### Orphaned Records Not Cleaned Up
If orphaned records remain in BIND9:

1. Check DNSZone reconciliation logs
2. Verify ACL allows localhost queries
3. Test DNS queries manually: `dig @127.0.0.1 example.com ANY`
4. Check TSIG authentication
```

---

## Risks and Mitigations

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Accidental deletion of valid records | High | Low | Add dry-run mode, extensive testing, metrics |
| Performance impact from DNS queries | Medium | Medium | Query once per zone, cache results, configurable interval |
| BIND9 temporarily unreachable | Medium | Medium | Retry logic, don't fail reconciliation, logging |
| Bug in orphan detection logic | High | Low | Comprehensive tests, code review, gradual rollout |
| SOA/NS records accidentally deleted | High | Very Low | Blacklist critical record types, only delete CRD types |

---

## Success Criteria

### Must Have (MVP)
- [x] ACL configuration working
- [x] DNS queries functional
- [x] Record deletion via RFC 2136
- [x] Orphaned record detection
- [x] Orphaned record cleanup
- [x] Finalizers on all record types
- [x] Tests passing
- [x] Documentation updated

### Should Have
- [ ] Metrics for orphaned records
- [ ] Dry-run mode for safety
- [ ] Configurable cleanup interval
- [ ] Manual approval for first run

### Nice to Have
- [ ] Dashboard for orphaned records
- [ ] Alerts for high deletion counts
- [ ] Audit log for deletions
- [ ] Record resurrection detection

---

## Timeline

**Total Estimated Time:** 4-5 weeks

- **Week 1:** Phase 1 & 2 (ACL + DNS queries)
- **Week 2:** Phase 3 (Record deletion)
- **Week 3-4:** Phase 4 (Orphaned cleanup)
- **Week 4-5:** Phase 5 (Finalizers)

**Dependencies:**
- No external dependencies
- Uses existing hickory-client library
- No API changes required

**Rollout Strategy:**
- Develop in feature branch
- Test in dev cluster first
- Add feature flag for gradual rollout
- Monitor metrics closely
- Document rollback procedure

---

## Questions and Open Items

1. **Cleanup Interval:** How often should orphaned record checks run?
   - Current: Every DNSZone reconciliation (5 minutes)
   - Alternative: Configurable per-zone annotation

2. **Dry-Run Mode:** Should we add a dry-run flag for safety?
   - Proposal: Add `bindy.firestoned.io/orphan-cleanup: "enabled|dry-run|disabled"` annotation

3. **Blacklisted Record Types:** Which records should never be deleted?
   - Proposal: SOA, NS (managed by DNSZone)
   - Proposal: Any record without matching CRD type

4. **Metrics Retention:** How long to track deletion metrics?
   - Proposal: Use Prometheus default (15 days)

---

## Related Work

- **[2025-12-26 22:45]** - Bind9Cluster declarative reconciliation (managed instances)
- **[2025-12-26 23:15]** - DNSZone declarative reconciliation (zones)
- **[2025-12-26 23:45]** - DNS Records declarative reconciliation (records)

This roadmap completes the **declarative reconciliation trilogy** by adding deletion support to complement the create/update logic implemented in the previous changes.

---

## Approval and Sign-off

**Created by:** Erick Bourgeois
**Date:** 2025-12-26
**Status:** 📋 Planning - Awaiting approval to begin implementation

**Approved by:** _________________
**Approval Date:** _________________
