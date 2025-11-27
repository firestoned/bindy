# A Records (IPv4)

A records map domain names to IPv4 addresses.

## Creating an A Record

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

This creates `www.example.com -> 192.0.2.1`.

## Root Record

For the zone apex (example.com):

```yaml
spec:
  zone: example-com
  name: "@"
  ipv4Address: "192.0.2.1"
```

## Multiple A Records

Create multiple records for the same name for load balancing:

```bash
kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-1
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.1"
---
apiVersion: dns.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-2
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.2"
