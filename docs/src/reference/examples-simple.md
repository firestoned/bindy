# Simple Setup Example

Complete configuration for a basic single-instance DNS setup.

## Overview

This example demonstrates:
- Single Bind9Instance
- One DNS zone (example.com)
- Common DNS records (A, AAAA, CNAME, MX, TXT)
- Suitable for testing and development

## Prerequisites

- Kubernetes cluster (kind, minikube, or cloud)
- kubectl configured
- Bindy operator installed

## Configuration

### Complete YAML

Save as `simple-dns.yaml`:

```yaml
---
# Namespace
apiVersion: v1
kind: Namespace
metadata:
  name: dns-system

---
# Bind9Instance - Single DNS Server
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: simple-dns
  namespace: dns-system
  labels:
    app: bindy
    dns-role: primary
    environment: development
spec:
  replicas: 1
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer: []
    listenOn:
      - "any"
    listenOnV6:
      - "any"

---
# DNSZone - example.com
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: "example.com"
  zoneType: "primary"
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "admin@example.com"
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600

---
# A Record - Nameserver
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ns1-a-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "ns1"
  ipv4Address: "192.0.2.1"
  ttl: 3600

---
# A Record - Web Server
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-a-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "www"
  ipv4Address: "192.0.2.10"
  ttl: 300

---
# AAAA Record - Web Server (IPv6)
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-aaaa-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "www"
  ipv6Address: "2001:db8::10"
  ttl: 300

---
# A Record - Mail Server
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: mail-a-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "mail"
  ipv4Address: "192.0.2.20"
  ttl: 3600

---
# MX Record - Mail Exchange
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "@"
  priority: 10
  mailServer: "mail.example.com."
  ttl: 3600

---
# TXT Record - SPF
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: spf-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "@"
  text:
    - "v=spf1 mx -all"
  ttl: 3600

---
# TXT Record - DMARC
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: dmarc-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "_dmarc"
  text:
    - "v=DMARC1; p=none; rua=mailto:dmarc@example.com"
  ttl: 3600

---
# CNAME Record - API Alias
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: api-cname-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "api"
  target: "www.example.com."
  ttl: 3600
```

## Deployment

### 1. Install CRDs

From latest release:

```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml
```

Or from local files:

```bash
kubectl apply -k deploy/crds/
```

### 2. Install RBAC

From latest release:

```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/serviceaccount.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/role.yaml
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/rbac/rolebinding.yaml
```

Or from local files:

```bash
kubectl apply -f deploy/rbac/
```

### 3. Deploy Bindy Operator

From latest release:

```bash
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/operator/deployment.yaml
```

Or from local files:

```bash
kubectl apply -f deploy/operator/deployment.yaml
```

### 4. Apply Configuration

```bash
kubectl apply -f simple-dns.yaml
```

### 5. Verify Deployment

```bash
# Check Bind9Instance
kubectl get bind9instances -n dns-system
kubectl describe bind9instance simple-dns -n dns-system

# Check DNSZone
kubectl get dnszones -n dns-system
kubectl describe dnszone example-com -n dns-system

# Check DNS Records
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords -n dns-system

# Check pods
kubectl get pods -n dns-system

# Check logs
kubectl logs -n dns-system -l app=bindy
```

## Testing

### DNS Queries

Get the DNS service IP:

```bash
DNS_IP=$(kubectl get svc -n dns-system simple-dns -o jsonpath='{.spec.clusterIP}')
```

Test DNS resolution:

```bash
# A record
dig @${DNS_IP} www.example.com A

# AAAA record
dig @${DNS_IP} www.example.com AAAA

# MX record
dig @${DNS_IP} example.com MX

# TXT record
dig @${DNS_IP} example.com TXT

# CNAME record
dig @${DNS_IP} api.example.com CNAME
```

Expected responses:

```
; www.example.com A
www.example.com.    300    IN    A    192.0.2.10

; www.example.com AAAA
www.example.com.    300    IN    AAAA    2001:db8::10

; example.com MX
example.com.        3600   IN    MX    10 mail.example.com.

; example.com TXT
example.com.        3600   IN    TXT   "v=spf1 mx -all"

; api.example.com CNAME
api.example.com.    3600   IN    CNAME www.example.com.
```

### Port Forward for External Testing

```bash
# Forward DNS port to localhost
kubectl port-forward -n dns-system svc/simple-dns 5353:53

# Test from local machine
dig @localhost -p 5353 www.example.com
```

## Monitoring

### Check Status

```bash
# Instance status
kubectl get bind9instance simple-dns -n dns-system -o yaml | grep -A 10 status

# Zone status
kubectl get dnszone example-com -n dns-system -o yaml | grep -A 10 status

# Record status
kubectl get arecord www-a-record -n dns-system -o yaml | grep -A 10 status
```

### View Logs

```bash
# Operator logs
kubectl logs -n dns-system deployment/bindy

# BIND9 logs
kubectl logs -n dns-system -l app=bindy,dns-role=primary
```

## Updating Configuration

### Add New Record

```bash
cat <<EOF | kubectl apply -f -
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: app-a-record
  namespace: dns-system
spec:
  zone: "example-com"
  name: "app"
  ipv4Address: "192.0.2.30"
  ttl: 300
EOF
```

### Update SOA Serial

```bash
kubectl edit dnszone example-com -n dns-system

# Update serial field:
# serial: 2024010102
```

### Scale Instance

```bash
kubectl patch bind9instance simple-dns -n dns-system \
  --type merge \
  --patch '{"spec":{"replicas":2}}'
```

## Cleanup

### Remove All Resources

```bash
kubectl delete -f simple-dns.yaml
```

### Remove Namespace

```bash
kubectl delete namespace dns-system
```

## Next Steps

- [Production Setup](./examples-production.md) - Add HA and monitoring
- [Multi-Region Setup](./examples-multi-region.md) - Geographic distribution
- [Operations Guide](../operations/monitoring.md) - Monitoring and troubleshooting

## Troubleshooting

### Pods Not Starting

```bash
# Check pod events
kubectl describe pod -n dns-system -l app=bindy

# Check operator logs
kubectl logs -n dns-system deployment/bindy
```

### DNS Not Resolving

```bash
# Check zone status
kubectl get dnszone example-com -n dns-system -o yaml

# Check BIND9 logs
kubectl logs -n dns-system -l app=bindy,dns-role=primary

# Verify zone file
kubectl exec -n dns-system -it <pod-name> -- cat /var/lib/bind/zones/example.com.zone
```

### Record Not Appearing

```bash
# Check record status
kubectl get arecord www-a-record -n dns-system -o yaml

# Check zone record count
kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.recordCount}'
```
