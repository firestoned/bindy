# DNSSEC

Enable DNS Security Extensions (DNSSEC) for cryptographic validation of DNS responses.

## Overview

DNSSEC adds cryptographic signatures to DNS records, preventing:
- Cache poisoning
- Man-in-the-middle attacks
- Response tampering

## Enabling DNSSEC

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-dns
spec:
  config:
    dnssec:
      enabled: true      # Enable DNSSEC signing
      validation: true   # Enable DNSSEC validation
```

## DNSSEC Record Types

- **DNSKEY** - Public signing keys
- **RRSIG** - Resource record signatures
- **NSEC/NSEC3** - Proof of non-existence
- **DS** - Delegation signer (at parent zone)

## Verification

### Check DNSSEC Status

```bash
# Query with DNSSEC validation
dig @$SERVICE_IP example.com +dnssec

# Check for ad (authentic data) flag
dig @$SERVICE_IP example.com +dnssec | grep "flags.*ad"

# Verify RRSIG records
dig @$SERVICE_IP example.com RRSIG
```

### Validate Chain of Trust

```bash
# Check DS record at parent
dig @parent-dns example.com DS

# Verify DNSKEY matches DS
dig @$SERVICE_IP example.com DNSKEY
```

## Key Management

### Automatic Key Rotation

BIND9 handles automatic key rotation (future enhancement for Bindy configuration).

### Manual Key Management

```bash
# Generate keys (inside BIND9 pod)
kubectl exec -n dns-system deployment/primary-dns -- \
  dnssec-keygen -a RSASHA256 -b 2048 -n ZONE example.com

# Sign zone
kubectl exec -n dns-system deployment/primary-dns -- \
  dnssec-signzone -o example.com /var/lib/bind/zones/example.com.zone
```

## Troubleshooting

### DNSSEC Validation Failures

```bash
# Check validation logs
kubectl logs -n dns-system -l instance=primary-dns | grep dnssec

# Test with validation disabled
dig @$SERVICE_IP example.com +cd

# Verify time synchronization (critical for DNSSEC)
kubectl exec -n dns-system deployment/primary-dns -- date
```

## Best Practices

1. **Enable on primaries** - Sign at source
2. **Monitor expiration** - Alert on expiring signatures
3. **Test before enabling** - Verify in staging first
4. **Keep clocks synced** - NTP critical for DNSSEC
5. **Plan key rotation** - Regular key updates

## Next Steps

- [Security](./security.md) - Overall security strategy
- [Access Control](./access-control.md) - Query restrictions
