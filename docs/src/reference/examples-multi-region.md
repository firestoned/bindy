# Multi-Region Setup Example

Geographic distribution for global DNS resilience and performance.

## Overview

This example demonstrates:
- Primary instances in multiple regions
- Secondary instances for redundancy
- Zone replication across regions
- Anycast for geographic load balancing
- Cross-region monitoring

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│                        Global DNS Infrastructure                    │
└────────────────────────────────────────────────────────────────────┘

  Region 1: us-east-1           Region 2: us-west-2         Region 3: eu-west-1
┌─────────────────────┐      ┌─────────────────────┐     ┌─────────────────────┐
│  Primary Instances  │      │ Secondary Instances │     │ Secondary Instances │
│                     │      │                     │     │                     │
│  ┌────┐  ┌────┐   │◄─────┤  ┌────┐  ┌────┐    │◄────┤  ┌────┐  ┌────┐    │
│  │Pod1│  │Pod2│   │ AXFR │  │Pod1│  │Pod2│    │AXFR │  │Pod1│  │Pod2│    │
│  └────┘  └────┘   │      │  └────┘  └────┘    │     │  └────┘  └────┘    │
│                     │      │                     │     │                     │
│  DNSSEC: Enabled    │      │  DNSSEC: Verify    │     │  DNSSEC: Verify    │
│  Replicas: 2        │      │  Replicas: 2        │     │  Replicas: 2        │
└─────────────────────┘      └─────────────────────┘     └─────────────────────┘
         │                            │                            │
         └────────────────────────────┴────────────────────────────┘
                                      │
                              Anycast IP: 192.0.2.1
                        (Routes to nearest region)
```

## Region 1: us-east-1 (Primary)

Save as `region-us-east-1.yaml`:

```yaml
---
# Namespace
apiVersion: v1
kind: Namespace
metadata:
  name: dns-system
  labels:
    region: us-east-1
    role: primary

---
# Primary Bind9Instance
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: primary-us-east-1
  namespace: dns-system
  labels:
    app: bindy
    dns-role: primary
    region: us-east-1
    environment: production
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.1.0.0/16"  # us-west-2 CIDR
      - "10.2.0.0/16"  # eu-west-1 CIDR
    dnssec:
      enabled: true
      validation: false
    listenOn:
      - "any"
    listenOnV6:
      - "any"

---
# PodDisruptionBudget
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: primary-dns-pdb
  namespace: dns-system
spec:
  minAvailable: 1
  selector:
    matchLabels:
      dns-role: primary
      region: us-east-1

---
# Primary DNSZone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-primary
  namespace: dns-system
spec:
  zoneName: "example.com"
  zoneType: "primary"
  instanceSelector:
    matchLabels:
      dns-role: primary
      region: us-east-1
  soaRecord:
    primaryNs: "ns1.us-east-1.example.com."
    adminEmail: "dns-admin@example.com"
    serial: 2024010101
    refresh: 900
    retry: 300
    expire: 604800
    negativeTtl: 300
  ttl: 300

---
# Nameserver Records
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ns1-us-east-1
  namespace: dns-system
spec:
  zone: "example-com-primary"
  name: "ns1.us-east-1"
  ipv4Address: "192.0.2.1"
  ttl: 86400

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ns2-us-west-2
  namespace: dns-system
spec:
  zone: "example-com-primary"
  name: "ns2.us-west-2"
  ipv4Address: "192.0.2.2"
  ttl: 86400

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ns3-eu-west-1
  namespace: dns-system
spec:
  zone: "example-com-primary"
  name: "ns3.eu-west-1"
  ipv4Address: "192.0.2.3"
  ttl: 86400

---
# Regional Web Servers
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-us-east-1
  namespace: dns-system
spec:
  zone: "example-com-primary"
  name: "www.us-east-1"
  ipv4Address: "192.0.2.10"
  ttl: 60

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-us-west-2
  namespace: dns-system
spec:
  zone: "example-com-primary"
  name: "www.us-west-2"
  ipv4Address: "192.0.2.20"
  ttl: 60

---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-eu-west-1
  namespace: dns-system
spec:
  zone: "example-com-primary"
  name: "www.eu-west-1"
  ipv4Address: "192.0.2.30"
  ttl: 60

