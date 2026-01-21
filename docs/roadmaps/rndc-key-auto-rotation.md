# RNDC Key Auto-Rotation Implementation Roadmap

**Status:** Proposed
**Date:** 2026-01-14
**Author:** Erick Bourgeois
**Impact:** Major API changes, new operator, breaking changes to CRD schemas

---

## Executive Summary

This roadmap outlines the implementation of automatic RNDC key rotation for the Bindy operator. Currently, RNDC keys are generated once at instance creation and never rotated. This proposal adds support for:

1. **Enhanced RNDC key configuration** with lifecycle management fields
2. **Automatic key rotation** based on configurable time intervals
3. **Manual rotation triggers** via annotation updates
4. **Backward compatibility** with existing deployments
5. **A new dedicated operator** for managing key rotation lifecycle
6. **Comprehensive documentation** for compliance and regulatory requirements

### Regulatory Compliance Benefits

This feature is **critical for regulated environments** (banking, healthcare, government) where credential rotation is mandatory:

- **NIST SP 800-57 Compliance**: Aligns with cryptographic key management lifecycle requirements
- **NIST SP 800-53 AC-2 Compliance**: Supports account management and credential lifecycle controls
- **FIPS 140-2/140-3 Alignment**: Enables proper key lifecycle management for cryptographic operations
- **PCI DSS 3.2 Requirement 8.2.4**: Automated password/key changes at defined intervals
- **SOC 2 Trust Service Criteria**: Demonstrates effective access control and key management
- **HIPAA Security Rule**: Supports technical safeguards for secure communication

**Key Compliance Benefit**: Automated rotation reduces human error, provides audit trails, and ensures consistent application of security policies across the infrastructure.

---

## Current State Analysis

### Existing RNDC Key Management

**Current CRD Structure:**

```rust
// In src/crd.rs
pub struct GlobalConfig {
    pub rndc_secret_ref: Option<RndcSecretRef>,
    // ...
}

pub struct PrimaryConfig {
    pub rndc_secret_ref: Option<RndcSecretRef>,
    // ...
}

pub struct SecondaryConfig {
    pub rndc_secret_ref: Option<RndcSecretRef>,
    // ...
}

pub struct RndcSecretRef {
    pub name: String,
    pub algorithm: RndcAlgorithm,
    pub key_name_key: String,
    pub secret_key: String,
}
```

**Current Behavior:**

- RNDC keys are generated once during `Bind9Instance` creation
- Keys are stored in Kubernetes Secrets with operator-managed or user-managed content
- No rotation mechanism exists
- No tracking of key creation time or age
- Keys persist indefinitely unless manually rotated

**Key Generation:** `src/bind9/rndc.rs`
```rust
pub fn generate_rndc_key() -> RndcKeyData {
    let mut rng = rand::thread_rng();
    let mut key_bytes = [0u8; 32]; // 256 bits for HMAC-SHA256
    rng.fill(&mut key_bytes);
    RndcKeyData {
        name: String::new(),
        algorithm: RndcAlgorithm::HmacSha256,
        secret: BASE64.encode(key_bytes),
    }
}
```

**Key Storage:** Secrets have 4 fields:
- `key-name`: Operator metadata
- `algorithm`: HMAC algorithm
- `secret`: Base64-encoded key material
- `rndc.key`: BIND9 key file content (used by BIND9)

### Precedence Order (Current)

1. Instance level (`spec.rndcSecretRef`)
2. Role level (`spec.primary.rndcSecretRef` or `spec.secondary.rndcSecretRef`)
3. Global level (`spec.global.rndcSecretRef`)
4. Auto-generated (default)

### Limitations

- **No key lifecycle management**: Keys never expire or rotate
- **Security compliance gaps**: Cannot meet policies requiring periodic key rotation
- **Manual rotation burden**: Requires manual Secret updates and pod restarts
- **No rotation tracking**: No visibility into key age or rotation history
- **Risk of key compromise**: Long-lived keys increase security risk

---

## Proposed API Changes

### New CRD Structure: `RndcKeyConfig`

Add a new struct to replace `Option<RndcSecretRef>` at global, primary, and secondary levels:

```rust
/// RNDC key configuration with lifecycle management and auto-rotation support.
///
/// Supports three modes:
/// 1. **Auto-generated with rotation** - Operator creates and rotates keys automatically
/// 2. **User-managed Secret reference** - Reference existing Secret (no rotation)
/// 3. **Inline Secret with rotation** - Embed Secret spec with auto-rotation
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RndcKeyConfig {
    /// Enable automatic key rotation (default: false).
    ///
    /// When enabled, the operator will automatically rotate keys after the
    /// `rotate_after` duration has elapsed since the key creation timestamp
    /// (tracked in the Secret's `bindy.firestoned.io/created-at` annotation).
    ///
    /// **Important**: Rotation only applies to operator-managed Secrets. If you
    /// specify `secret_ref`, that Secret will not be rotated automatically.
    #[serde(default)]
    pub auto_rotate: bool,

    /// Duration after which to rotate the key (e.g., "720h", "30d").
    ///
    /// Supports Go duration format: `100h12m12s`, `720h`, `30d`, etc.
    /// Minimum: 1 hour. Recommended: 30-90 days.
    ///
    /// When `auto_rotate: true`, the operator will rotate the key when:
    /// `current_time - created_at >= rotate_after`
    ///
    /// **Manual rotation trigger**: Set the `bindy.firestoned.io/created-at`
    /// annotation to a past timestamp to force immediate rotation.
    ///
    /// Default: 2160h (90 days)
    #[serde(default = "default_rotate_after")]
    pub rotate_after: String,

    /// Reference to an existing Kubernetes Secret (user-managed, not rotated).
    ///
    /// When specified, the operator uses this existing Secret and does NOT
    /// auto-generate or rotate it. This takes precedence over `secret`.
    ///
    /// **Rotation note**: User-managed Secrets are NOT automatically rotated
    /// even if `auto_rotate: true`. You must rotate these manually.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<RndcSecretRef>,

    /// Inline Secret specification (operator-managed, can be rotated).
    ///
    /// Embeds a full Kubernetes Secret API object. The operator will create
    /// and manage this Secret, and rotate it if `auto_rotate: true`.
    ///
    /// If both `secret_ref` and `secret` are specified, `secret_ref` takes
    /// precedence and `secret` is ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<SecretSpec>,

    /// HMAC algorithm for the RNDC key (default: hmac-sha256).
    ///
    /// Only used when auto-generating keys (when neither `secret_ref` nor
    /// `secret` are specified).
    #[serde(default)]
    pub algorithm: RndcAlgorithm,
}

fn default_rotate_after() -> String {
    "2160h".to_string() // 90 days
}

/// Full Kubernetes Secret specification for inline RNDC keys.
///
/// This is a subset of the v1 Secret API, containing fields relevant for
/// RNDC key management. The operator will create a Secret from this spec.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretSpec {
    /// Secret metadata (name, labels, annotations).
    ///
    /// **Required**: You must specify `metadata.name` for the Secret name.
    pub metadata: SecretMetadata,

    /// Secret type (default: Opaque).
    #[serde(default = "default_secret_type")]
    pub type_: String,

    /// String data (keys and values as strings).
    ///
    /// For RNDC keys, you should provide:
    /// - `rndc.key`: Full BIND9 key file content (required by BIND9)
    ///
    /// Optional metadata (for operator-generated Secrets):
    /// - `key-name`: Name of the TSIG key
    /// - `algorithm`: HMAC algorithm
    /// - `secret`: Base64-encoded key material
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_data: Option<BTreeMap<String, String>>,

    /// Binary data (keys and values as base64 strings).
    ///
    /// Alternative to `string_data` if you want to provide binary values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<BTreeMap<String, String>>,
}

fn default_secret_type() -> String {
    "Opaque".to_string()
}

/// Minimal Secret metadata for inline Secret specs.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretMetadata {
    /// Secret name (required).
    pub name: String,

    /// Labels to apply to the Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,

    /// Annotations to apply to the Secret.
    ///
    /// **Note**: The operator will add `bindy.firestoned.io/created-at` for
    /// rotation tracking. Do not set this manually.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<BTreeMap<String, String>>,
}
```

### Updated CRD Fields

**Replace existing `Option<RndcSecretRef>` with `Option<RndcKeyConfig>`:**

```rust
pub struct GlobalConfig {
    // OLD: pub rndc_secret_ref: Option<RndcSecretRef>,

    /// RNDC key configuration for all instances (global default).
    ///
    /// This replaces the deprecated `rndc_secret_ref` field. If both are
    /// specified, `rndc_keys` takes precedence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_keys: Option<RndcKeyConfig>,

    /// Deprecated: Use `rndc_keys` instead.
    ///
    /// This field is deprecated and will be removed in v1. Use `rndc_keys`
    /// for new deployments.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[deprecated(since = "0.6.0", note = "Use rndc_keys instead")]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    // ...
}

pub struct PrimaryConfig {
    // OLD: pub rndc_secret_ref: Option<RndcSecretRef>,

    /// RNDC key configuration for all primary instances.
    ///
    /// Overrides global `rndc_keys` configuration for primary instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_keys: Option<RndcKeyConfig>,

    /// Deprecated: Use `rndc_keys` instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[deprecated(since = "0.6.0", note = "Use rndc_keys instead")]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    // ...
}

pub struct SecondaryConfig {
    // OLD: pub rndc_secret_ref: Option<RndcSecretRef>,

    /// RNDC key configuration for all secondary instances.
    ///
    /// Overrides global `rndc_keys` configuration for secondary instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rndc_keys: Option<RndcKeyConfig>,

    /// Deprecated: Use `rndc_keys` instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[deprecated(since = "0.6.0", note = "Use rndc_keys instead")]
    pub rndc_secret_ref: Option<RndcSecretRef>,

    // ...
}
```

### Updated Precedence Order

1. Instance level (`spec.rndcSecretRef` or `spec.rndcKeys`)
2. Role level (`spec.primary.rndcKeys` or `spec.secondary.rndcKeys`)
3. Global level (`spec.global.rndcKeys`)
4. Auto-generated with rotation (default)

**Backward compatibility**: Existing `rndc_secret_ref` fields will continue to work but are deprecated.

---

## Example YAML Usage

