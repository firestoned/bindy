# Bindy: Implementing kube-condition for Enhanced Status Conditions

## Executive Summary

This roadmap outlines the implementation of a `kube-condition` derive macro library into the bindy BIND9 Kubernetes operator. The goal is to provide rich, consistent, and user-friendly status conditions that give operators immediate visibility into resource state without digging through logs.

**Repository**: https://github.com/firestoned/bindy

---

## Part 1: Background & Analysis

### Current bindy Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Kubernetes Cluster                       │
│                                                              │
│  ┌──────────────────┐         ┌──────────────────────────┐  │
│  │   bindy          │         │   BIND9 Pod              │  │
│  │   operator       │ HTTP    │  ┌────────┐  ┌────────┐  │  │
│  │                  │────────▶│  │bindcar │──│ bind9  │  │  │
│  │  (watches CRDs)  │         │  │ :8953  │  │ :53    │  │  │
│  └──────────────────┘         │  └────────┘  └────────┘  │  │
│           │                   │       │           ▲      │  │
│           │                   │       │    RNDC   │      │  │
│           ▼                   │       │   :953    │      │  │
│  ┌──────────────────┐         │       └───────────┘      │  │
│  │   DNSZone CRD    │         │       shared volume      │  │
│  │   DNSRecord CRD  │         └──────────────────────────┘  │
│  └──────────────────┘                                       │
└─────────────────────────────────────────────────────────────┘
```

**Components**:
- **bindy**: Rust Kubernetes operator (kube-rs, tokio)
- **bindcar**: REST API sidecar for RNDC operations
- **DNSZone**: CRD for zone management
- **DNSRecord**: CRD for individual DNS records

### Why kube-condition?

Currently, status conditions in Kubernetes operators require:
1. Manual construction of `Condition` structs
2. Repetitive logic for `lastTransitionTime` handling
3. Inconsistent reason/message formatting across error types
4. No compile-time guarantees that errors map to valid conditions

**kube-condition provides**:
- Derive macro that auto-generates condition mappings from error enums
- Consistent severity levels and retry behavior
- Automatic `observedGeneration` and timestamp handling
- Type-safe condition construction

---

## Part 2: Recommended Status Condition Types

### Core Conditions (All Resources)

| Type | Purpose | Positive Polarity |
|------|---------|-------------------|
| `Ready` | Overall operational readiness | True = resource is fully operational |
| `Progressing` | Reconciliation in flight | True = actively working |

### DNSZone-Specific Conditions

| Type | Purpose | Example Reasons |
|------|---------|-----------------|
| `Synchronized` | Zone matches BIND9 state | `ZoneInSync`, `DriftDetected`, `SyncPending` |
| `Available` | Zone is serving queries | `ZoneServingQueries`, `ZoneNotLoaded` |
| `SecondariesInSync` | Zone transfers complete | `TransferComplete`, `TransferPending`, `TransferFailed` |
| `Validated` | Zone file syntax valid | `SyntaxValid`, `SyntaxError` |
| `DnssecReady` | DNSSEC signing operational | `SigningActive`, `KeyMissing`, `SigningFailed` |

### DNSRecord-Specific Conditions

| Type | Purpose | Example Reasons |
|------|---------|-----------------|
| `Synchronized` | Record matches BIND9 | `RecordInSync`, `UpdatePending` |
| `DependenciesReady` | Parent zone exists/ready | `ZoneReady`, `ZoneNotFound`, `ZoneNotReady` |
| `Validated` | Record syntax valid | `SyntaxValid`, `InvalidRdata`, `UnsupportedType` |

### Condition State Matrix

| Scenario | Ready | Synchronized | DependenciesReady | Progressing |
|----------|-------|--------------|-------------------|-------------|
| Happy path | True | True | True | False |
| Initial creation | False | False | True | True |
| Zone doesn't exist | False | Unknown | False | False |
| RNDC unreachable | Unknown | Unknown | True | True |
| Syntax error | False | False | True | False |
| Update in progress | True | False | True | True |

---

## Part 3: kube-condition Library Design

### Crate Structure

```
kube-condition/
├── Cargo.toml
├── src/
│   └── lib.rs              # Runtime traits and helpers
│
kube-condition-derive/
├── Cargo.toml
└── src/
    └── lib.rs              # Proc macro implementation
