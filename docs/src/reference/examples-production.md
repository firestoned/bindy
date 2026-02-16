# Production Setup Example

Production-ready configuration with high availability, monitoring, and security.

## Overview

This example demonstrates:
- Primary/Secondary HA setup
- Multiple replicas with pod anti-affinity
- Resource limits and requests
- PodDisruptionBudgets
- DNSSEC enabled
- Monitoring and logging
- Production-grade security

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Production DNS                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│   Primary Instances (2 replicas)                            │
│   ┌──────────────┐  ┌──────────────┐                       │
│   │   Primary-1  │  │   Primary-2  │                       │
│   │  (us-east-1a)│  │  (us-east-1b)│                       │
│   └──────┬───────┘  └──────┬───────┘                       │
│          │                  │                               │
│          └──────────┬───────┘                               │
│                     │ Zone Transfer (AXFR/IXFR)            │
│          ┌──────────┴───────┐                               │
│          │                  │                               │
│   ┌──────▼───────┐  ┌──────▼───────┐                       │
│   │ Secondary-1  │  │ Secondary-2  │                       │
│   │ (us-west-2a) │  │ (us-west-2b) │                       │
│   └──────────────┘  └──────────────┘                       │
│                                                              │
│   Secondary Instances (2 replicas)                          │
└─────────────────────────────────────────────────────────────┘
```

## Complete Configuration

Save as `production-dns.yaml`:

```yaml
---
# Namespace
apiVersion: v1
kind: Namespace
metadata:
  name: dns-system-prod
  labels:
    environment: production

---
# ConfigMap for Operator Configuration
apiVersion: v1
kind: ConfigMap
metadata:
  name: bindy-config
  namespace: dns-system-prod
data:
  RUST_LOG: "info"
  RECONCILE_INTERVAL: "300"

---
# Primary Bind9Instance
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system-prod
  labels:
    app: bindy
    dns-role: primary
    environment: production
    component: dns-server
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.2.0/24"  # Secondary instance subnet
    dnssec:
      enabled: true
      validation: false
    listenOn:
      - "any"
    listenOnV6:
      - "any"

---
# Secondary Bind9Instance
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: secondary-dns
  namespace: dns-system-prod
  labels:
    app: bindy
    dns-role: secondary
    environment: production
    component: dns-server
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    dnssec:
      enabled: false
      validation: true
    listenOn:
      - "any"
    listenOnV6:
      - "any"

---
# PodDisruptionBudget for Primary
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: primary-dns-pdb
  namespace: dns-system-prod
spec:
  minAvailable: 1
  selector:
    matchLabels:
      app: bindy
      dns-role: primary

---
# PodDisruptionBudget for Secondary
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: secondary-dns-pdb
  namespace: dns-system-prod
spec:
  minAvailable: 1
  selector:
    matchLabels:
      app: bindy
      dns-role: secondary

---
# DNSZone - Primary
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-primary
  namespace: dns-system-prod
spec:
  zoneName: "example.com"
  zoneType: "primary"
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNs: "ns1.example.com."
    adminEmail: "dns-admin@example.com"
    serial: 2024010101
    refresh: 900       # 15 minutes - production refresh
    retry: 300         # 5 minutes
    expire: 604800     # 1 week
    negativeTtl: 300   # 5 minutes
  ttl: 300  # 5 minutes default TTL

---
# DNSZone - Secondary
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-secondary
  namespace: dns-system-prod
spec:
  zoneName: "example.com"
  zoneType: "secondary"
  instanceSelector:
    matchLabels:
      dns-role: secondary
  secondaryConfig:
    primaryServers:
      - "10.0.1.10"
      - "10.0.1.11"
  ttl: 300

---
# Production DNS Records
# Nameservers
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ns1-primary
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "ns1"
  ipv4Addresses:
    - "192.0.2.1"
  ttl: 86400  # 24 hours for NS records

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ns2-secondary
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "ns2"
  ipv4Addresses:
    - "192.0.2.2"
  ttl: 86400

---
# Load Balanced Web Servers (Round Robin)
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-lb-1
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "www"
  ipv4Addresses:
    - "192.0.2.10"
  ttl: 60  # 1 minute for load balanced IPs

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-lb-2
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "www"
  ipv4Addresses:
    - "192.0.2.11"
  ttl: 60

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-lb-3
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "www"
  ipv4Addresses:
    - "192.0.2.12"
  ttl: 60

---
# Dual Stack for www
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-v6-1
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "www"
  ipv6Addresses:
    - "2001:db8::10"
  ttl: 60

---
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: www-v6-2
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "www"
  ipv6Addresses:
    - "2001:db8::11"
  ttl: 60

---
# Mail Infrastructure
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: mail1
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "mail1"
  ipv4Addresses:
    - "192.0.2.20"
  ttl: 3600

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: mail2
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "mail2"
  ipv4Addresses:
    - "192.0.2.21"
  ttl: 3600

---
# MX Records - Primary and Backup
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-primary
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "@"
  priority: 10
  mailServer: "mail1.example.com."
  ttl: 3600

---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mx-backup
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "@"
  priority: 20
  mailServer: "mail2.example.com."
  ttl: 3600