### Example 1: Auto-Rotation with Defaults

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: production-dns
spec:
  global:
    rndcKeys:
      autoRotate: true
      rotateAfter: 720h  # 30 days
      algorithm: hmac-sha256
  primary:
    replicas: 1
  secondary:
    replicas: 2
```

**Behavior:**
- Operator auto-generates RNDC key on first deployment
- Key is rotated every 30 days automatically
- All instances (primary and secondary) share the same key lifecycle

### Example 2: User-Managed Secret (No Rotation)

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dev-dns
  namespace: dev-team
spec:
  global:
    rndcKeys:
      secretRef:
        name: my-custom-rndc-key
        algorithm: hmac-sha256
  primary:
    replicas: 1
```

**Behavior:**
- Uses existing Secret `my-custom-rndc-key` in `dev-team` namespace
- No automatic rotation (user manages this Secret manually)
- `autoRotate` field ignored when `secretRef` is specified

### Example 3: Inline Secret with Rotation

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: platform-dns
spec:
  global:
    rndcKeys:
      autoRotate: true
      rotateAfter: 2160h  # 90 days
      secret:
        metadata:
          name: platform-rndc-key
          labels:
            app: bindy
            tier: infrastructure
        stringData:
          rndc.key: |
            key "bindy-operator" {
                algorithm hmac-sha256;
                secret "dGhpc2lzYXNlY3JldGtleQ==";
            };
  primary:
    replicas: 2
```

**Behavior:**
- Operator creates Secret `platform-rndc-key` with provided content
- Key is rotated every 90 days
- Old Secret is replaced with new auto-generated key

### Example 4: Different Keys for Primary and Secondary

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: multi-key-cluster
  namespace: dns-system
spec:
  primary:
    replicas: 1
    rndcKeys:
      autoRotate: true
      rotateAfter: 1440h  # 60 days
      algorithm: hmac-sha512
  secondary:
    replicas: 2
    rndcKeys:
      autoRotate: true
      rotateAfter: 720h  # 30 days
      algorithm: hmac-sha256
```

**Behavior:**
- Primary instances use HMAC-SHA512 keys rotated every 60 days
- Secondary instances use HMAC-SHA256 keys rotated every 30 days
- Keys are rotated independently

### Example 5: Manual Rotation Trigger

```bash
# Force immediate rotation by setting creation timestamp to the past
kubectl annotate secret platform-rndc-key \
  bindy.firestoned.io/created-at="2020-01-01T00:00:00Z" \
  --overwrite
```

**Behavior:**
- Operator detects creation timestamp is older than `rotateAfter`
- Triggers immediate key rotation
- Updates Secret with new key material
- Restarts BIND9 pods to pick up new key

---

## Implementation Plan

### Phase 1: API Changes (Breaking Change)

**Goal:** Update CRD schemas and regenerate YAML files.

**Tasks:**

1. **Update `src/crd.rs`:**
   - Add `RndcKeyConfig`, `SecretSpec`, `SecretMetadata` structs
   - Add `rndc_keys: Option<RndcKeyConfig>` to `GlobalConfig`, `PrimaryConfig`, `SecondaryConfig`
   - Mark existing `rndc_secret_ref` fields as deprecated
   - Add rustdoc comments for all new fields

2. **Regenerate CRD YAMLs:**
   ```bash
   cargo run --bin crdgen
   ```

3. **Update examples:**
   - Update `/examples/bind9cluster.yaml`
   - Update `/examples/clusterbind9provider.yaml`
   - Add new example: `/examples/rndc-auto-rotation.yaml`
   - Validate all examples: `./scripts/validate-examples.sh`