```

### Core Trait Definition

```rust
// kube-condition/src/lib.rs

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use chrono::{DateTime, Utc};

/// Severity levels for conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// Information needed to construct a Condition
#[derive(Debug, Clone)]
pub struct ConditionInfo {
    pub condition_type: String,
    pub status: String,           // "True", "False", "Unknown"
    pub reason: String,
    pub message: String,
    pub observed_generation: Option<i64>,
}

/// Trait implemented by the derive macro
pub trait StatusCondition: std::error::Error {
    /// Generate condition info from this error
    fn to_condition_info(&self) -> ConditionInfo;
    
    /// Get the severity level
    fn severity(&self) -> Severity;
    
    /// Should this error trigger a retry?
    fn is_retryable(&self) -> bool;
    
    /// Custom requeue duration (None = use default)
    fn requeue_duration(&self) -> Option<std::time::Duration>;
}

/// Extension trait for building Conditions
pub trait ConditionExt {
    fn from_error<E: StatusCondition>(
        error: &E,
        observed_generation: Option<i64>,
    ) -> Condition;
    
    fn ready(observed_generation: Option<i64>) -> Condition;
}

impl ConditionExt for Condition {
    fn from_error<E: StatusCondition>(
        error: &E,
        observed_generation: Option<i64>,
    ) -> Condition {
        let info = error.to_condition_info();
        Condition {
            type_: info.condition_type,
            status: info.status,
            reason: info.reason,
            message: info.message,
            last_transition_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc::now()
            )),
            observed_generation,
        }
    }
    
    fn ready(observed_generation: Option<i64>) -> Condition {
        Condition {
            type_: "Ready".to_string(),
            status: "True".to_string(),
            reason: "ReconcileSucceeded".to_string(),
            message: "Resource reconciled successfully".to_string(),
            last_transition_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc::now()
            )),
            observed_generation,
        }
    }
}
```

### Derive Macro Usage

```rust
// bindy/src/error.rs

use kube_condition::StatusCondition;
use thiserror::Error;

#[derive(Error, Debug, StatusCondition)]
#[condition(default_type = "Ready")]
pub enum ZoneReconcileError {
    // =========================================================================
    // RNDC/BIND9 communication errors
    // =========================================================================
    
    #[error("Failed to connect to BIND9 server at {address}: {source}")]
    #[condition(
        reason = "RndcConnectionFailed", 
        severity = "error", 
        retryable = true, 
        requeue_secs = 30
    )]
    RndcConnection {
        address: String,
        #[source]
        source: std::io::Error,
    },

    #[error("RNDC command '{command}' failed: {message}")]
    #[condition(
        reason = "RndcCommandFailed", 
        severity = "error", 
        retryable = true, 
        requeue_secs = 15
    )]
    RndcCommand { command: String, message: String },

    #[error("RNDC authentication failed - check secret '{secret_name}'")]
    #[condition(
        reason = "RndcAuthFailed", 
        severity = "error", 
        retryable = false  // User must fix the secret
    )]
    RndcAuth { secret_name: String },

    // =========================================================================
    // Zone validation errors
    // =========================================================================

    #[error("Zone file syntax error for '{zone}': {details}")]
    #[condition(
        type = "Validated",  // Override default_type
        reason = "ZoneSyntaxError", 
        severity = "error", 
        retryable = false
    )]
    ZoneSyntax { zone: String, details: String },

    #[error("Invalid SOA parameters: {message}")]
    #[condition(
        type = "Validated",
        reason = "InvalidSOA", 
        retryable = false
    )]
    InvalidSoa { message: String },

    // =========================================================================
    // Synchronization errors
    // =========================================================================

    #[error("Zone '{zone}' not found on BIND9 server")]
    #[condition(
        type = "Synchronized",
        reason = "ZoneNotFound", 
        severity = "warning",
        retryable = true,
        requeue_secs = 60
    )]
    ZoneNotFound { zone: String },

    #[error("Zone transfer to secondary '{secondary}' failed: {reason}")]
    #[condition(
        type = "SecondariesInSync",
        reason = "TransferFailed",
        severity = "warning",
        retryable = true,
        requeue_secs = 120
    )]
    ZoneTransferFailed { secondary: String, reason: String },

    // =========================================================================
    // DNSSEC errors  
    // =========================================================================

    #[error("DNSSEC key '{key_id}' not found in secret '{secret}'")]
    #[condition(
        type = "DnssecReady",
        reason = "KeyNotFound",
        severity = "error",
        retryable = false
    )]
    DnssecKeyNotFound { key_id: String, secret: String },

    #[error("DNSSEC signing failed: {0}")]
    #[condition(
        type = "DnssecReady",
        reason = "SigningFailed",
        retryable = true,
        requeue_secs = 300
    )]
    DnssecSigning(String),

    // =========================================================================
    // Kubernetes API errors
    // =========================================================================

    #[error("Kubernetes API error: {0}")]
    #[condition(
        reason = "KubernetesApiError",
        severity = "error",
        retryable = true,
        requeue_secs = 5
    )]
    KubeApi(#[from] kube::Error),

    #[error("Secret '{name}' not found in namespace '{namespace}'")]
    #[condition(
        reason = "SecretNotFound",
        retryable = true,
        requeue_secs = 30
    )]
    SecretNotFound { name: String, namespace: String },
}

