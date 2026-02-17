# DNSSEC

DNS Security Extensions (DNSSEC) provides cryptographic authentication of DNS data.

## Implementation Status

| Feature | Status | Notes |
|---------|--------|-------|
| DNSSEC Validation | ✅ Implemented | Validate upstream responses via `dnssec.validation` |
| CRD Schema (Phase 1) | ✅ Implemented | `DNSSECConfig`, `DNSSECSigningConfig`, `DNSSECKeySource` types |
| Policy Configuration (Phase 2) | ✅ Implemented | `dnssec-policy` blocks generated in `named.conf` |
| Key Source Configuration (Phase 3) | ✅ Implemented | Secret-backed and auto-generated keys |
| Zone Signing Configuration (Phase 4) | ✅ Implemented | Per-zone `dnssecPolicy` field, inline signing via bindcar |
| DS Record Status Reporting (Phase 5) | 🚧 Planned | Status struct exists; extraction logic not yet implemented |
| Integration Tests (Phase 6) | 🚧 Planned | Unit tests exist; end-to-end suite pending |

---

## DNSSEC Validation

When DNSSEC validation is enabled, BIND9 verifies cryptographic signatures on DNS responses from upstream nameservers. This protects against cache poisoning, man-in-the-middle tampering, and spoofed DNS responses.

**Requirements:**
- Valid DNSSEC trust anchors (managed automatically by BIND9 via RFC 5011)
- Network connectivity to root DNS servers
- Accurate system time (NTP synchronization)

### Configuration

Configure validation in the `Bind9Cluster` global configuration:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  global:
    dnssec:
      validation: true  # Enable DNSSEC validation of upstream responses
```

Or override per-instance in `Bind9Instance`:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: production-dns
  config:
    dnssec:
      validation: true
```

### Verification

```bash
SERVICE_IP=$(kubectl get svc -n dns-system production-dns-primary -o jsonpath='{.spec.clusterIP}')

# Query a DNSSEC-signed domain - look for 'ad' (authentic data) flag
dig @$SERVICE_IP cloudflare.com +dnssec
# flags: qr rd ra ad; QUERY: 1, ANSWER: 1, ...
#                ^^-- successful DNSSEC validation

# Query a domain with broken DNSSEC - should FAIL with SERVFAIL
dig @$SERVICE_IP dnssec-failed.org
```

### Troubleshooting Validation

```bash
# Check BIND9 logs for DNSSEC errors
kubectl logs -n dns-system -l app.kubernetes.io/component=bind9 | grep -i dnssec

# Common errors:
# "broken trust chain"      - Missing or invalid DS records in parent zone
# "no valid signature found" - Expired or missing RRSIG records
# "validation failed"        - Signature verification failed

# Query without validation for debugging
dig @$SERVICE_IP example.com +cd  # +cd = checking disabled
```

```bash
# Verify NTP sync - DNSSEC signatures have validity periods
kubectl exec -n dns-system -l app.kubernetes.io/component=bind9 -- date
```

---

## DNSSEC Zone Signing

Bindy supports declarative DNSSEC zone signing using BIND9's modern `dnssec-policy` mechanism. Configuration is via `Bind9Cluster` (global policy) and optionally overridden per `DNSZone`.

### How It Works

1. The operator generates a `dnssec-policy` block in `named.conf` from your CRD config
2. BIND9 handles key generation, signing, and automatic key rotation
3. Each `DNSZone` can specify a policy via `spec.dnssecPolicy` (or inherit the cluster default)

### Cluster-Level Signing Configuration

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  global:
    dnssec:
      validation: true
      signing:
        enabled: true
        policy: "default"          # Policy name referenced by zones
        algorithm: "ECDSAP256SHA256"  # Recommended: ECDSA P-256
        kskLifetime: "365d"        # Key Signing Key lifetime
        zskLifetime: "90d"         # Zone Signing Key lifetime
        nsec3: true                # Use NSEC3 (privacy-preserving)
        nsec3Iterations: 0         # Per RFC 9276 recommendation

        # Key management
        autoGenerate: true         # BIND9 generates keys automatically
        exportToSecret: true       # Back up generated keys to a Secret
```

This generates a `dnssec-policy` block in `named.conf` similar to:

```bind
dnssec-policy "default" {
    keys {
        ksk lifetime 365d algorithm ECDSAP256SHA256;
        zsk lifetime 90d algorithm ECDSAP256SHA256;
    };
    nsec3param iterations 0 optout no salt-length 16;
    signatures-refresh 5d;
    signatures-validity 30d;
    signatures-validity-dnskey 30d;
    zone-propagation-delay 300;
    parent-propagation-delay 3600;
    max-zone-ttl 86400;
};
```

### Per-Zone Policy Override

Each `DNSZone` inherits the cluster's DNSSEC signing policy by default. Override it with `spec.dnssecPolicy`:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: production-dns
  soaRecord:
    primaryNs: ns1.example.com.
    adminEmail: admin.example.com.
    serial: 2026012801
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600

  # Override cluster default or disable signing for this zone:
  # dnssecPolicy: "high-security"  # Use a different named policy
  # dnssecPolicy: "none"           # Disable signing for this zone
```

When `dnssecPolicy` is set, `inline-signing yes;` is automatically added to the zone configuration.

### Key Source Options

#### Option 1: Auto-Generated Keys (Recommended for Development)