---
# GeoDNS using SRV records for service discovery
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata:
  name: srv-web-us-east
  namespace: dns-system
spec:
  zone: "example-com-primary"
  name: "_http._tcp.us-east-1"
  priority: 10
  weight: 100
  port: 80
  target: "www.us-east-1.example.com."
  ttl: 300
```

## Region 2: us-west-2 (Secondary)

Save as `region-us-west-2.yaml`:

```yaml
---
# Namespace
apiVersion: v1
kind: Namespace
metadata:
  name: dns-system
  labels:
    region: us-west-2
    role: secondary

---
# Secondary Bind9Instance
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: secondary-us-west-2
  namespace: dns-system
  labels:
    app: bindy
    dns-role: secondary
    region: us-west-2
    environment: production
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
# PodDisruptionBudget
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: secondary-dns-pdb
  namespace: dns-system
spec:
  minAvailable: 1
  selector:
    matchLabels:
      dns-role: secondary
      region: us-west-2

---
# Secondary DNSZone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-secondary
  namespace: dns-system
spec:
  zoneName: "example.com"
  zoneType: "secondary"
  instanceSelector:
    matchLabels:
      dns-role: secondary
      region: us-west-2
  secondaryConfig:
    primaryServers:
      - "192.0.2.1"   # Primary in us-east-1
      - "192.0.2.2"
  ttl: 300
```

## Region 3: eu-west-1 (Secondary)

Save as `region-eu-west-1.yaml`:

```yaml
---
# Namespace
apiVersion: v1
kind: Namespace
metadata:
  name: dns-system
  labels:
    region: eu-west-1
    role: secondary

---
# Secondary Bind9Instance
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: secondary-eu-west-1
  namespace: dns-system
  labels:
    app: bindy
    dns-role: secondary
    region: eu-west-1
    environment: production
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
# PodDisruptionBudget
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: secondary-dns-pdb
  namespace: dns-system
spec:
  minAvailable: 1
  selector:
    matchLabels:
      dns-role: secondary
      region: eu-west-1

---
# Secondary DNSZone
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com-secondary
  namespace: dns-system
spec:
  zoneName: "example.com"
  zoneType: "secondary"
  instanceSelector:
    matchLabels:
      dns-role: secondary
      region: eu-west-1
  secondaryConfig:
    primaryServers:
      - "192.0.2.1"   # Primary in us-east-1
      - "192.0.2.2"
  ttl: 300
```

## Deployment

### 1. Deploy to Each Region

```bash
# us-east-1
kubectl apply -f region-us-east-1.yaml --context us-east-1

# us-west-2
kubectl apply -f region-us-west-2.yaml --context us-west-2

# eu-west-1
kubectl apply -f region-eu-west-1.yaml --context eu-west-1
```

### 2. Verify Replication

```bash
# Check zone transfer from primary
kubectl exec -n dns-system -it <primary-pod> -- \
  dig @localhost example.com AXFR

# Verify secondary received zone
kubectl exec -n dns-system -it <secondary-pod> -- \
  dig @localhost example.com SOA
```

### 3. Configure Anycast (Infrastructure Level)

This requires network infrastructure support:

```bash
# Example using MetalLB for on-premises
apiVersion: v1
kind: Service
metadata:
  name: dns-anycast
  namespace: dns-system
  annotations:
    metallb.universe.tf/address-pool: anycast-pool
spec:
  type: LoadBalancer
  loadBalancerIP: 192.0.2.1  # Same IP in all regions
  selector:
    app: bindy
  ports:
    - protocol: UDP
      port: 53
      targetPort: 53
```

## Cross-Region Monitoring

### Prometheus Federation

```yaml
# Global Prometheus Configuration
apiVersion: v1
kind: ConfigMap
metadata:
  name: prometheus-config