#[derive(Error, Debug, StatusCondition)]
#[condition(default_type = "Ready")]
pub enum RecordReconcileError {
    #[error("Parent zone '{zone}' not found")]
    #[condition(
        type = "DependenciesReady",
        reason = "ZoneNotFound",
        retryable = true,
        requeue_secs = 10
    )]
    ZoneNotFound { zone: String },

    #[error("Parent zone '{zone}' is not ready")]
    #[condition(
        type = "DependenciesReady",
        reason = "ZoneNotReady",
        retryable = true,
        requeue_secs = 15
    )]
    ZoneNotReady { zone: String },

    #[error("Invalid record data for type {record_type}: {details}")]
    #[condition(
        type = "Validated",
        reason = "InvalidRdata",
        retryable = false
    )]
    InvalidRdata { record_type: String, details: String },

    #[error("RFC 2136 update failed: {0}")]
    #[condition(
        type = "Synchronized",
        reason = "UpdateFailed",
        severity = "error",
        retryable = true,
        requeue_secs = 15
    )]
    Rfc2136Update(String),

    #[error("Kubernetes API error: {0}")]
    #[condition(reason = "KubernetesApiError", retryable = true)]
    KubeApi(#[from] kube::Error),
}
```

---

## Part 4: Implementation Phases

### Phase 1: Create kube-condition Crates (Week 1)

**Deliverables**:
1. `kube-condition` runtime crate with traits and helpers
2. `kube-condition-derive` proc macro crate
3. Unit tests for macro expansion
4. Documentation with examples

**Tasks**:
```bash
# Create workspace structure
mkdir -p kube-condition/kube-condition-derive/src
mkdir -p kube-condition/kube-condition/src

# Files to create:
# - kube-condition/Cargo.toml (workspace)
# - kube-condition/kube-condition/Cargo.toml
# - kube-condition/kube-condition/src/lib.rs
# - kube-condition/kube-condition-derive/Cargo.toml  
# - kube-condition/kube-condition-derive/src/lib.rs
```

**Cargo.toml (workspace)**:
```toml
[workspace]
resolver = "2"
members = ["kube-condition", "kube-condition-derive"]