```yaml
signing:
  enabled: true
  policy: "default"
  autoGenerate: true
  exportToSecret: true  # Back up keys to a Kubernetes Secret
```

BIND9 generates KSK and ZSK keys automatically. With `exportToSecret: true`, the operator exports the generated keys to a Secret for backup and recovery.

#### Option 2: User-Supplied Keys (Recommended for Production)

```yaml
signing:
  enabled: true
  policy: "default"
  keysFrom:
    secretRef:
      name: my-dnssec-keys
      namespace: dns-system
```

Supply pre-generated keys via a Kubernetes Secret. The Secret is mounted read-only at `/var/cache/bind/keys`. This is the recommended production approach as it gives full control over key material.

### Algorithm Selection

| Algorithm | OID | Recommended Use |
|-----------|-----|-----------------|
| `ECDSAP256SHA256` | 13 | **Default — modern, fast, small keys** |
| `ECDSAP384SHA384` | 14 | Higher security margin, slightly larger |
| `RSASHA256` | 8 | Legacy compatibility only |

Use `ECDSAP256SHA256` unless you have a specific compatibility requirement. Per [RFC 8624](https://www.rfc-editor.org/rfc/rfc8624.html), ECDSA algorithms are MUST implement for modern resolvers.

### NSEC vs NSEC3

| Setting | Privacy | Notes |
|---------|---------|-------|
| `nsec3: false` | ❌ Zone enumerable | Simpler, lower overhead |
| `nsec3: true` | ✅ Hashed names | Recommended; use `nsec3Iterations: 0` per RFC 9276 |

---

## Completing the Chain of Trust

After zones are signed, publish DS records in the parent zone to complete the DNSSEC chain of trust. The DS record links the child zone's KSK to the parent zone's trust.

**Note:** DS record extraction (Phase 5) is not yet automated. Extract DS records manually:

```bash
# Get DS records from signed zone
kubectl exec -n dns-system -l app.kubernetes.io/component=bind9 -- \
  dig @localhost example.com DNSKEY | dnssec-dsfromkey -f - example.com

# Or extract from BIND9's key directory
kubectl exec -n dns-system -l app.kubernetes.io/component=bind9 -- \
  cat /var/cache/bind/keys/dsset-example.com.
```

Publish the output DS records at your domain registrar or parent zone operator.

---

## DNSSEC Record Types Reference

| Record | Purpose |
|--------|---------|
| **DNSKEY** | Public signing keys (KSK and ZSK) |
| **RRSIG** | Cryptographic signatures for each RRset |
| **NSEC** | Proof of non-existence (zone-enumerable) |
| **NSEC3** | Privacy-preserving proof of non-existence (hashed names) |
| **DS** | Delegation signer — published in the parent zone |

---

## Best Practices

1. **Test in staging first** — Enable signing on non-critical zones before production
2. **Use NSEC3** — Set `nsec3: true` and `nsec3Iterations: 0` per RFC 9276
3. **Use ECDSAP256SHA256** — Modern, compact, widely supported
4. **Back up keys** — Set `exportToSecret: true` or use `keysFrom.secretRef` for user-managed keys
5. **Publish DS records promptly** — Signed zones without a parent DS record are signed but not validated by resolvers
6. **Monitor for expiry** — Alert on RRSIG validity windows; BIND9 auto-renews but monitor for issues
7. **Plan DS rollovers** — KSK rollovers require coordinating DS record updates with the parent zone

---

## Reference

### RFCs

- [RFC 4033](https://www.rfc-editor.org/rfc/rfc4033.html) - DNS Security Introduction and Requirements
- [RFC 4034](https://www.rfc-editor.org/rfc/rfc4034.html) - Resource Records for DNSSEC
- [RFC 4035](https://www.rfc-editor.org/rfc/rfc4035.html) - Protocol Modifications for DNSSEC
- [RFC 5155](https://www.rfc-editor.org/rfc/rfc5155.html) - NSEC3 (Hashed Authenticated Denial)
- [RFC 8624](https://www.rfc-editor.org/rfc/rfc8624.html) - Algorithm Requirements and Usage
- [RFC 9276](https://www.rfc-editor.org/rfc/rfc9276.html) - NSEC3 Parameter Guidance

### BIND9 Documentation

- [BIND9 DNSSEC Guide](https://bind9.readthedocs.io/en/latest/dnssec-guide.html)
- [BIND9 dnssec-policy Reference](https://bind9.readthedocs.io/en/latest/reference.html#dnssec-policy)
- [DNSSEC Validation Configuration](https://bind9.readthedocs.io/en/latest/reference.html#dnssec-validation)

### External Tools

- [DNSViz](https://dnsviz.net/) - DNSSEC chain visualization and debugging
- [Verisign DNSSEC Analyzer](https://dnssec-analyzer.verisignlabs.com/) - Full chain validation
- [DNSSEC Debugger](https://dnssec-debugger.verisignlabs.com/) - Interactive DNSSEC testing

---

## See Also

- [Security Overview](./security.md)
- [Access Control](./access-control.md)
- [RNDC Key Rotation](../guide/rndc-key-rotation.md)
- [DNSSEC Zone Signing Implementation Roadmap](https://github.com/firestoned/bindy/blob/main/docs/roadmaps/dnssec-zone-signing-implementation.md)
