# A Records (IPv4)

A records map domain names to IPv4 addresses.

## Creating an A Record

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zoneRef: example-com  # References DNSZone metadata.name (recommended)
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

This creates `www.example.com -> 192.0.2.1`.

**Note:** You can also use `zone: example.com` (matching `DNSZone.spec.zoneName`) instead of `zoneRef`. See [Referencing DNS Zones](./records-guide.md#referencing-dns-zones) for details on choosing between `zone` and `zoneRef`.

## Root Record

For the zone apex (example.com):

```yaml
spec:
  zoneRef: example-com
  name: "@"
  ipv4Address: "192.0.2.1"
```

## Multiple A Records

Create multiple records for the same name for load balancing:

```bash
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-1
spec:
  zoneRef: example-com
  name: www
  ipv4Address: "192.0.2.1"
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-2
spec:
  zoneRef: example-com
  name: www
  ipv4Address: "192.0.2.2"