[workspace.package]
edition = "2021"
license = "MIT OR Apache-2.0"
repository = "https://github.com/firestoned/kube-condition"
```

### Phase 2: Define Error Types in Bindy (Week 2)

**Deliverables**:
1. `bindy/src/error.rs` with full error enums
2. Condition attributes on all variants
3. Update existing reconcilers to use new error types

**Current bindy error handling** (likely):
```rust
// Before: Ad-hoc error handling
async fn reconcile_zone(zone: Arc<DnsZone>, ctx: Arc<Context>) -> Result<Action, Error> {
    // ... reconciliation logic
    if let Err(e) = rndc_client.addzone(&zone_name).await {
        // Manual status update
        let condition = Condition {
            type_: "Ready".to_string(),
            status: "False".to_string(),
            reason: "RndcFailed".to_string(),
            message: e.to_string(),
            // ... more boilerplate
        };
        // ... update status
    }
}
```

**After kube-condition**:
```rust
// After: Declarative error-to-condition mapping
async fn reconcile_zone(zone: Arc<DnsZone>, ctx: Arc<Context>) -> Result<Action, ZoneReconcileError> {
    let zone_name = zone.spec.zone_name.clone();
    
    ctx.rndc_client
        .addzone(&zone_name)
        .await
        .map_err(|e| ZoneReconcileError::RndcCommand {
            command: "addzone".into(),
            message: e.to_string(),
        })?;
    
    Ok(Action::requeue(Duration::from_secs(300)))
}

// The error handler automatically updates status:
fn error_policy<K: Resource>(
    resource: Arc<K>,
    error: &impl StatusCondition,
    ctx: Arc<Context>,
) -> Action {
    // Update status with condition from error
    let condition = Condition::from_error(error, resource.meta().generation);
    // ... patch status
    
    if error.is_retryable() {
        Action::requeue(error.requeue_duration().unwrap_or(DEFAULT_REQUEUE))
    } else {
        Action::await_change()
    }
}
```

### Phase 3: Update CRD Status Structs (Week 2-3)

**DNSZoneStatus**:
```rust
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DnsZoneStatus {
    /// Standard Kubernetes conditions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    
    /// Current zone serial number from BIND9
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<u32>,
    
    /// Last time zone was synchronized with BIND9
    #[serde(default, skip_serializing_if = "Option::is_none")]  
    pub last_sync_time: Option<String>,
    
    /// Primary BIND9 server serving this zone
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_server: Option<String>,
    
    /// Status of each secondary server
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub secondary_status: HashMap<String, SecondaryStatus>,
    
    /// Generation observed by controller
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryStatus {
    pub in_sync: bool,
    pub last_transfer_time: Option<String>,
    pub serial: Option<u32>,
    pub transfer_error: Option<String>,
}
```

**DNSRecordStatus**:
```rust
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecordStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    
    /// Fully qualified domain name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fqdn: Option<String>,
    
    /// Last time record was synchronized
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_time: Option<String>,
    
    /// Hash of current record data for drift detection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_hash: Option<String>,
    
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}
```

### Phase 4: Implement Condition Manager (Week 3)

Create a helper that manages condition transitions correctly:

```rust
// bindy/src/conditions.rs

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use chrono::Utc;

pub struct ConditionManager {
    conditions: Vec<Condition>,
}

impl ConditionManager {
    pub fn new(existing: Vec<Condition>) -> Self {
        Self { conditions: existing }
    }
    
    /// Set a condition, only updating lastTransitionTime if status changed
    pub fn set(&mut self, mut new_condition: Condition) {
        if let Some(existing) = self.conditions.iter().find(|c| c.type_ == new_condition.type_) {
            if existing.status == new_condition.status {
                // Status unchanged - preserve transition time
                new_condition.last_transition_time = existing.last_transition_time.clone();
            }
        }
        
        // Remove old condition of same type
        self.conditions.retain(|c| c.type_ != new_condition.type_);
        self.conditions.push(new_condition);
    }
    
