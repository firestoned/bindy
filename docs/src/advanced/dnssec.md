# DNSSEC

DNS Security Extensions (DNSSEC) provides cryptographic authentication of DNS data.

## Current Implementation Status

**✅ DNSSEC Validation:** Bindy currently supports DNSSEC **validation** of responses from upstream nameservers.

**🚧 DNSSEC Zone Signing:** Automatic zone signing is planned but not yet implemented. See [DNSSEC Zone Signing Roadmap](../roadmaps/dnssec-zone-signing-implementation.md) for implementation timeline.

---

## DNSSEC Validation (Current Feature)

### Overview

When DNSSEC validation is enabled, BIND9 will verify cryptographic signatures on DNS responses from other nameservers. This protects against:
- Cache poisoning attacks
- Man-in-the-middle tampering
- Spoofed DNS responses

**Important:** DNSSEC validation requires:

- Valid DNSSEC trust anchors
- Proper network connectivity to root DNS servers
- Accurate system time (NTP synchronization)

Invalid or missing DNSSEC signatures will cause queries to fail when validation is enabled.

### Enabling DNSSEC Validation

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
      validation: true  # Instance-specific override
```

### Verification

Check that DNSSEC validation is working:

```bash
# Get BIND9 pod IP or service IP
SERVICE_IP=$(kubectl get svc -n dns-system production-dns-primary -o jsonpath='{.spec.clusterIP}')

# Query a DNSSEC-signed domain
dig @$SERVICE_IP cloudflare.com +dnssec

# Look for the 'ad' (authentic data) flag in the response
# flags: qr rd ra ad; QUERY: 1, ANSWER: 1, ...
#                ^^-- This indicates successful DNSSEC validation

# Query a domain known to have broken DNSSEC
# This should FAIL if validation is working correctly
dig @$SERVICE_IP dnssec-failed.org
```

### Troubleshooting DNSSEC Validation

#### Validation Failures

If queries fail with validation enabled:

```bash
# Check BIND9 logs for DNSSEC errors
kubectl logs -n dns-system -l app.kubernetes.io/component=bind9 | grep -i dnssec

# Common errors:
# - "broken trust chain" - Missing or invalid DS records
# - "no valid signature found" - Expired or missing RRSIG
# - "validation failed" - Signature verification failed
```

#### Bypass Validation for Testing

To query without DNSSEC validation (for debugging):

```bash
# Use +cd (checking disabled) flag
dig @$SERVICE_IP example.com +cd

# This retrieves the response without validating signatures
```

#### Time Synchronization Issues

DNSSEC signatures have validity periods and will fail if system time is incorrect:

```bash
# Check BIND9 pod time
kubectl exec -n dns-system -l app.kubernetes.io/component=bind9 -- date

# Compare with actual time
date

# If time is off, ensure NTP is configured on nodes
```

---

## DNSSEC Zone Signing (Planned Feature)

**Status:** 🚧 Not yet implemented

Bindy will support automatic DNSSEC zone signing in a future release, including:

- Automatic key generation (KSK and ZSK)
- Configurable signing policies
- Automatic key rotation
- DS record generation for parent zones
- NSEC3 support for authenticated denial

For details, see:

- [DNSSEC Zone Signing Roadmap](../roadmaps/dnssec-zone-signing-implementation.md)
- [Roadmap vs Current State Analysis](../roadmaps/dnssec-roadmap-vs-current-state-analysis.md)

### Manual Zone Signing (Workaround)

Until automatic signing is implemented, you can manually sign zones inside BIND9 pods:

```bash
# 1. Generate keys (inside BIND9 pod)
kubectl exec -n dns-system -l app.kubernetes.io/component=bind9 -- \
  dnssec-keygen -a ECDSAP256SHA256 -n ZONE example.com

# 2. Sign the zone file
kubectl exec -n dns-system -l app.kubernetes.io/component=bind9 -- \
  dnssec-signzone -o example.com /var/cache/bind/db.example.com

# 3. Extract DS record for parent zone
kubectl exec -n dns-system -l app.kubernetes.io/component=bind9 -- \
  cat dsset-example.com.
```

**Important:** Manual signing is not persistent across pod restarts and does not integrate with Bindy's declarative zone management. Use only for testing or temporary purposes.

---

## DNSSEC Record Types

Understanding DNSSEC record types:

| Record Type | Purpose | Example |
|-------------|---------|---------|
| **DNSKEY** | Public signing keys (KSK and ZSK) | `example.com. IN DNSKEY 257 3 13 ...` |
| **RRSIG** | Cryptographic signatures for records | `example.com. IN RRSIG A 13 2 3600 ...` |
| **NSEC** | Proof of non-existence (shows what doesn't exist) | `example.com. IN NSEC www.example.com. A RRSIG` |
| **NSEC3** | Privacy-preserving proof of non-existence (hashed) | `abc123.example.com. IN NSEC3 1 0 10 ...` |
| **DS** | Delegation signer (published in parent zone) | `example.com. IN DS 12345 13 2 ABC...` |

**Note:** When DNSSEC signing is implemented, Bindy will automatically generate and manage these records.

---

## Best Practices

### For DNSSEC Validation (Current)

1. **Test before production** - Enable validation in staging first to identify upstream DNSSEC issues
2. **Monitor query failures** - Alert on increased SERVFAIL responses after enabling
3. **Keep trust anchors updated** - BIND9 manages this automatically via RFC 5011
4. **Ensure NTP sync** - DNSSEC signature validation requires accurate time
5. **Document upstream dependencies** - Know which upstream resolvers support DNSSEC

### For DNSSEC Signing (Future)

When zone signing is implemented:

1. **Start with test zones** - Validate DS record publication process
2. **Monitor key expiration** - Alert before keys expire
3. **Automate DS updates** - Integrate with domain registrar APIs where possible
4. **Use NSEC3** - Provides better privacy than NSEC for zone enumeration
5. **Plan for emergencies** - Have procedures for emergency key rollovers

---

## Reference

### DNSSEC RFCs

- [RFC 4033](https://www.rfc-editor.org/rfc/rfc4033.html) - DNS Security Introduction and Requirements
- [RFC 4034](https://www.rfc-editor.org/rfc/rfc4034.html) - Resource Records for DNSSEC
- [RFC 4035](https://www.rfc-editor.org/rfc/rfc4035.html) - Protocol Modifications for DNSSEC
- [RFC 5155](https://www.rfc-editor.org/rfc/rfc5155.html) - NSEC3 (Hashed Authenticated Denial)
- [RFC 8624](https://www.rfc-editor.org/rfc/rfc8624.html) - Algorithm Requirements and Usage

### BIND9 Documentation

- [BIND9 DNSSEC Guide](https://bind9.readthedocs.io/en/latest/dnssec-guide.html)
- [DNSSEC Validation Configuration](https://bind9.readthedocs.io/en/latest/reference.html#dnssec-validation)

### External Tools

- [DNSViz](https://dnsviz.net/) - DNSSEC visualization and debugging
- [Verisign DNSSEC Analyzer](https://dnssec-analyzer.verisignlabs.com/) - DNSSEC chain validation
- [DNSSEC Debugger](https://dnssec-debugger.verisignlabs.com/) - Interactive DNSSEC testing

---

## Next Steps

- [Security Overview](./security.md) - Overall security strategy
- [Access Control](./access-control.md) - Query restrictions and ACLs
- [RNDC Key Rotation](../guide/rndc-key-rotation.md) - Automatic RNDC key rotation (implemented)