---
# SPF Record
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: spf
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "@"
  text:
    - "v=spf1 mx ip4:192.0.2.20/32 ip4:192.0.2.21/32 -all"
  ttl: 3600

---
# DKIM Record
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: dkim
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "default._domainkey"
  text:
    - "v=DKIM1; k=rsa; p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC..."
  ttl: 3600

---
# DMARC Record
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: dmarc
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "_dmarc"
  text:
    - "v=DMARC1; p=quarantine; pct=100; rua=mailto:dmarc-reports@example.com; ruf=mailto:dmarc-forensics@example.com"
  ttl: 3600

---
# CAA Records
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-issue
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "@"
  flags: 0
  tag: "issue"
  value: "letsencrypt.org"
  ttl: 86400

---
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-issuewild
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "@"
  flags: 0
  tag: "issuewild"
  value: "letsencrypt.org"
  ttl: 86400

---
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: caa-iodef
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "@"
  flags: 0
  tag: "iodef"
  value: "mailto:security@example.com"
  ttl: 86400

---
# Service Records
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata:
  name: srv-sip-tcp
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "_sip._tcp"
  priority: 10
  weight: 60
  port: 5060
  target: "sip1.example.com."
  ttl: 3600

---
# CDN CNAME
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: cdn
  namespace: dns-system-prod
spec:
  zone: "example-com-primary"
  name: "cdn"
  target: "d123456.cloudfront.net."
  ttl: 3600
```

## Deployment

### 1. Prerequisites

```bash
# Create namespace
kubectl create namespace dns-system-prod

# Label nodes for DNS pods (optional but recommended)
kubectl label nodes node1 dns-zone=primary
kubectl label nodes node2 dns-zone=primary
kubectl label nodes node3 dns-zone=secondary
kubectl label nodes node4 dns-zone=secondary
```

### 2. Deploy

```bash
kubectl apply -f production-dns.yaml
```

### 3. Verify

```bash
# Check all instances
kubectl get bind9instances -n dns-system-prod
kubectl get dnszones -n dns-system-prod
kubectl get pods -n dns-system-prod -o wide

# Check PodDisruptionBudgets
kubectl get pdb -n dns-system-prod

# Verify HA distribution
kubectl get pods -n dns-system-prod -o custom-columns=\
NAME:.metadata.name,\
NODE:.spec.nodeName,\
ROLE:.metadata.labels.dns-role
```

## Monitoring

### Prometheus Metrics

```yaml
apiVersion: v1
kind: Service
metadata:
  name: bindy-metrics
  namespace: dns-system-prod
  labels:
    app: bindy
spec:
  ports:
    - name: metrics
      port: 9090
      targetPort: 9090
  selector:
    app: bindy
```

### ServiceMonitor (for Prometheus Operator)

```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: bindy-dns
  namespace: dns-system-prod
spec:
  selector:
    matchLabels:
      app: bindy
  endpoints:
    - port: metrics
      interval: 30s
```

## Backup and Disaster Recovery

### Backup Zones

```bash
#!/bin/bash
# backup-zones.sh

NAMESPACE="dns-system-prod"
BACKUP_DIR="./dns-backups/$(date +%Y%m%d)"

mkdir -p "$BACKUP_DIR"

# Backup all zones
kubectl get dnszones -n $NAMESPACE -o yaml > "$BACKUP_DIR/zones.yaml"

# Backup all records
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords \
  -n $NAMESPACE -o yaml > "$BACKUP_DIR/records.yaml"

echo "Backup completed: $BACKUP_DIR"
```

### Restore

```bash
kubectl apply -f dns-backups/20240115/zones.yaml
kubectl apply -f dns-backups/20240115/records.yaml
```

## Security Hardening

### Network Policies

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: dns-allow-queries
  namespace: dns-system-prod
spec:
  podSelector:
    matchLabels:
      app: bindy
  policyTypes:
    - Ingress
  ingress:
    - ports:
        - protocol: UDP
          port: 53
        - protocol: TCP
          port: 53
```

### Pod Security Standards

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: dns-system-prod
  labels:
    pod-security.kubernetes.io/enforce: restricted
    pod-security.kubernetes.io/audit: restricted
    pod-security.kubernetes.io/warn: restricted
```

## Performance Tuning

### Resource Limits

```yaml
spec:
  resources:
    requests:
      memory: "512Mi"
      cpu: "500m"
    limits:
      memory: "1Gi"
      cpu: "1000m"
```

### HorizontalPodAutoscaler

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: primary-dns-hpa
  namespace: dns-system-prod
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: primary-dns
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
```

## Testing

### Load Testing

```bash
# Using dnsperf
dnsperf -s <DNS_IP> -d queries.txt -c 100 -l 60

# queries.txt format:
# www.example.com A
# mail1.example.com A
# example.com MX
```

### Failover Testing

```bash
# Delete primary pod to test failover
kubectl delete pod -n dns-system-prod -l dns-role=primary --force

# Monitor DNS continues to serve
dig @<DNS_IP> www.example.com
```

## Related Documentation

- [High Availability Guide](../advanced/ha.md)
- [Monitoring Guide](../operations/monitoring.md)
- [Security Guide](../advanced/security.md)