data:
  prometheus.yml: |
    global:
      scrape_interval: 30s
    
    scrape_configs:
      # us-east-1
      - job_name: 'dns-us-east-1'
        static_configs:
          - targets: ['prometheus.us-east-1.example.com:9090']
        metric_relabel_configs:
          - source_labels: [__name__]
            regex: 'dns_.*'
            action: keep
      
      # us-west-2
      - job_name: 'dns-us-west-2'
        static_configs:
          - targets: ['prometheus.us-west-2.example.com:9090']
        metric_relabel_configs:
          - source_labels: [__name__]
            regex: 'dns_.*'
            action: keep
      
      # eu-west-1
      - job_name: 'dns-eu-west-1'
        static_configs:
          - targets: ['prometheus.eu-west-1.example.com:9090']
        metric_relabel_configs:
          - source_labels: [__name__]
            regex: 'dns_.*'
            action: keep
```

### Health Checks

```bash
#!/bin/bash
# health-check-multi-region.sh

REGIONS=("us-east-1" "us-west-2" "eu-west-1")
QUERY="www.example.com"

for region in "${REGIONS[@]}"; do
  echo "Checking $region..."
  
  # Get DNS service IP
  DNS_IP=$(kubectl get svc -n dns-system --context $region \
    -o jsonpath='{.items[0].status.loadBalancer.ingress[0].ip}')
  
  # Test query
  if dig @$DNS_IP $QUERY +short > /dev/null; then
    echo "✓ $region: OK"
  else
    echo "✗ $region: FAILED"
  fi
done
```

## Disaster Recovery

### Regional Failover

```bash
# Promote secondary in us-west-2 to primary
kubectl patch bind9instance secondary-us-west-2 \
  -n dns-system --context us-west-2 \
  --type merge \
  --patch '{"metadata":{"labels":{"dns-role":"primary"}}}'

# Update zone to primary
kubectl patch dnszone example-com-secondary \
  -n dns-system --context us-west-2 \
  --type merge \
  --patch '{"spec":{"zoneType":"primary"}}'
```

### Backup Strategy

```bash
#!/bin/bash
# backup-all-regions.sh

REGIONS=("us-east-1" "us-west-2" "eu-west-1")
BACKUP_DIR="./multi-region-backups/$(date +%Y%m%d)"

mkdir -p "$BACKUP_DIR"

for region in "${REGIONS[@]}"; do
  echo "Backing up $region..."
  
  kubectl get dnszones,arecords,aaaarecords,cnamerecords,mxrecords,txtrecords \
    -n dns-system --context $region -o yaml \
    > "$BACKUP_DIR/$region.yaml"
done

echo "Backup completed: $BACKUP_DIR"
```

## Performance Testing

### Global Latency Test

```bash
#!/bin/bash
# test-global-latency.sh

REGIONS=(
  "us-east-1:192.0.2.1"
  "us-west-2:192.0.2.2"
  "eu-west-1:192.0.2.3"
)

for region_ip in "${REGIONS[@]}"; do
  region="${region_ip%%:*}"
  ip="${region_ip##*:}"
  
  echo "Testing $region ($ip)..."
  
  # Measure query time
  time dig @$ip www.example.com +short
done
```

### Load Distribution

```bash
# Using dnsperf across regions
for region in us-east-1 us-west-2 eu-west-1; do
  dnsperf -s $DNS_IP -d queries.txt -c 50 -l 30 -Q 1000 | \
    tee results-$region.txt
done
```

## Cost Optimization

### Regional Scaling

```yaml
# HPA for each region based on local load
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: dns-hpa-us-east-1
  namespace: dns-system
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: primary-us-east-1
  minReplicas: 2
  maxReplicas: 5
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
```

## Compliance and Data Residency

### Regional Data Isolation

```yaml
# EU-specific zone for GDPR compliance
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: eu-example-com
  namespace: dns-system
  labels:
    compliance: gdpr
spec:
  zoneName: "eu.example.com"
  zoneType: "primary"
  instanceSelector:
    matchLabels:
      region: eu-west-1
  soaRecord:
    primaryNs: "ns1.eu-west-1.example.com."
    adminEmail: "dpo@example.com"
    serial: 2024010101
    refresh: 900
    retry: 300
    expire: 604800
    negativeTtl: 300
```

## Related Documentation

- [Multi-Region Deployment Guide](../guide/multi-region.md)
- [Replication Strategies](../advanced/replication.md)
- [High Availability](../advanced/ha.md)
- [Zone Transfers](../advanced/zone-transfers.md)