    /// Set Ready=True with success reason
    pub fn set_ready(&mut self, generation: Option<i64>) {
        self.set(Condition {
            type_: "Ready".into(),
            status: "True".into(),
            reason: "ReconcileSucceeded".into(),
            message: "Resource reconciled successfully".into(),
            last_transition_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Utc::now())),
            observed_generation: generation,
        });
    }
    
    /// Set condition from error
    pub fn set_from_error<E: StatusCondition>(&mut self, error: &E, generation: Option<i64>) {
        self.set(Condition::from_error(error, generation));
    }
    
    /// Get all conditions
    pub fn into_conditions(self) -> Vec<Condition> {
        self.conditions
    }
    
    /// Check if Ready=True
    pub fn is_ready(&self) -> bool {
        self.conditions
            .iter()
            .any(|c| c.type_ == "Ready" && c.status == "True")
    }
}
```

### Phase 5: Update Reconcilers (Week 4)

**Zone Reconciler Pattern**:
```rust
// bindy/src/controller/zone.rs

use crate::error::ZoneReconcileError;
use crate::conditions::ConditionManager;
use kube_condition::StatusCondition;

pub async fn reconcile_zone(
    zone: Arc<DnsZone>,
    ctx: Arc<Context>,
) -> Result<Action, ZoneReconcileError> {
    let name = zone.name_any();
    let namespace = zone.namespace().unwrap_or_default();
    let generation = zone.metadata.generation;
    
    info!(%name, %namespace, "Reconciling DNSZone");
    
    // Initialize condition manager with existing conditions
    let mut conditions = ConditionManager::new(
        zone.status.as_ref().map(|s| s.conditions.clone()).unwrap_or_default()
    );
    
    // Set Progressing while we work
    conditions.set(Condition {
        type_: "Progressing".into(),
        status: "True".into(),
        reason: "Reconciling".into(),
        message: format!("Reconciling zone {}", zone.spec.zone_name),
        last_transition_time: Some(Time(Utc::now())),
        observed_generation: generation,
    });
    
    // Early status update to show we're working
    patch_status(&ctx.client, &zone, conditions.clone().into_conditions()).await?;
    
    // Validate zone configuration
    validate_zone_config(&zone.spec).map_err(|e| {
        conditions.set_from_error(&e, generation);
        e
    })?;
    
    conditions.set(Condition {
        type_: "Validated".into(),
        status: "True".into(),
        reason: "SyntaxValid".into(),
        message: "Zone configuration is valid".into(),
        last_transition_time: Some(Time(Utc::now())),
        observed_generation: generation,
    });
    
    // Ensure zone exists on BIND9
    let zone_exists = ctx.bindcar_client
        .zone_exists(&zone.spec.zone_name)
        .await
        .map_err(|e| ZoneReconcileError::RndcConnection {
            address: ctx.bindcar_url.clone(),
            source: e,
        })?;
    
    if !zone_exists {
        ctx.bindcar_client
            .addzone(&zone.spec.zone_name, &zone.spec)
            .await
            .map_err(|e| ZoneReconcileError::RndcCommand {
                command: "addzone".into(),
                message: e.to_string(),
            })?;
    }
    
    // Mark synchronized
    conditions.set(Condition {
        type_: "Synchronized".into(),
        status: "True".into(),
        reason: "ZoneInSync".into(),
        message: format!("Zone {} synchronized with BIND9", zone.spec.zone_name),
        last_transition_time: Some(Time(Utc::now())),
        observed_generation: generation,
    });
    
    // Check secondary status if applicable
    if !zone.spec.secondaries.is_empty() {
        check_secondary_sync(&zone, &ctx, &mut conditions).await?;
    }
    
    // All done - mark ready
    conditions.set(Condition {
        type_: "Progressing".into(),
        status: "False".into(),
        reason: "ReconcileComplete".into(),
        message: "Reconciliation completed".into(),
        last_transition_time: Some(Time(Utc::now())),
        observed_generation: generation,
    });
    
    conditions.set_ready(generation);
    
    // Final status update
    patch_status(&ctx.client, &zone, conditions.into_conditions()).await?;
    
    Ok(Action::requeue(Duration::from_secs(300)))
}

