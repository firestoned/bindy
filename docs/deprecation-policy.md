# Deprecation Policy

**Project:** Bindy - Kubernetes Operator for BIND9
**Version:** 0.2.0
**Last Updated:** 2026-01-09

---

## Overview

This document tracks all deprecated fields, types, and APIs in the Bindy codebase. It provides migration guidance and removal timelines for users.

## Policy Guidelines

1. **Deprecation Notice Period**: Minimum 2 minor versions (e.g., deprecated in 0.2.0, removed no earlier than 0.4.0)
2. **Breaking Change Notice**: Deprecation removals are considered breaking changes and require major version bump (e.g., 0.x → 1.0 or 1.x → 2.0)
3. **Migration Path**: All deprecated items must have a clear migration path documented
4. **Warnings**: Deprecated fields emit warnings in logs when used
5. **Documentation**: API documentation must clearly mark deprecated items

---

## Active Deprecations

### 1. `RecordStatus.zone` Field

**Status:** ⚠️ Deprecated
**Deprecated Since:** v0.2.0 (2026-01-08)
**Planned Removal:** v0.4.0 or v1.0.0 (whichever comes first)
**Reason:** Replaced by structured `zone_ref` field for event-driven architecture

#### What It Is

The `zone` field in `RecordStatus` is a simple string containing the FQDN of the zone that owns a DNS record (e.g., `"example.com"`).

```yaml
# Old approach (deprecated)
status:
  zone: "example.com"
```

#### Why It's Deprecated

The `zone` string field doesn't provide enough information for the record reconciler to:
- Look up the parent `DNSZone` Kubernetes resource
- Find the zone's primary `Bind9Instance` servers
- Determine the namespace of the zone

The new `zone_ref` field provides a complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.

#### Migration Path

**For Users:**
- No action required - the operator maintains both fields for backward compatibility
- The `DNSZone` operator sets both `status.zone` (string) and `status.zoneRef` (structured reference)
- Your existing queries using `.status.zone` will continue to work

**For Developers/Integrations:**
- Update code that reads `status.zone` to use `status.zoneRef` instead
- New field structure:
  ```yaml
  status:
    zone_ref:
      apiVersion: dns.firestoned.io/v1beta1
      kind: DNSZone
      name: example-com
      namespace: dns-system
      zoneName: example.com
  ```

#### Code Examples

**Before (deprecated):**
```rust
// Reading the zone string
let zone_fqdn = record.status.zone.as_ref();
```

**After (recommended):**
```rust
// Reading the structured zone reference
let zone_ref = record.status.zone_ref.as_ref();
let zone_fqdn = zone_ref.map(|z| &z.zone_name);
let zone_namespace = zone_ref.map(|z| &z.namespace);
```

#### Timeline

- **v0.2.0 (2026-01-08)**: Field marked as deprecated, both `zone` and `zone_ref` set by operator
- **v0.3.0 (Target: 2026-Q1)**: Add warning logs when `zone` is read by integrations
- **v0.4.0 or v1.0.0 (Target: 2026-Q2)**: Remove `zone` field entirely (breaking change)

---

### 2. `TSIGKey` Struct

**Status:** ⚠️ Soft Deprecated (not enforced yet)
**Deprecated Since:** v0.2.0 (mentioned in comments)
**Planned Removal:** v1.0.0
**Reason:** Replaced by `RndcSecretRef` for better Kubernetes secret integration

#### What It Is

The `TSIGKey` struct allows inline specification of TSIG keys for authenticated zone transfers:

```rust
pub struct TSIGKey {
    pub name: String,
    pub algorithm: RndcAlgorithm,
    pub secret: String,  // Base64-encoded secret
}
```

#### Why It's Deprecated

**Security Concerns:**
- Inline secrets in CRD specs are visible in etcd and kubectl output
- Secrets should be stored in Kubernetes `Secret` resources
- `RndcSecretRef` provides proper secret management

**Better Alternative:**
The `RndcSecretRef` struct references a Kubernetes Secret:

```yaml
# Old approach (deprecated)
spec:
  tsigKey:
    name: my-key
    algorithm: hmac-sha256
    secret: "dGVzdC1zZWNyZXQ="  # Visible in plain text!

# New approach (recommended)
spec:
  rndcSecretRef:
    name: bind9-rndc-secret
    namespace: dns-system
```

#### Migration Path

**For Users:**
1. Create a Kubernetes Secret with your RNDC key:
   ```bash
   kubectl create secret generic bind9-rndc-secret \
     --from-literal=rndc.key="$(cat rndc.key)"
   ```

2. Update your `Bind9Instance` or `Bind9Cluster` spec:
   ```yaml
   # Remove:
   # spec:
   #   tsigKey:
   #     name: my-key
   #     algorithm: hmac-sha256
   #     secret: "dGVzdC1zZWNyZXQ="

   # Add:
   spec:
     rndcSecretRef:
       name: bind9-rndc-secret
       namespace: dns-system
   ```

#### Timeline

- **v0.2.0 (2026-01-08)**: `TSIGKey` marked as deprecated in documentation
- **v0.3.0 (Target: 2026-Q1)**: Add `#[deprecated]` attribute to `TSIGKey` struct
- **v0.3.0 (Target: 2026-Q1)**: Log warnings when `tsigKey` is used
- **v1.0.0 (Target: 2026-Q3)**: Remove `TSIGKey` struct and all support (breaking change)

---

## Removed Deprecations

### None Yet

The first deprecated items will be removed in v0.4.0 or v1.0.0.

---

## How to Handle Deprecated Fields

### For Users

When you see deprecation warnings:
1. Check this document for migration guidance
2. Update your YAML manifests to use the new fields
3. Test the changes in a non-production environment
4. Roll out the changes to production before the removal deadline

### For Developers

When deprecating a field:
1. Add `#[deprecated(since = "x.y.z", note = "Use ... instead")]` attribute in Rust
2. Update this document with deprecation details and migration path
3. Add warning logs when the deprecated field is used
4. Set a removal timeline (minimum 2 minor versions)
5. Update API documentation to mark the field as deprecated
6. Update all examples to use the new field

---

## Version History

- **2026-01-09**: Initial deprecation policy document created
  - Documented `RecordStatus.zone` deprecation (v0.2.0)
  - Documented `TSIGKey` soft deprecation (v0.2.0)

---

## Related Documents

- [API Reference](./src/reference/api.md) - Generated API documentation
- [CHANGELOG.md](../CHANGELOG.md) - Full version history
- [Migration Guide](./migrations/) - Version-specific migration guides (future)