4. **Update documentation:**
   - Update `/docs/src/user-guide/rndc-keys.md` (create if doesn't exist)
   - Update `/docs/src/reference/api.md` (via `crddoc`)
   - Update quickstart guide with new field examples
   - Add troubleshooting section for rotation issues

5. **Testing:**
   - Run `cargo fmt`, `cargo clippy`, `cargo test`
   - Manually validate CRD schemas: `kubectl apply --dry-run=client -f deploy/crds/`

**Success Criteria:**
- ✅ CRD YAMLs regenerated with new schema
- ✅ All examples validate successfully
- ✅ Documentation updated and builds: `make docs`
- ✅ No clippy warnings or test failures

---

### Phase 2: Secret Creation Logic (TDD)

**Goal:** Implement Secret creation and update logic with rotation support.

**TDD Workflow:**

1. **Write failing tests first** (`src/bind9/rndc_tests.rs`):
   ```rust
   #[tokio::test]
   async fn test_create_secret_with_rotation_annotation() {
       // Test that created Secrets include creation timestamp annotation
   }

   #[tokio::test]
   async fn test_parse_rndc_key_config_with_auto_rotate() {
       // Test parsing new RndcKeyConfig from spec
   }

   #[tokio::test]
   async fn test_secret_needs_rotation() {
       // Test logic for determining if key needs rotation
   }
   ```

2. **Implement minimum code to pass tests** (`src/bind9/rndc.rs`):
   - Add `create_secret_from_key_config()` function
   - Add `should_rotate_key()` function
   - Add `parse_rotate_after_duration()` function

3. **Refactor for clarity:**
   - Extract constants (e.g., `CREATED_AT_ANNOTATION`, `DEFAULT_ROTATE_AFTER`)
   - Add rustdoc comments
   - Run `cargo clippy` and fix warnings

**Implementation Details:**

```rust
// In src/bind9/rndc.rs

/// Annotation key for tracking Secret creation timestamp.
const CREATED_AT_ANNOTATION: &str = "bindy.firestoned.io/created-at";

/// Default rotation interval (90 days).
const DEFAULT_ROTATE_AFTER: &str = "2160h";

/// Create a Kubernetes Secret from RndcKeyConfig.
///
/// If `config.auto_rotate` is true, adds the creation timestamp annotation.
/// If `config.secret_ref` is specified, returns None (user-managed Secret).
/// If `config.secret` is specified, creates Secret from inline spec.
/// Otherwise, auto-generates a new Secret.
///
/// # Errors
///
/// Returns an error if:
/// - Secret spec is invalid
/// - Duration parsing fails
pub fn create_secret_from_key_config(
    namespace: &str,
    instance_name: &str,
    config: &RndcKeyConfig,
) -> Result<Option<Secret>> {
    // If secretRef is specified, user manages the Secret
    if config.secret_ref.is_some() {
        return Ok(None);
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut annotations = BTreeMap::new();

    if config.auto_rotate {
        annotations.insert(CREATED_AT_ANNOTATION.to_string(), now);
    }

    // If inline Secret provided, use it
    if let Some(secret_spec) = &config.secret {
        let mut secret = Secret {
            metadata: ObjectMeta {
                name: Some(secret_spec.metadata.name.clone()),
                namespace: Some(namespace.to_string()),
                labels: secret_spec.metadata.labels.clone(),
                annotations: Some(annotations),
                ..Default::default()
            },
            type_: Some(secret_spec.type_.clone()),
            string_data: secret_spec.string_data.clone(),
            data: secret_spec.data.clone(),
            ..Default::default()
        };

        // Merge user annotations with rotation annotation
        if let Some(user_annotations) = &secret_spec.metadata.annotations {
            if let Some(ref mut secret_annotations) = secret.metadata.annotations {
                secret_annotations.extend(user_annotations.clone());
            }
        }

        return Ok(Some(secret));
    }

    // Auto-generate Secret
    let mut key_data = generate_rndc_key();
    key_data.name = "bindy-operator".to_string();
    key_data.algorithm = config.algorithm.clone();

    let secret_data = create_rndc_secret_data(&key_data);
    let secret_name = format!("{}-rndc", instance_name);

    Ok(Some(Secret {
        metadata: ObjectMeta {
            name: Some(secret_name),
            namespace: Some(namespace.to_string()),
            annotations: Some(annotations),
            ..Default::default()
        },
        type_: Some("Opaque".to_string()),
        string_data: Some(secret_data),
        ..Default::default()
    }))
}

/// Check if an RNDC key Secret needs rotation.
///
/// Returns `true` if:
/// - Secret has the creation timestamp annotation
/// - Current time - created_at >= rotate_after duration
///
/// # Errors
///
/// Returns an error if:
/// - Annotation is present but not a valid RFC3339 timestamp
/// - Duration parsing fails
pub fn should_rotate_key(
    secret: &Secret,
    rotate_after: &str,
) -> Result<bool> {
    let annotations = match &secret.metadata.annotations {
        Some(a) => a,
        None => return Ok(false), // No annotation, no rotation
    };

    let created_at_str = match annotations.get(CREATED_AT_ANNOTATION) {
        Some(s) => s,
        None => return Ok(false), // No creation timestamp, no rotation
    };

    let created_at = chrono::DateTime::parse_from_rfc3339(created_at_str)
        .with_context(|| format!("Invalid created-at annotation: {}", created_at_str))?;

    let rotate_duration = parse_duration(rotate_after)
        .with_context(|| format!("Invalid rotate_after duration: {}", rotate_after))?;

    let now = chrono::Utc::now();
    let age = now.signed_duration_since(created_at.with_timezone(&chrono::Utc));

    Ok(age >= rotate_duration)
}

/// Parse a duration string (e.g., "720h", "30d", "100h12m12s").
///
/// Supports Go-style duration format with extensions:
/// - h: hours
/// - m: minutes
/// - s: seconds
/// - d: days (extension, converted to hours)
///
/// # Errors
///
/// Returns an error if the duration string is invalid.
fn parse_duration(s: &str) -> Result<chrono::Duration> {
    // Handle days extension (not in Go duration)
    let s = if s.ends_with('d') {
        let days: i64 = s.trim_end_matches('d').parse()
            .with_context(|| format!("Invalid day count: {}", s))?;
        format!("{}h", days * 24)
    } else {
        s.to_string()
    };

    // Use a duration parsing library (e.g., humantime or duration-str)
    // For now, simplified parsing:
    let duration = humantime::parse_duration(&s)
        .with_context(|| format!("Invalid duration: {}", s))?;

    Ok(chrono::Duration::from_std(duration)?)
}
```

**Testing:**
- Unit tests for `create_secret_from_key_config()`
- Unit tests for `should_rotate_key()`
- Unit tests for `parse_duration()`
- Test edge cases: invalid durations, missing annotations, past timestamps

**Dependencies:**
- Add `humantime` crate for duration parsing: `cargo add humantime`
- Add `chrono` for timestamp handling (already in use)

**Success Criteria:**
- ✅ All unit tests pass
- ✅ Secret creation includes rotation annotation when `autoRotate: true`
- ✅ Rotation detection logic correctly identifies expired keys
- ✅ Duration parsing handles all formats (hours, minutes, seconds, days)

---

### Phase 3: Bind9Instance Reconciler Updates

**Goal:** Update `Bind9Instance` reconciler to use new `RndcKeyConfig` fields.

**TDD Workflow:**

1. **Write failing tests** (`src/reconcilers/bind9instance_tests.rs`):
   ```rust
   #[tokio::test]
   async fn test_reconcile_with_auto_rotate_enabled() {
       // Test instance creation with auto-rotation
   }

   #[tokio::test]
   async fn test_reconcile_uses_secret_ref_precedence() {
       // Test precedence: instance > role > global
   }

   #[tokio::test]
   async fn test_reconcile_backward_compatibility() {
       // Test deprecated rndc_secret_ref still works
   }
   ```

2. **Implement reconciler changes** (`src/reconcilers/bind9instance/resources.rs`):
   - Add `resolve_rndc_key_config()` helper function
   - Update `create_or_update_rndc_secret()` to use `RndcKeyConfig`
   - Add backward compatibility layer for deprecated `rndc_secret_ref`

3. **Refactor for clarity:**
   - Extract helper functions
   - Add rustdoc comments
   - Run `cargo clippy` and fix warnings

**Implementation Details:**

```rust
// In src/reconcilers/bind9instance/resources.rs

/// Resolve RNDC key configuration with precedence order.
///
/// Precedence (highest to lowest):
/// 1. Instance level (`spec.rndcKeys` or deprecated `spec.rndcSecretRef`)
/// 2. Role level (primary/secondary `rndcKeys`)
/// 3. Global level (`spec.global.rndcKeys`)
/// 4. Auto-generated default
///
/// # Returns
///
/// Returns the resolved `RndcKeyConfig`, applying backward compatibility
/// for deprecated `rndc_secret_ref` fields.
fn resolve_rndc_key_config(
    instance: &Bind9Instance,
    cluster: Option<&Bind9Cluster>,
    provider: Option<&ClusterBind9Provider>,
) -> RndcKeyConfig {
    // Instance level (highest priority)
    if let Some(ref config) = instance.spec.rndc_keys {
        return config.clone();
    }

    // Backward compatibility: instance rndc_secret_ref
    #[allow(deprecated)]
    if let Some(ref secret_ref) = instance.spec.rndc_secret_ref {
        return RndcKeyConfig {
            auto_rotate: false,
            rotate_after: DEFAULT_ROTATE_AFTER.to_string(),
            secret_ref: Some(secret_ref.clone()),
            secret: None,
            algorithm: secret_ref.algorithm.clone(),
        };
    }

    // Role level (primary or secondary)
    let role_config = match instance.spec.role {
        InstanceRole::Primary => {
            cluster.and_then(|c| c.spec.common.primary.as_ref())
                .and_then(|p| p.rndc_keys.as_ref())
                .or_else(|| provider.and_then(|p| p.spec.common.primary.as_ref())
                    .and_then(|p| p.rndc_keys.as_ref()))
        }
        InstanceRole::Secondary => {
            cluster.and_then(|c| c.spec.common.secondary.as_ref())
                .and_then(|s| s.rndc_keys.as_ref())
                .or_else(|| provider.and_then(|p| p.spec.common.secondary.as_ref())
                    .and_then(|s| s.rndc_keys.as_ref()))
        }
    };

    if let Some(config) = role_config {
        return config.clone();
    }

    // Backward compatibility: role rndc_secret_ref
    let role_secret_ref = match instance.spec.role {
        InstanceRole::Primary => {
            #[allow(deprecated)]
            cluster.and_then(|c| c.spec.common.primary.as_ref())
                .and_then(|p| p.rndc_secret_ref.as_ref())
                .or_else(|| {
                    #[allow(deprecated)]
                    provider.and_then(|p| p.spec.common.primary.as_ref())
                        .and_then(|p| p.rndc_secret_ref.as_ref())
                })
        }
        InstanceRole::Secondary => {
            #[allow(deprecated)]
            cluster.and_then(|c| c.spec.common.secondary.as_ref())
                .and_then(|s| s.rndc_secret_ref.as_ref())
                .or_else(|| {
                    #[allow(deprecated)]
                    provider.and_then(|p| p.spec.common.secondary.as_ref())
                        .and_then(|s| s.rndc_secret_ref.as_ref())
                })
        }
    };

    if let Some(secret_ref) = role_secret_ref {
        return RndcKeyConfig {
            auto_rotate: false,
            rotate_after: DEFAULT_ROTATE_AFTER.to_string(),
            secret_ref: Some(secret_ref.clone()),
            secret: None,
            algorithm: secret_ref.algorithm.clone(),
        };
    }

    // Global level
    let global_config = cluster.map(|c| &c.spec.common.global)
        .or_else(|| provider.map(|p| &p.spec.common.global));

    if let Some(global) = global_config {
        if let Some(ref config) = global.rndc_keys {
            return config.clone();
        }

        // Backward compatibility: global rndc_secret_ref
        #[allow(deprecated)]
        if let Some(ref secret_ref) = global.rndc_secret_ref {
            return RndcKeyConfig {
                auto_rotate: false,
                rotate_after: DEFAULT_ROTATE_AFTER.to_string(),
                secret_ref: Some(secret_ref.clone()),
                secret: None,
                algorithm: secret_ref.algorithm.clone(),
            };
        }
    }

    // Default: auto-generate with rotation enabled
    RndcKeyConfig {
        auto_rotate: true,
        rotate_after: DEFAULT_ROTATE_AFTER.to_string(),
        secret_ref: None,
        secret: None,
        algorithm: RndcAlgorithm::HmacSha256,
    }
}

/// Create or update RNDC Secret for a Bind9Instance.
///
/// Uses the resolved `RndcKeyConfig` to create or update the Secret.
/// If `config.secret_ref` is specified, this function is a no-op (user manages Secret).
///
/// # Errors
///
/// Returns an error if Secret creation or API call fails.
pub async fn create_or_update_rndc_secret(
    client: &Client,
    instance: &Bind9Instance,
    config: &RndcKeyConfig,
) -> Result<()> {
    let namespace = instance.namespace().context("Instance has no namespace")?;
    let instance_name = instance.name_any();

    // If secretRef is specified, user manages the Secret
    if config.secret_ref.is_some() {
        debug!(
            instance = %instance_name,
            "Using user-managed Secret (secretRef specified), skipping Secret creation"
        );
        return Ok(());
    }

    // Create Secret from config
    let secret = create_secret_from_key_config(&namespace, &instance_name, config)?
        .context("Failed to create Secret from RndcKeyConfig")?;

    // Add owner reference to instance
    let secret = add_owner_reference(secret, instance)?;

    // Apply Secret (create or update)
    let secrets_api: Api<Secret> = Api::namespaced(client.clone(), &namespace);
    secrets_api
        .patch(
            secret.metadata.name.as_ref().unwrap(),
            &PatchParams::apply("bindy-operator"),
            &Patch::Apply(secret),
        )
        .await
        .with_context(|| format!("Failed to apply Secret for instance {}", instance_name))?;

    info!(
        instance = %instance_name,
        namespace = %namespace,
        secret = %secret.metadata.name.as_ref().unwrap(),
        "RNDC Secret created/updated"
    );

    Ok(())
}
```

**Testing:**
- Test precedence order with mocked cluster/provider specs
- Test backward compatibility with deprecated fields
- Test that user-managed Secrets are not created/updated
- Test that auto-generated Secrets include rotation annotation

**Success Criteria:**
- ✅ Instance reconciler uses new `RndcKeyConfig` fields
- ✅ Backward compatibility with deprecated fields maintained
- ✅ All unit tests pass
- ✅ No clippy warnings

---

### Phase 4: New RndcKeyRotation Operator (Event-Driven)

**Goal:** Create a new operator to manage RNDC key rotation lifecycle.

**Why a Separate Operator?**
- **Separation of concerns**: Key rotation is orthogonal to instance reconciliation
- **Independent reconciliation loop**: Can watch Secrets directly
- **Scalability**: Doesn't add overhead to instance reconciler
- **Testability**: Easier to test rotation logic in isolation

**TDD Workflow:**

1. **Write failing tests** (`src/reconcilers/rndc_rotation_tests.rs`):
   ```rust
   #[tokio::test]
   async fn test_detect_expired_key() {
       // Test detecting key that needs rotation
   }

   #[tokio::test]
   async fn test_rotate_key() {
       // Test key rotation process
   }

   #[tokio::test]
   async fn test_restart_pods_after_rotation() {
       // Test triggering pod restart after key rotation
   }

   #[tokio::test]
   async fn test_skip_user_managed_secrets() {
       // Test that user-managed Secrets are not rotated
   }
   ```

2. **Implement operator** (`src/reconcilers/rndc_rotation.rs`):
   - Create `reconcile_rndc_secret()` function
   - Watch Secrets with label selector: `app.kubernetes.io/managed-by=bindy`
   - Implement event-driven reconciliation loop
   - Add pod restart logic after rotation

3. **Refactor for clarity:**
   - Extract helper functions
   - Add rustdoc comments
   - Run `cargo clippy` and fix warnings

**Implementation Details:**

```rust
// In src/reconcilers/rndc_rotation.rs

//! RNDC key rotation operator.
//!
//! This operator watches Secrets managed by Bindy and rotates RNDC keys
//! when they exceed the configured `rotate_after` duration.
//!
//! # How It Works
//!
//! 1. **Watch Secrets**: Event-driven watching of Secrets with the label
//!    `app.kubernetes.io/managed-by=bindy`
//! 2. **Check expiration**: On each reconciliation, check if the Secret's
//!    `bindy.firestoned.io/created-at` annotation indicates rotation is needed
//! 3. **Rotate key**: If expired, generate a new RNDC key and update the Secret
//! 4. **Restart pods**: Trigger a rollout restart of the Deployment using this Secret
//!    to pick up the new key
//!
//! # Manual Rotation
//!
//! Users can force rotation by setting the `bindy.firestoned.io/created-at`
//! annotation to a past timestamp.

use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::Secret;
use kube::{
    api::{Api, Patch, PatchParams},
    runtime::{operator::Action, Operator},
    Client, Resource, ResourceExt,
};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Annotation key for tracking Secret creation timestamp.
const CREATED_AT_ANNOTATION: &str = "bindy.firestoned.io/created-at";

/// Annotation key for storing the rotate_after duration.
const ROTATE_AFTER_ANNOTATION: &str = "bindy.firestoned.io/rotate-after";

/// Label selector for Bindy-managed Secrets.
const BINDY_MANAGED_LABEL: &str = "app.kubernetes.io/managed-by=bindy";

/// Reconcile an RNDC Secret for key rotation.
///
/// This function is called by the operator for each Secret event.
/// It checks if the Secret needs rotation and performs the rotation if necessary.
///
/// # Event-Driven Design
///
/// This operator uses the Kubernetes watch API to react to Secret changes,
/// not polling. It only reconciles when:
/// - A new Secret is created
/// - A Secret is modified (e.g., annotation updated)
/// - Periodic requeue (every 1 hour to check for expired keys)
///
/// # Errors
///
/// Returns an error if:
/// - Secret parsing fails
/// - Key rotation fails
/// - Pod restart fails
pub async fn reconcile_rndc_secret(
    secret: Arc<Secret>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcileError> {
    let client = ctx.client.clone();
    let secret_name = secret.name_any();
    let namespace = secret.namespace().context("Secret has no namespace")?;

    debug!(
        secret = %secret_name,
        namespace = %namespace,
        "Reconciling RNDC Secret for rotation"
    );

    // Check if this is a Bindy-managed Secret with rotation enabled
    let annotations = match &secret.metadata.annotations {
        Some(a) => a,
        None => {
            debug!(
                secret = %secret_name,
                "Secret has no annotations, skipping"
            );
            return Ok(Action::await_change()); // No annotations, no rotation
        }
    };

    // Check if rotation is enabled (created-at annotation present)
    if !annotations.contains_key(CREATED_AT_ANNOTATION) {
        debug!(
            secret = %secret_name,
            "Secret does not have created-at annotation, skipping rotation"
        );
        return Ok(Action::await_change());
    }

    // Get rotate_after duration from annotation
    let rotate_after = annotations
        .get(ROTATE_AFTER_ANNOTATION)
        .map(|s| s.as_str())
        .unwrap_or("2160h"); // Default: 90 days

    // Check if key needs rotation
    if !should_rotate_key(&secret, rotate_after)? {
        debug!(
            secret = %secret_name,
            rotate_after = %rotate_after,
            "Secret does not need rotation yet"
        );
        // Requeue after 1 hour to check again
        return Ok(Action::requeue(std::time::Duration::from_secs(3600)));
    }

    info!(
        secret = %secret_name,
        namespace = %namespace,
        "RNDC key expired, rotating..."
    );

    // Rotate the key
    rotate_key(&client, &secret).await?;

    // Restart pods using this Secret
    restart_pods_using_secret(&client, &namespace, &secret_name).await?;

    info!(
        secret = %secret_name,
        namespace = %namespace,
        "RNDC key rotated successfully"
    );

    // Requeue after 1 hour to check for next rotation
    Ok(Action::requeue(std::time::Duration::from_secs(3600)))
}

/// Rotate an RNDC key Secret.
///
/// Generates a new RNDC key and updates the Secret with the new key material.
/// Updates the `bindy.firestoned.io/created-at` annotation to the current time.
///
/// # Errors
///
/// Returns an error if Secret update fails.
async fn rotate_key(client: &Client, secret: &Secret) -> Result<()> {
    let secret_name = secret.name_any();
    let namespace = secret.namespace().context("Secret has no namespace")?;

    // Generate new RNDC key
    let mut key_data = Bind9Manager::generate_rndc_key();
    key_data.name = "bindy-operator".to_string();

    // Get algorithm from existing Secret (if specified)
    let algorithm = secret
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get("bindy.firestoned.io/algorithm"))
        .and_then(|s| s.parse::<RndcAlgorithm>().ok())
        .unwrap_or(RndcAlgorithm::HmacSha256);

    key_data.algorithm = algorithm;

    // Create new Secret data
    let secret_data = Bind9Manager::create_rndc_secret_data(&key_data);

    // Update annotations with new creation timestamp
    let now = chrono::Utc::now().to_rfc3339();
    let mut annotations = secret
        .metadata
        .annotations
        .clone()
        .unwrap_or_default();
    annotations.insert(CREATED_AT_ANNOTATION.to_string(), now);

    // Patch Secret with new data and annotations
    let secrets_api: Api<Secret> = Api::namespaced(client.clone(), &namespace);
    let mut patched_secret = secret.as_ref().clone();
    patched_secret.string_data = Some(secret_data);
    patched_secret.metadata.annotations = Some(annotations);

    secrets_api
        .patch(
            &secret_name,
            &PatchParams::apply("bindy-rndc-rotation-operator"),
            &Patch::Apply(patched_secret),
        )
        .await
        .with_context(|| format!("Failed to rotate Secret {}", secret_name))?;

    Ok(())
}

/// Restart pods using a given Secret.
///
/// Finds all Deployments that reference the Secret and triggers a rollout restart
/// by adding a restart annotation with the current timestamp.
///
/// # Errors
///
/// Returns an error if Deployment discovery or restart fails.
async fn restart_pods_using_secret(
    client: &Client,
    namespace: &str,
    secret_name: &str,
) -> Result<()> {
    use k8s_openapi::api::apps::v1::Deployment;

    let deployments_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployments = deployments_api
        .list(&Default::default())
        .await
        .context("Failed to list Deployments")?;

    for deployment in deployments {
        let deployment_name = deployment.name_any();

        // Check if Deployment references this Secret
        let references_secret = deployment
            .spec
            .as_ref()
            .and_then(|s| s.template.spec.as_ref())
            .and_then(|s| s.volumes.as_ref())
            .map(|volumes| {
                volumes.iter().any(|v| {
                    v.secret.as_ref().map_or(false, |s| {
                        s.secret_name.as_ref() == Some(&secret_name.to_string())
                    })
                })
            })
            .unwrap_or(false);

        if !references_secret {
            continue;
        }

        info!(
            deployment = %deployment_name,
            namespace = %namespace,
            "Restarting Deployment after RNDC key rotation"
        );

        // Trigger rollout restart by adding annotation
        let now = chrono::Utc::now().to_rfc3339();
        let patch = serde_json::json!({
            "spec": {
                "template": {
                    "metadata": {
                        "annotations": {
                            "bindy.firestoned.io/rndc-key-rotated-at": now
                        }
                    }
                }
            }
        });

        deployments_api
            .patch(
                &deployment_name,
                &PatchParams::apply("bindy-rndc-rotation-operator"),
                &Patch::Merge(patch),
            )
            .await
            .with_context(|| {
                format!("Failed to restart Deployment {} after key rotation", deployment_name)
            })?;
    }

    Ok(())
}

/// Error policy for RNDC rotation reconciliation.
///
/// Implements exponential backoff for transient errors.
pub fn error_policy(
    secret: Arc<Secret>,
    error: &ReconcileError,
    _ctx: Arc<Context>,
) -> Action {
    warn!(
        secret = %secret.name_any(),
        error = %error,
        "Reconciliation error, requeueing with backoff"
    );
    Action::requeue(std::time::Duration::from_secs(60))
}

/// Start the RNDC key rotation operator.
///
/// This operator watches Secrets with the label `app.kubernetes.io/managed-by=bindy`
/// and rotates RNDC keys when they expire.
///
/// # Event-Driven Design
///
/// This operator uses the Kubernetes watch API (event-driven), not polling.
/// It reacts to Secret changes and requeues periodically to check for expired keys.
///
/// # Errors
///
/// Returns an error if the operator fails to start.
pub async fn run_rotation_operator(client: Client) -> Result<()> {
    let secrets_api: Api<Secret> = Api::all(client.clone());
    let context = Arc::new(Context { client: client.clone() });

    info!("Starting RNDC key rotation operator");

    Operator::new(secrets_api, Default::default())
        .run(reconcile_rndc_secret, error_policy, context)
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}
```

**Integration in `main.rs`:**

```rust
// In src/main.rs

// Start RNDC key rotation operator in background
let rotation_client = client.clone();
tokio::spawn(async move {
    if let Err(e) = reconcilers::rndc_rotation::run_rotation_operator(rotation_client).await {
        error!("RNDC key rotation operator failed: {}", e);
    }
});
```

**Testing:**
- Mock Secrets with expired `created-at` annotations
- Test key rotation generates new Secret data
- Test pod restart logic finds correct Deployments
- Test that user-managed Secrets (no annotation) are skipped

**Success Criteria:**
- ✅ Operator watches Secrets event-driven (no polling)
- ✅ Expired keys are detected and rotated
- ✅ Pods are restarted after rotation
- ✅ User-managed Secrets are not rotated
- ✅ All unit tests pass

---

### Phase 5: Comprehensive Documentation and Migration Guide

**Goal:** Provide comprehensive documentation for the new feature, migration path, and regulatory compliance guidance.

**CRITICAL:** This phase is **NOT optional**. Comprehensive documentation is mandatory for:
- Regulated environments (banking, healthcare, government)
- Security audit requirements
- Compliance certification processes
- Operational handoff and team knowledge transfer

**Tasks:**

1. **Create comprehensive user guide** (`docs/src/user-guide/rndc-keys.md`):
   - **RNDC Key Management Overview**:
     - What are RNDC keys and why they matter
     - Security best practices for key management
     - Key lifecycle (generation, rotation, revocation)
   - **`RndcKeyConfig` API Reference**:
     - Document all fields with examples
     - Explain precedence order (instance > role > global)
     - Show how to enable/disable auto-rotation
     - Document manual rotation trigger via annotations
   - **Configuration Examples**:
     - Auto-rotation with defaults
     - User-managed Secrets (no rotation)
     - Inline Secret specs with rotation
     - Different keys for primary vs. secondary
     - Multi-environment configurations (dev/staging/prod)
   - **Troubleshooting**:
     - Common rotation issues and solutions
     - How to verify rotation occurred
     - How to debug failed rotations
     - Emergency key rollback procedures

2. **Create regulatory compliance guide** (`docs/src/compliance/rndc-key-rotation.md`):
   - **NIST SP 800-57 Compliance**:
     - Key lifecycle management requirements
     - Recommended rotation intervals by key type
     - Documentation requirements for audits
   - **NIST SP 800-53 Controls**:
     - AC-2 (Account Management): Credential lifecycle
     - IA-5 (Authenticator Management): Key rotation policies
     - SC-12 (Cryptographic Key Management): Key generation and rotation
   - **FIPS 140-2/140-3 Considerations**:
     - Cryptographic module requirements
     - Key zeroization after rotation
     - Audit logging requirements
   - **Industry-Specific Compliance**:
     - PCI DSS 3.2 Requirement 8.2.4 (password changes every 90 days)
     - HIPAA Security Rule (technical safeguards)
     - SOC 2 Trust Service Criteria (access control)
   - **Recommended Rotation Intervals**:
     - **High-security environments**: 30 days (`rotateAfter: 720h`)
     - **Standard environments**: 60-90 days (`rotateAfter: 1440h` - `2160h`)
     - **Development environments**: 180 days (`rotateAfter: 4320h`)
   - **Audit Trail Requirements**:
     - What logs to collect for audits
     - How to demonstrate compliance
     - Sample audit report generation

3. **Create security documentation** (`docs/src/security/rndc-key-security.md`):
   - **Threat Model**:
     - Risks of long-lived keys
     - Attack vectors mitigated by rotation
     - Blast radius of compromised keys
   - **Security Benefits**:
     - Reduced exposure window
     - Automated key hygiene
     - Compliance with zero-trust principles
   - **Key Material Protection**:
     - Secret encryption at rest requirements
     - RBAC configuration for Secret access
     - Preventing key material logging
   - **Incident Response**:
     - Procedure for suspected key compromise
     - Emergency rotation process
     - Forensic logging and analysis

4. **Create migration guide** (`docs/src/migration/v0.6-rndc-keys.md`):
   - **Breaking Changes Summary**:
     - New CRD fields and schema changes
     - Deprecated fields (still supported)
     - Timeline for deprecation removal
   - **Step-by-Step Migration**:
     - Pre-migration checklist
     - CRD update procedure
     - Manifest conversion examples (before/after)
     - Rollback procedure
   - **Migration for Different Deployment Models**:
     - Namespace-scoped clusters (`Bind9Cluster`)
     - Cluster-scoped providers (`ClusterBind9Provider`)
     - Mixed environments
   - **Testing Migration**:
     - Validation steps before production deployment
     - Dry-run procedures
     - Monitoring rotation after migration
   - **Backward Compatibility Guarantee**:
     - How long deprecated fields will be supported
     - Migration timeline recommendations

5. **Create operations runbook** (`docs/src/operations/rndc-key-rotation-runbook.md`):
   - **Day 1 Operations**:
     - Initial deployment with rotation enabled
     - Verification procedures
     - Monitoring setup
   - **Day 2 Operations**:
     - Monitoring rotation health
     - Responding to rotation failures
     - Manual rotation procedures
     - Key rollback procedures
   - **Common Operational Tasks**:
     - Changing rotation intervals
     - Disabling rotation temporarily
     - Re-enabling rotation
     - Forcing immediate rotation
   - **Alerting and Monitoring**:
     - Key metrics to track
     - Alert thresholds
     - Dashboard setup (Grafana examples)
   - **Troubleshooting Playbooks**:
     - Rotation stuck/hanging
     - Pods not restarting after rotation
     - Permission errors during rotation
     - Clock skew issues with timestamps

6. **Update API reference** (`docs/src/reference/api.md`):
   ```bash
   cargo run --bin crddoc > docs/src/reference/api.md
   ```
   - Ensure all new fields are documented
   - Include deprecation notices
   - Link to user guide for detailed examples

7. **Update CHANGELOG.md** with comprehensive change notes:
   ```markdown
   ## [2026-01-XX] - RNDC Key Auto-Rotation with Compliance Support

   **Author:** Erick Bourgeois

   ### Added
   - `RndcKeyConfig` struct for enhanced RNDC key management with lifecycle controls
   - Auto-rotation support via `autoRotate` and `rotateAfter` fields
   - New operator: `RndcKeyRotationOperator` for managing key lifecycle (event-driven)
   - Manual rotation trigger via annotation updates (`bindy.firestoned.io/created-at`)
   - Backward compatibility with deprecated `rndcSecretRef` fields
   - Comprehensive documentation for regulatory compliance (NIST, FIPS, PCI DSS, HIPAA)
   - Security documentation covering threat models and incident response
   - Operations runbook for day-to-day key management
   - Migration guide with step-by-step instructions

   ### Changed
   - `spec.global.rndcSecretRef` → `spec.global.rndcKeys` (deprecated old field)
   - `spec.primary.rndcSecretRef` → `spec.primary.rndcKeys` (deprecated old field)
   - `spec.secondary.rndcSecretRef` → `spec.secondary.rndcKeys` (deprecated old field)

   ### Deprecated
   - `rndc_secret_ref` fields (use `rndc_keys` instead, removal planned for v1.0)

   ### Why
   Long-lived RNDC keys increase security risk and create compliance gaps. Auto-rotation:
   - Reduces exposure window for compromised keys
   - Enables compliance with NIST SP 800-57, PCI DSS 8.2.4, HIPAA Security Rule
   - Automates key hygiene, reducing human error
   - Provides audit trail for security certifications
   - Supports zero-trust security architecture

   ### Impact
   - [x] Breaking change (new CRD schema, backward compatible with deprecated fields)
   - [ ] Requires cluster rollout (optional, migration at your own pace)
   - [x] Config change recommended (enable auto-rotation for compliance)
   - [x] Documentation update (comprehensive compliance and security documentation)

   ### Compliance Benefits
   - **NIST SP 800-57**: Cryptographic key management lifecycle
   - **NIST SP 800-53 AC-2**: Account management and credential lifecycle
   - **FIPS 140-2/140-3**: Key lifecycle management for cryptographic operations
   - **PCI DSS 3.2 Req 8.2.4**: Automated credential changes at defined intervals
   - **SOC 2**: Access control and key management trust service criteria
   - **HIPAA Security Rule**: Technical safeguards for secure communication

   ### Recommended Rotation Intervals
   - **High-security/Production**: 30 days (`rotateAfter: 720h`)
   - **Standard environments**: 60-90 days (`rotateAfter: 1440h` - `2160h`)
   - **Development**: 180 days (`rotateAfter: 4320h`)

   **Note**: These recommendations align with NIST and industry best practices.
   ```

8. **Update quickstart guide** (`docs/src/quickstart.md`):
   - Add prominent section on RNDC key rotation
   - Update examples to use new `rndcKeys` field
   - Show compliance-friendly configuration
   - Include monitoring setup for rotation

9. **Create compliance checklist** (`docs/src/compliance/checklist.md`):
   - Pre-deployment compliance verification
   - Configuration audit checklist
   - Documentation requirements for certification
   - Evidence collection for audits

10. **Build and validate documentation**:
    ```bash
    make docs
    ```
    - Verify all internal links work
    - Ensure all code examples are valid
    - Check that mermaid diagrams render correctly
    - Validate API documentation is complete

**Migration Example:**

```yaml
# BEFORE (deprecated, still works)
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: production-dns
spec:
  global:
    rndcSecretRef:
      name: my-rndc-key
      algorithm: hmac-sha256

# AFTER (recommended)
apiVersion: bindy.firestoned.io/v1beta1
kind: ClusterBind9Provider
metadata:
  name: production-dns
spec:
  global:
    rndcKeys:
      autoRotate: true
      rotateAfter: 720h  # 30 days
      secretRef:
        name: my-rndc-key
        algorithm: hmac-sha256
```

**Success Criteria:**
- ✅ User guide created with examples
- ✅ Migration guide explains breaking changes
- ✅ API reference updated
- ✅ CHANGELOG.md updated with author attribution
- ✅ Quickstart guide updated
- ✅ Documentation builds successfully: `make docs`

---

### Phase 6: Integration Testing

**Goal:** Validate the entire rotation workflow end-to-end.

**Tasks:**

1. **Create integration test** (`tests/rndc_rotation_test.rs`):
   - Deploy `Bind9Cluster` with `autoRotate: true`
   - Verify Secret is created with rotation annotation
   - Manually set annotation to trigger rotation
   - Verify Secret is updated with new key
   - Verify pods are restarted

2. **Manual testing in Kind cluster:**
   ```bash
   # 1. Deploy cluster with auto-rotation
   kubectl apply -f examples/rndc-auto-rotation.yaml

   # 2. Verify Secret has creation timestamp
   kubectl get secret platform-rndc-key -o yaml | grep created-at

   # 3. Force rotation by setting timestamp to past
   kubectl annotate secret platform-rndc-key \
     bindy.firestoned.io/created-at="2020-01-01T00:00:00Z" \
     --overwrite

   # 4. Wait for operator to detect and rotate
   kubectl logs -l app=bindy-operator -f | grep "RNDC key expired"

   # 5. Verify Secret has new creation timestamp
   kubectl get secret platform-rndc-key -o yaml | grep created-at

   # 6. Verify pods were restarted
   kubectl get pods -l app.kubernetes.io/name=bind9 -o wide
   ```

3. **Add to CI/CD pipeline** (`.github/workflows/integration-test.yml`):
   - Run rotation test in Kind cluster
   - Verify rotation workflow completes successfully

**Success Criteria:**
- ✅ Integration test passes in Kind cluster
- ✅ Manual testing validates rotation workflow
- ✅ CI/CD pipeline includes rotation test
- ✅ All tests pass: `make kind-integration-test`

---

## Breaking Changes and Backward Compatibility

### Breaking Changes

1. **New CRD fields**: `rndcKeys` replaces `rndcSecretRef` (deprecated, not removed)
2. **CRD schema version**: Bumped from `v1beta1` to `v1beta1` (no version bump needed)
3. **Secret annotations**: Operator-managed Secrets will have new annotations

### Backward Compatibility Strategy

1. **Deprecated fields still work**: `rndc_secret_ref` continues to function
2. **Precedence order maintained**: Existing configs work without changes
3. **Opt-in rotation**: Auto-rotation is OFF by default unless explicitly enabled
4. **Migration path**: Users can migrate at their own pace

### Migration Steps for Users

1. **Review existing RNDC key configuration**:
   ```bash
   kubectl get bind9cluster -A -o yaml | grep -A 5 rndcSecretRef
   ```

2. **Update CRDs**:
   ```bash
   kubectl replace --force -f deploy/crds/
   ```

3. **Update manifests** to use new `rndcKeys` field (optional):
   ```yaml
   # OLD
   spec:
     global:
       rndcSecretRef:
         name: my-key

   # NEW
   spec:
     global:
       rndcKeys:
         autoRotate: true
         rotateAfter: 720h
         secretRef:
           name: my-key
   ```

4. **Deploy updated operator**:
   ```bash
   kubectl set image deployment/bindy-operator \
     operator=ghcr.io/firestoned/bindy:v0.6.0
   ```

5. **Monitor rotation** (if enabled):
   ```bash
   kubectl logs -l app=bindy-operator -f | grep rotation
   ```

---

## Security Considerations

### Key Material Protection

- **Secret encryption at rest**: Kubernetes Secrets should be encrypted at rest (cluster requirement)
- **RBAC**: Limit access to Secrets to operator ServiceAccount only
- **No logging of key material**: Ensure key material is never logged (use `[REDACTED]` in logs)

### Rotation Timing

- **Minimum rotation interval**: Enforce minimum `rotateAfter` of 1 hour to prevent thrashing
- **Maximum rotation interval**: Recommend maximum of 90 days for compliance
- **Graceful restart**: Ensure zero-downtime restarts during rotation

### Audit Trail

- **Track rotation events**: Log all key rotation events to audit log
- **Annotation history**: Consider adding rotation history to Secret annotations
- **Metrics**: Expose Prometheus metrics for rotation counts and failures

---

## Regulatory Compliance: NIST and FIPS Standards

This section provides detailed guidance on aligning RNDC key rotation with federal and industry security standards.

### NIST SP 800-57: Recommendation for Key Management

**Standard:** NIST Special Publication 800-57 Part 1, Rev. 5 (May 2020)

**Key Recommendations for Symmetric Authentication Keys (RNDC/TSIG keys):**

| Key Type | NIST Classification | Recommended Cryptoperiod | Bindy Configuration |
|----------|-------------------|--------------------------|---------------------|
| Symmetric Authentication Key (HMAC-SHA256) | Symmetric Key | 1-2 years | `rotateAfter: 2160h` (90 days) is **MORE conservative** |
| High-value assets | Symmetric Key | Shorter period recommended | `rotateAfter: 720h` (30 days) |
| Development/Testing | Symmetric Key | Up to 2 years acceptable | `rotateAfter: 4320h` (180 days) |

**NIST Cryptoperiod Definition:**
> "The time span during which a specific key is authorized for use by legitimate entities or the keys for a given system will remain in effect."

**Bindy's Approach vs. NIST:**
- **NIST allows**: Up to 2 years for symmetric authentication keys
- **Bindy default**: 90 days (MORE conservative than NIST)
- **Recommendation**: Use shorter intervals (30-90 days) to align with **defense-in-depth** and **zero-trust** principles

**NIST SP 800-57 Section 5.3 Compliance:**
- ✅ **Key generation**: RNDC keys use HMAC-SHA256 with 256-bit entropy (cryptographically secure)
- ✅ **Key storage**: Kubernetes Secrets (encrypted at rest via cluster configuration)
- ✅ **Key distribution**: Secrets mounted as volumes (in-memory, not written to disk)
- ✅ **Key rotation**: Automated with configurable intervals
- ✅ **Key destruction**: Old key material overwritten in Secret, Kubernetes garbage collection

**Documentation Requirements (NIST SP 800-57 Section 6):**
- Document key lifecycle policies
- Maintain audit logs of rotation events
- Record cryptographic algorithms used
- Track key creation and expiration timestamps

**Bindy Implementation:**
```yaml
# NIST-compliant configuration for production
spec:
  global:
    rndcKeys:
      autoRotate: true
      rotateAfter: 2160h  # 90 days (more conservative than NIST's 2 years)
      algorithm: hmac-sha256  # NIST-approved algorithm
```

---

### NIST SP 800-53: Security and Privacy Controls

**Standard:** NIST SP 800-53 Rev. 5 (September 2020)

**Relevant Controls:**

#### AC-2: Account Management (Control Enhancement AC-2(1))
**Control:** "The organization manages information system accounts, including... (f) Establishing conditions for group and role membership; and (g) Notifying account managers when accounts are no longer required."

**Bindy Compliance:**
- RNDC keys are "system accounts" for DNS server authentication
- Automated rotation ensures credentials don't remain valid indefinitely
- Annotation-based timestamp tracking provides audit trail
- Metrics expose key age for compliance monitoring

**Implementation Evidence:**
- Operator logs all rotation events (audit trail)
- Prometheus metrics track key age (`bindy_rndc_key_age_seconds`)
- Alerting detects keys exceeding configured lifetime

#### IA-5: Authenticator Management
**Control:** "The organization manages information system authenticators by... (e) Requiring individuals to take, and having devices implement, specific safeguards to protect authenticators; and (f) Changing authenticators... based on [Assignment: organization-defined time period by authenticator type]."

**Bindy Compliance:**
- ✅ **IA-5(1)(a)**: HMAC-SHA256 minimum (FIPS 198-1 approved)
- ✅ **IA-5(1)(b)**: Enforces minimum key length (256 bits)
- ✅ **IA-5(1)(c)**: Protects key material in Kubernetes Secrets
- ✅ **IA-5(1)(d)**: Encrypts authenticators at rest (cluster requirement)
- ✅ **IA-5(1)(e)**: Automated rotation based on time period
- ✅ **IA-5(1)(f)**: Generates cryptographically random keys

**Time Period Assignment (Organization-Defined):**
- **High impact systems**: 30 days (`rotateAfter: 720h`)
- **Moderate impact systems**: 60-90 days (`rotateAfter: 1440h` - `2160h`)
- **Low impact systems**: 180 days (`rotateAfter: 4320h`)

#### SC-12: Cryptographic Key Establishment and Management
**Control:** "The organization establishes and manages cryptographic keys for required cryptography employed within the information system."

**Bindy Compliance:**
- ✅ **SC-12(1)**: Keys available for authorized use only (RBAC on Secrets)
- ✅ **SC-12(2)**: Symmetric keys generated using approved methods (crypto-grade RNG)
- ✅ **SC-12(3)**: Symmetric keys protected during storage (Kubernetes Secret encryption)
- ✅ **SC-12(6)**: Automated key generation and rotation

**NIST SP 800-53 Control Baseline:**
| Control | Low Baseline | Moderate Baseline | High Baseline | Bindy Default |
|---------|--------------|-------------------|---------------|---------------|
| AC-2 | ✅ Implemented | ✅ Implemented | ✅ Implemented | ✅ Automated |
| IA-5 | ✅ Implemented | ✅ Enhanced (IA-5(1)) | ✅ Enhanced (IA-5(1)) | ✅ Enhanced |
| SC-12 | ✅ Implemented | ✅ Enhanced (SC-12(1-3)) | ✅ Enhanced (SC-12(1-6)) | ✅ Enhanced |

---

### FIPS 140-2/140-3: Cryptographic Module Validation

**Standards:**
- FIPS 140-2: Security Requirements for Cryptographic Modules (2001, updated 2002)
- FIPS 140-3: Security Requirements for Cryptographic Modules (2019, active 2021)

**FIPS Relevance to RNDC Keys:**

RNDC keys use HMAC (Hash-based Message Authentication Code) for authenticated communication. FIPS 198-1 defines HMAC requirements.

**FIPS 198-1 Compliance:**
- ✅ **Approved hash functions**: SHA-256, SHA-384, SHA-512 (Bindy supports all)
- ✅ **Key size**: Minimum 112 bits (Bindy uses 256 bits)
- ✅ **Key generation**: Cryptographically secure random number generator

**FIPS 140-2/140-3 Key Lifecycle Requirements:**

| Requirement | FIPS 140-2 Section | Bindy Implementation |
|-------------|-------------------|----------------------|
| Key Generation | 4.7.1 | Crypto-grade RNG (`rand` crate with `OsRng`) |
| Key Entry/Output | 4.7.2 | Keys never exported; stored in Kubernetes Secrets |
| Key Storage | 4.7.3 | Encrypted at rest (cluster-level encryption) |
| Key Zeroization | 4.7.4 | Old keys overwritten during rotation |

**FIPS 140-3 Transition (IG 2.4.A):**
- FIPS 140-2 modules can be used until **September 21, 2026**
- FIPS 140-3 will be required after that date
- Bindy's HMAC implementation is algorithm-agnostic (supports both)

**Key Rotation Impact on FIPS:**
- **FIPS does NOT mandate specific rotation intervals** for symmetric keys
- However, **NIST SP 800-57** (referenced by FIPS) recommends cryptoperiods
- Shorter cryptoperiods = **reduced risk** if crypto module is compromised

**FIPS-Compliant Configuration:**
```yaml
# FIPS 140-2/140-3 compliant configuration
spec:
  global:
    rndcKeys:
      autoRotate: true
      rotateAfter: 2160h  # 90 days (NIST recommendation)
      algorithm: hmac-sha256  # FIPS 198-1 approved
```

**CRITICAL:** FIPS compliance requires the **entire Kubernetes cluster** to run in FIPS mode:
- Use FIPS-validated Linux distributions (RHEL 8+ FIPS mode, Ubuntu Pro FIPS)
- Enable Kubernetes API server with `--fips` flag (where supported)
- Use FIPS-validated container images

**Bindy's Role in FIPS Environment:**
- Bindy uses FIPS-approved algorithms (HMAC-SHA256)
- Key generation uses OS-level RNG (FIPS-validated when cluster is in FIPS mode)
- Key rotation reduces blast radius of crypto failures

---

### Industry-Specific Compliance Standards

#### PCI DSS 3.2 (Payment Card Industry Data Security Standard)

**Requirement 8.2.4:** "Change user passwords/passphrases at least once every 90 days."

**Applicability:** While PCI DSS focuses on "user passwords," many auditors extend this to **service account credentials** and **API keys** (including RNDC keys).

**Bindy Compliance:**
```yaml
# PCI DSS-compliant configuration
spec:
  global:
    rndcKeys:
      autoRotate: true
      rotateAfter: 2160h  # 90 days (PCI DSS maximum)
```

**Audit Evidence:**
- Prometheus metrics showing rotation frequency
- Audit logs demonstrating automated rotation
- Configuration showing `rotateAfter: 2160h` or less

**PCI DSS SSC Guidance:**
> "Passwords/passphrases that are valid for a long time without a change provide malicious individuals with more time to work on breaking the password/phrase."

This guidance applies equally to RNDC keys used for DNS server authentication.

#### HIPAA Security Rule (Healthcare)

**Standard:** 45 CFR § 164.312(a)(2)(i) - Unique User Identification

**Applicability:** RNDC keys provide "unique identification" for DNS servers in healthcare infrastructure.

**Technical Safeguard Requirements:**
- ✅ Unique identifiers for each DNS server (instance-specific RNDC keys)
- ✅ Automatic logoff (key rotation = forced re-authentication)
- ✅ Encryption and decryption (HMAC provides authentication)
- ✅ Audit controls (rotation logged and monitored)

**HIPAA-Compliant Configuration:**
```yaml
# HIPAA-compliant configuration
spec:
  global:
    rndcKeys:
      autoRotate: true
      rotateAfter: 1440h  # 60 days (more conservative for PHI environments)
      algorithm: hmac-sha256
```

#### SOC 2 Trust Service Criteria

**Criteria:** CC6.1 - "The entity implements logical access security software, infrastructure, and architectures over protected information assets to protect them from security events to meet the entity's objectives."

**Bindy Compliance:**
- Automated key lifecycle management (reduces human error)
- Audit trail for all rotation events
- Monitoring and alerting for rotation failures
- Documentation of key management policies

**SOC 2 Audit Evidence:**
- Operator logs showing rotation events
- Metrics dashboards showing key age
- Documented rotation policies in compliance guide
- Incident response procedures for failed rotations

---

### Recommended Rotation Intervals by Environment

Based on NIST, FIPS, and industry standards, Bindy recommends:

| Environment Type | Risk Level | Rotation Interval | Configuration | Rationale |
|-----------------|-----------|-------------------|---------------|-----------|
| **Production - Financial Services** | Critical | 30 days | `rotateAfter: 720h` | PCI DSS, high regulatory scrutiny |
| **Production - Healthcare** | Critical | 60 days | `rotateAfter: 1440h` | HIPAA, PHI protection |
| **Production - Government** | High | 90 days | `rotateAfter: 2160h` | NIST SP 800-53 moderate baseline |
| **Production - Standard** | Moderate | 90 days | `rotateAfter: 2160h` | NIST SP 800-57 conservative |
| **Staging/QA** | Low-Moderate | 120 days | `rotateAfter: 2880h` | Balance security and operational overhead |
| **Development** | Low | 180 days | `rotateAfter: 4320h` | Minimal compliance requirements |

**CRITICAL:** These recommendations are **MORE conservative** than NIST SP 800-57 allows (up to 2 years). This aligns with:
- **Defense-in-depth** security strategy
- **Zero-trust** architecture principles
- **Assume breach** mentality (limit blast radius)

**Why Shorter Than NIST Allows?**
1. **Reduced exposure window**: If a key is compromised, limited time for exploitation
2. **Operational maturity**: Automated rotation makes short intervals practical
3. **Compliance confidence**: Demonstrates proactive security posture to auditors
4. **Industry best practice**: Most organizations use 30-90 days, not 2 years

---

### Compliance Validation and Audit Preparation

**Pre-Audit Checklist:**

- [ ] **Configuration Review**:
  - Verify `rotateAfter` meets organizational policy (≤ 90 days for production)
  - Confirm HMAC-SHA256 or stronger algorithm in use
  - Validate auto-rotation enabled for all production instances

- [ ] **Audit Trail**:
  - Collect operator logs showing rotation events
  - Export Prometheus metrics for key age over audit period
  - Document any manual rotations (with justification)

- [ ] **Documentation**:
  - Key lifecycle policy documented
  - Rotation intervals justified (map to NIST/industry standards)
  - Incident response procedures for failed rotations

- [ ] **Evidence Collection**:
  - Screenshots of Grafana dashboards showing rotation history
  - Sample Secret YAML showing rotation annotations
  - Operator configuration showing automated rotation enabled

- [ ] **Testing**:
  - Demonstrate manual rotation trigger (for auditor)
  - Show alerting for failed rotations
  - Prove pod restart after rotation

**Audit Questions and Answers:**

**Q: How do you ensure cryptographic keys are rotated regularly?**
**A:** "We use the Bindy operator's automated key rotation feature, configured to rotate RNDC keys every [30/60/90] days. Rotation is enforced via a dedicated Kubernetes operator that monitors key age and automatically generates new keys when the rotation interval is reached."

**Q: What is your key rotation policy based on?**
**A:** "Our policy aligns with NIST SP 800-57 (cryptoperiod recommendations) and NIST SP 800-53 (IA-5 authenticator management). We use a [30/60/90]-day interval, which is more conservative than NIST's 2-year maximum for symmetric authentication keys."

**Q: How do you track key rotation for compliance?**
**A:** "We maintain three layers of audit evidence: (1) Operator logs recording every rotation event with timestamps, (2) Prometheus metrics exposing key age and rotation counts, and (3) Kubernetes Secret annotations storing creation timestamps. Alerting notifies us if keys exceed their configured lifetime."

**Q: What happens if key rotation fails?**
**A:** "Our system includes exponential backoff retry logic and alerting. If rotation fails repeatedly, an alert fires to notify the operations team. We have documented incident response procedures including manual rotation and troubleshooting steps."

---

### Recommendations for Regulated Environments

**HIGH PRIORITY:**

1. **Enable auto-rotation immediately** in production:
   ```yaml
   spec:
     global:
       rndcKeys:
         autoRotate: true
         rotateAfter: 2160h  # 90 days (or shorter for high-security)
   ```

2. **Configure monitoring and alerting**:
   - Deploy Prometheus and Grafana
   - Set up `RndcKeyRotationFailing` and `RndcKeyTooOld` alerts
   - Configure PagerDuty/Opsgenie integration

3. **Document your key lifecycle policy**:
   - Create internal policy document referencing NIST standards
   - Define rotation intervals for each environment tier
   - Establish exception approval process (if manual rotation needed)

4. **Establish audit evidence collection**:
   - Automated log aggregation (Splunk, ELK, CloudWatch)
   - Metrics retention (Prometheus/Thanos with 1+ year retention)
   - Quarterly compliance reports showing rotation history

5. **Train operations team**:
   - Runbook for manual rotation procedures
   - Incident response for failed rotations
   - Key compromise response plan

**MEDIUM PRIORITY:**

6. **Implement FIPS mode** (if required):
   - Use FIPS-validated Linux distributions
   - Enable FIPS mode on Kubernetes nodes
   - Validate FIPS operation with `fips-mode-setup --check`

7. **Enhance Secret protection**:
   - Enable Kubernetes Secret encryption at rest
   - Use external secret management (HashiCorp Vault, AWS Secrets Manager)
   - Implement Secret RBAC restrictions

8. **Conduct annual audits**:
   - Internal review of key management practices
   - External audit by qualified assessor (for SOC 2, PCI DSS)
   - Penetration testing of DNS infrastructure

**LOW PRIORITY (Future Enhancements):**

9. **Implement key escrow** (if organizational policy requires)
10. **Enable multi-key overlap** during rotation (zero-downtime)
11. **Integrate with SIEM** for real-time security monitoring

---

### Does This Violate Any NIST/FIPS Policies?

**Short Answer: NO** - This implementation **EXCEEDS** NIST/FIPS requirements.

**Detailed Analysis:**

| Standard | Requirement | Bindy Implementation | Compliant? |
|----------|-------------|---------------------|------------|
| NIST SP 800-57 | Max 2 years for symmetric auth keys | Default 90 days (MORE conservative) | ✅ YES (exceeds) |
| NIST SP 800-53 IA-5 | Organization-defined rotation period | Configurable, defaults to 90 days | ✅ YES |
| FIPS 198-1 | HMAC-SHA256 minimum | HMAC-SHA256 default, supports stronger | ✅ YES |
| FIPS 140-2 Key Lifecycle | Generation, storage, zeroization | All requirements met | ✅ YES |
| PCI DSS 8.2.4 | Max 90 days for credentials | Default 90 days, configurable shorter | ✅ YES |

**Potential Concern: Too FREQUENT Rotation?**
- **NIST does NOT prohibit** short rotation intervals
- Frequent rotation is **encouraged** for defense-in-depth
- Only concern: operational overhead (mitigated by automation)

**Recommended Response to "Is this too frequent?" questions:**
> "While NIST SP 800-57 allows up to 2 years for symmetric authentication keys, we implement a 90-day default (or shorter) to align with zero-trust principles, defense-in-depth strategy, and industry best practices. Automated rotation eliminates operational overhead, making short intervals practical and secure."

**Final Verdict:**
- ✅ **Fully compliant** with NIST SP 800-57
- ✅ **Fully compliant** with NIST SP 800-53
- ✅ **Fully compliant** with FIPS 140-2/140-3
- ✅ **Exceeds** PCI DSS requirements
- ✅ **Exceeds** HIPAA Security Rule requirements
- ✅ **Meets** SOC 2 Trust Service Criteria

**No violations. This feature IMPROVES compliance posture.**

---

## Monitoring and Observability

### Metrics (Prometheus)

```rust
// In src/metrics.rs

// Key rotation metrics
pub static RNDC_KEY_ROTATION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new("bindy_rndc_key_rotation_total", "Total number of RNDC key rotations"),
        &["namespace", "secret"],
    )
    .unwrap()
});

pub static RNDC_KEY_ROTATION_ERRORS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new("bindy_rndc_key_rotation_errors_total", "Total number of RNDC key rotation errors"),
        &["namespace", "secret", "error"],
    )
    .unwrap()
});

pub static RNDC_KEY_AGE_SECONDS: Lazy<GaugeVec> = Lazy::new(|| {
    GaugeVec::new(
        Opts::new("bindy_rndc_key_age_seconds", "Age of RNDC keys in seconds"),
        &["namespace", "secret"],
    )
    .unwrap()
});
```

### Logging

- **DEBUG**: Key rotation checks (every reconciliation)
- **INFO**: Successful rotations, pod restarts
- **WARN**: Rotation errors, retries
- **ERROR**: Critical failures (e.g., API unavailable)

### Alerts (Prometheus)

```yaml
# Alert if key rotation fails repeatedly
- alert: RndcKeyRotationFailing
  expr: rate(bindy_rndc_key_rotation_errors_total[5m]) > 0.1
  for: 15m
  labels:
    severity: warning
  annotations:
    summary: "RNDC key rotation failing for {{ $labels.namespace }}/{{ $labels.secret }}"

# Alert if key is too old (beyond rotation interval + grace period)
- alert: RndcKeyTooOld
  expr: bindy_rndc_key_age_seconds > (90 * 24 * 3600 + 24 * 3600)  # 91 days
  labels:
    severity: warning
  annotations:
    summary: "RNDC key {{ $labels.namespace }}/{{ $labels.secret }} has not been rotated"
```

---

## Testing Strategy Summary

### Unit Tests

- `src/bind9/rndc_tests.rs`: Test key generation, Secret creation, rotation detection
- `src/reconcilers/bind9instance_tests.rs`: Test precedence resolution, backward compatibility
- `src/reconcilers/rndc_rotation_tests.rs`: Test rotation operator logic

### Integration Tests

- `tests/rndc_rotation_test.rs`: End-to-end rotation workflow in Kind cluster
- `.github/workflows/integration-test.yml`: CI/CD pipeline validation

### Manual Testing

- Deploy in development cluster
- Force rotation and verify workflow
- Test backward compatibility with deprecated fields
- Test all precedence levels (instance, role, global)

---

## Timeline Estimate

| Phase | Description | Estimated Duration |
|-------|-------------|-------------------|
| Phase 1 | API Changes (CRD schema) | 1-2 days |
| Phase 2 | Secret Creation Logic (TDD) | 2-3 days |
| Phase 3 | Bind9Instance Reconciler Updates | 2-3 days |
| Phase 4 | RndcKeyRotation Operator | 3-4 days |
| Phase 5 | Documentation and Migration Guide | 2-3 days |
| Phase 6 | Integration Testing | 2-3 days |
| **Total** | **End-to-end implementation** | **12-18 days** |

**Note:** Estimates assume full-time work. Adjust based on availability and priorities.

---

## Success Criteria (Overall)

### Code Implementation
- ✅ All CRD schemas updated and regenerated
- ✅ Backward compatibility maintained (deprecated fields work)
- ✅ Auto-rotation implemented and tested
- ✅ Manual rotation trigger works via annotation
- ✅ Separate rotation operator implemented (event-driven, not polling)
- ✅ Pods restart after rotation
- ✅ All unit tests pass (`cargo test`)
- ✅ All integration tests pass (`make kind-integration-test`)
- ✅ No clippy warnings (`cargo clippy --all-targets --all-features -- -D warnings`)
- ✅ Code formatted (`cargo fmt`)

### Documentation (CRITICAL for Regulated Environments)
- ✅ **User guide created** (`docs/src/user-guide/rndc-keys.md`):
  - API reference with examples
  - Configuration patterns
  - Troubleshooting guide
- ✅ **Regulatory compliance guide created** (`docs/src/compliance/rndc-key-rotation.md`):
  - NIST SP 800-57 alignment documented
  - NIST SP 800-53 control mapping
  - FIPS 140-2/140-3 considerations
  - PCI DSS, HIPAA, SOC 2 compliance evidence
  - Recommended rotation intervals by environment
- ✅ **Security documentation created** (`docs/src/security/rndc-key-security.md`):
  - Threat model analysis
  - Key material protection
  - Incident response procedures
- ✅ **Migration guide created** (`docs/src/migration/v0.6-rndc-keys.md`):
  - Breaking changes documented
  - Step-by-step migration instructions
  - Backward compatibility explained
- ✅ **Operations runbook created** (`docs/src/operations/rndc-key-rotation-runbook.md`):
  - Day 1 and Day 2 operations
  - Troubleshooting playbooks
  - Monitoring and alerting setup
- ✅ **Compliance checklist created** (`docs/src/compliance/checklist.md`):
  - Pre-deployment verification
  - Audit evidence collection
- ✅ **API reference updated** (`docs/src/reference/api.md`)
- ✅ **Quickstart guide updated** with rotation examples
- ✅ **CHANGELOG.md updated** with author attribution and compliance benefits
- ✅ **All documentation builds successfully** (`make docs`)
- ✅ **All internal documentation links validated**
- ✅ **All code examples in documentation tested and working**

### Compliance Validation
- ✅ **NIST SP 800-57 alignment verified** (cryptoperiod recommendations)
- ✅ **NIST SP 800-53 controls mapped** (AC-2, IA-5, SC-12)
- ✅ **FIPS 198-1 compliance confirmed** (HMAC-SHA256)
- ✅ **PCI DSS 8.2.4 compliance demonstrated** (90-day rotation)
- ✅ **HIPAA Security Rule alignment verified**
- ✅ **SOC 2 Trust Service Criteria met**
- ✅ **Audit evidence collection procedures documented**

---

## Future Enhancements (Out of Scope)

- **Multi-key support**: Support multiple RNDC keys per instance (for key rollover)
- **External Secret integration**: Integration with HashiCorp Vault, AWS Secrets Manager, etc.
- **Key rotation history**: Track rotation history in CRD status
- **Rotation windows**: Specify time windows for rotation (e.g., maintenance windows)
- **Gradual rollout**: Rotate keys on a per-instance basis with delays
- **Metrics dashboard**: Grafana dashboard for key rotation metrics

---

## Open Questions

1. **Should we support rotation of user-managed Secrets (`secretRef`)?**
   - **Proposed answer**: No. User-managed Secrets are the user's responsibility.

2. **Should we support different rotation intervals for primary vs. secondary?**
   - **Proposed answer**: Yes. Already supported via role-level `rndcKeys` config.

3. **What should happen if pod restart fails after rotation?**
   - **Proposed answer**: Log error, requeue, and increment error metric. User must investigate.

4. **Should we support rolling back to previous key if rotation fails?**
   - **Proposed answer**: Out of scope for initial implementation. Consider for future.

5. **Should we add a `bindy.firestoned.io/rotation-enabled` label to Secrets?**
   - **Proposed answer**: Yes. Add label to operator-managed Secrets with rotation enabled.

---

## References

### Kubernetes and BIND9
- [Kubernetes Secret Management Best Practices](https://kubernetes.io/docs/concepts/configuration/secret/)
- [BIND9 RNDC Documentation](https://bind9.readthedocs.io/en/latest/reference.html#rndc)
- [kube-rs Operator Pattern](https://kube.rs/operators/intro/)
- [Kubernetes Operator Best Practices](https://kubernetes.io/docs/concepts/architecture/operator/)
- [Go Duration Format](https://pkg.go.dev/time#ParseDuration)

### NIST Standards
- [NIST SP 800-57 Part 1 Rev. 5: Recommendation for Key Management (May 2020)](https://csrc.nist.gov/publications/detail/sp/800-57-part-1/rev-5/final)
  - Section 5.3: Cryptoperiod Recommendations for Specific Key Types
  - Table 5: Key Management Lifecycle (Generation, Storage, Distribution, Rotation, Destruction)
- [NIST SP 800-53 Rev. 5: Security and Privacy Controls (September 2020)](https://csrc.nist.gov/publications/detail/sp/800-53/rev-5/final)
  - AC-2: Account Management
  - IA-5: Authenticator Management
  - SC-12: Cryptographic Key Establishment and Management
- [NIST SP 800-131A Rev. 2: Transitioning the Use of Cryptographic Algorithms and Key Lengths (March 2019)](https://csrc.nist.gov/publications/detail/sp/800-131a/rev-2/final)

### FIPS Standards
- [FIPS 140-2: Security Requirements for Cryptographic Modules (2001)](https://csrc.nist.gov/publications/detail/fips/140/2/final)
  - Section 4.7: Cryptographic Key Management
- [FIPS 140-3: Security Requirements for Cryptographic Modules (2019)](https://csrc.nist.gov/publications/detail/fips/140/3/final)
- [FIPS 198-1: The Keyed-Hash Message Authentication Code (HMAC) (July 2008)](https://csrc.nist.gov/publications/detail/fips/198/1/final)
  - Approved hash functions and key sizes for HMAC
- [FIPS 140-3 Implementation Guidance](https://csrc.nist.gov/projects/cryptographic-module-validation-program/fips-140-3-ig)

### Industry Compliance Standards
- [PCI DSS v3.2.1: Payment Card Industry Data Security Standard (May 2018)](https://www.pcisecuritystandards.org/document_library)
  - Requirement 8.2.4: Change user passwords/passphrases at least once every 90 days
- [HIPAA Security Rule: 45 CFR Part 164](https://www.hhs.gov/hipaa/for-professionals/security/index.html)
  - § 164.312(a)(2)(i): Unique User Identification
  - § 164.312(e)(2)(ii): Encryption and Decryption
- [SOC 2 Trust Service Criteria (2017)](https://www.aicpa.org/interestareas/frc/assuranceadvisoryservices/aicpasoc2report.html)
  - CC6.1: Logical Access Security Software

### Additional Resources
- [CIS Kubernetes Benchmark v1.8](https://www.cisecurity.org/benchmark/kubernetes)
- [OWASP Kubernetes Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Kubernetes_Security_Cheat_Sheet.html)
- [NSA/CISA Kubernetes Hardening Guidance (August 2021)](https://media.defense.gov/2021/Aug/03/2002820425/-1/-1/0/CTR_Kubernetes_Hardening_Guidance_1.1_20220315.PDF)

---

## Appendix: Alternative Approaches Considered

### Approach 1: In-Place Secret Rotation (Chosen)

**Description**: Rotate keys by updating the existing Secret in-place.

**Pros:**
- Simple implementation
- No DNS propagation issues
- Minimal pod disruption

**Cons:**
- Brief downtime during pod restart
- Requires pod restart to pick up new key

**Decision**: Chosen for simplicity and minimal complexity.

### Approach 2: Blue-Green Key Rotation

**Description**: Create a new Secret, update pod config, delete old Secret.

**Pros:**
- Zero-downtime rotation
- Rollback capability

**Cons:**
- Complex implementation
- Requires pod affinity changes
- More testing required

**Decision**: Rejected for initial implementation (consider for future).

### Approach 3: Multi-Key Support (Deferred)

**Description**: Support multiple RNDC keys simultaneously during rotation.

**Pros:**
- True zero-downtime rotation
- Allows key overlap period

**Cons:**
- Significantly more complex
- Requires BIND9 configuration changes
- Harder to test

**Decision**: Deferred to future enhancement.

---

**End of Roadmap**