/// Error policy that updates status conditions
pub fn zone_error_policy(
    zone: Arc<DnsZone>,
    error: &ZoneReconcileError,
    ctx: Arc<Context>,
) -> Action {
    let generation = zone.metadata.generation;
    
    // Log based on severity
    match error.severity() {
        Severity::Error => error!(%error, "Zone reconciliation failed"),
        Severity::Warning => warn!(%error, "Zone reconciliation warning"),
        Severity::Info => info!(%error, "Zone reconciliation info"),
    }
    
    // Update status with error condition
    let rt = tokio::runtime::Handle::current();
    rt.spawn(async move {
        let mut conditions = ConditionManager::new(
            zone.status.as_ref().map(|s| s.conditions.clone()).unwrap_or_default()
        );
        
        conditions.set_from_error(error, generation);
        
        // Also set Ready=False if error affects readiness
        if error.to_condition_info().condition_type == "Ready" 
            || error.to_condition_info().status == "False" 
        {
            conditions.set(Condition {
                type_: "Ready".into(),
                status: "False".into(),
                reason: error.to_condition_info().reason,
                message: error.to_string(),
                last_transition_time: Some(Time(Utc::now())),
                observed_generation: generation,
            });
        }
        
        if let Err(e) = patch_status(&ctx.client, &zone, conditions.into_conditions()).await {
            error!(%e, "Failed to update zone status");
        }
    });
    
    // Determine requeue behavior
    if error.is_retryable() {
        Action::requeue(error.requeue_duration().unwrap_or(Duration::from_secs(30)))
    } else {
        Action::await_change()
    }
}
```

### Phase 6: Add kubectl Integration (Week 4)

Update CRD printer columns for better `kubectl get` output:

```rust
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "dns.bindy.io",
    version = "v1alpha1",
    kind = "DnsZone",
    namespaced,
    status = "DnsZoneStatus",
    printcolumn = r#"{"name":"Zone","type":"string","jsonPath":".spec.zoneName"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type==\"Ready\")].status"}"#,
    printcolumn = r#"{"name":"Synced","type":"string","jsonPath":".status.conditions[?(@.type==\"Synchronized\")].status"}"#,
    printcolumn = r#"{"name":"Serial","type":"integer","jsonPath":".status.serial"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#,
)]
pub struct DnsZoneSpec {
    // ...
}
```

**Result**:
```bash
$ kubectl get dnszones
NAME          ZONE              READY   SYNCED   SERIAL      AGE
prod-zone     example.com       True    True     2024121901  5d
staging       staging.local     True    True     2024121801  3d
broken-zone   bad.example.com   False   False    -           1h
```

---

## Part 5: Testing Strategy

### Unit Tests for kube-condition

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[derive(Error, Debug, StatusCondition)]
    #[condition(default_type = "Ready")]
    enum TestError {
        #[error("Connection failed")]
        #[condition(reason = "ConnectionFailed", retryable = true, requeue_secs = 30)]
        Connection,
        
        #[error("Validation failed: {0}")]
        #[condition(type = "Validated", reason = "ValidationFailed", retryable = false)]
        Validation(String),
    }
    
    #[test]
    fn test_condition_generation() {
        let err = TestError::Connection;
        let info = err.to_condition_info();
        
        assert_eq!(info.condition_type, "Ready");
        assert_eq!(info.status, "False");
        assert_eq!(info.reason, "ConnectionFailed");
        assert!(err.is_retryable());
        assert_eq!(err.requeue_duration(), Some(Duration::from_secs(30)));
    }
    
    #[test]
    fn test_custom_condition_type() {
        let err = TestError::Validation("invalid syntax".into());
        let info = err.to_condition_info();
        
        assert_eq!(info.condition_type, "Validated");
        assert!(!err.is_retryable());
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_zone_condition_progression() {
    let client = kube::Client::try_default().await.unwrap();
    let zones: Api<DnsZone> = Api::namespaced(client, "test");
    
    // Create zone
    let zone = DnsZone::new("test-zone", DnsZoneSpec {
        zone_name: "test.example.com".into(),
        // ...
    });
    zones.create(&PostParams::default(), &zone).await.unwrap();
    
    // Wait for Progressing=True
    await_condition(&zones, "test-zone", |z| {
        z.status.as_ref()
            .and_then(|s| s.conditions.iter().find(|c| c.type_ == "Progressing"))
            .map(|c| c.status == "True")
            .unwrap_or(false)
    }).await;
    
    // Wait for Ready=True
    await_condition(&zones, "test-zone", |z| {
        z.status.as_ref()
            .and_then(|s| s.conditions.iter().find(|c| c.type_ == "Ready"))
            .map(|c| c.status == "True")
            .unwrap_or(false)
    }).await;
}
```

---

## Part 6: Documentation Updates

### User-Facing Documentation

Add to bindy docs:

```markdown
## Status Conditions

bindy uses Kubernetes-standard status conditions to communicate resource state.

### DNSZone Conditions

| Condition | Meaning |
|-----------|---------|
| `Ready` | Zone is fully operational and serving DNS |
| `Progressing` | Reconciliation is in progress |
| `Synchronized` | Zone configuration matches BIND9 |
| `Validated` | Zone syntax is valid |
| `SecondariesInSync` | All secondary servers have current data |
| `DnssecReady` | DNSSEC signing is operational |

### Checking Status

```bash
# Quick status check
kubectl get dnszone my-zone -o jsonpath='{.status.conditions}'

# Detailed condition view
kubectl describe dnszone my-zone

# Watch for Ready state
kubectl wait --for=condition=Ready dnszone/my-zone --timeout=60s
```

### Troubleshooting by Condition

**Ready=False, Reason=RndcConnectionFailed**
- Check bindcar sidecar is running: `kubectl get pods -l app=bind9`
- Verify network policy allows bindy → bindcar communication
- Check bindcar logs: `kubectl logs <pod> -c bindcar`

**Synchronized=False, Reason=DriftDetected**
- Zone was modified outside of bindy
- Run `kubectl annotate dnszone <name> bindy.io/force-sync=true` to resync
```

---

## Part 7: Metrics (Optional Enhancement)

Add Prometheus metrics for conditions:

```rust
use prometheus::{IntCounterVec, IntGaugeVec, opts, register_int_counter_vec, register_int_gauge_vec};

lazy_static! {
    pub static ref RECONCILE_ERRORS: IntCounterVec = register_int_counter_vec!(
        opts!("bindy_reconcile_errors_total", "Total reconciliation errors"),
        &["resource_type", "reason", "severity"]
    ).unwrap();
    
    pub static ref CONDITION_STATE: IntGaugeVec = register_int_gauge_vec!(
        opts!("bindy_condition_state", "Current condition state (1=True, 0=False, -1=Unknown)"),
        &["resource_type", "namespace", "name", "condition_type"]
    ).unwrap();
}
```

---

## Summary: Files to Create/Modify

### New Files (kube-condition crate)
- `kube-condition/Cargo.toml`
- `kube-condition/kube-condition/Cargo.toml`
- `kube-condition/kube-condition/src/lib.rs`
- `kube-condition/kube-condition-derive/Cargo.toml`
- `kube-condition/kube-condition-derive/src/lib.rs`
- `kube-condition/README.md`

### Modified Files (bindy)
- `bindy/Cargo.toml` - add kube-condition dependency
- `bindy/src/error.rs` - new error types with derive macro
- `bindy/src/conditions.rs` - new ConditionManager helper
- `bindy/src/crd/zone.rs` - update DnsZoneStatus
- `bindy/src/crd/record.rs` - update DnsRecordStatus
- `bindy/src/controller/zone.rs` - use new error types
- `bindy/src/controller/record.rs` - use new error types
- `docs/status-conditions.md` - user documentation

---

## Claude Code Prompt

When starting this in Claude Code, use:

```
I'm implementing kube-condition, a Rust derive macro library for mapping errors to Kubernetes 
status conditions, and integrating it into my bindy BIND9 operator.

Context:
- bindy repo: https://github.com/firestoned/bindy
- kube-condition will be a new crate (potentially separate repo or workspace member)
- Using kube-rs, tokio, thiserror

Start with Phase 1: Create the kube-condition and kube-condition-derive crates with the 
basic trait definitions and proc macro scaffolding. Reference the roadmap in this file 
for the full design.
```
