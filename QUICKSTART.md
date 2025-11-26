# Quick Reference Guide

## Installation

```bash
# Install CRDs
kubectl apply -f deploy/crds/dns-crds.yaml

# Create namespace
kubectl create namespace dns-system

# Install RBAC
kubectl apply -f deploy/rbac/

# Deploy controller
kubectl apply -f deploy/operator/deployment.yaml
```

## Label Bind9Instance

```bash
kubectl label bind9instance primary-dns \
  dns-role=primary \
  environment=production \
  -n dns-system
```

## Create Zone

```yaml
apiVersion: dns.example.com/v1alpha1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  instanceSelector:
    matchLabels:
      dns-role: primary
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
```

## Add Records

### A Record
```yaml
apiVersion: dns.example.com/v1alpha1
kind: ARecord
metadata:
  name: www
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300
```

### CNAME Record
```yaml
apiVersion: dns.example.com/v1alpha1
kind: CNAMERecord
metadata:
  name: blog
spec:
  zone: example-com
  name: blog
  target: www.example.com.
  ttl: 300
```

### TXT Record
```yaml
apiVersion: dns.example.com/v1alpha1
kind: TXTRecord
metadata:
  name: spf
spec:
  zone: example-com
  name: "@"
  text:
    - "v=spf1 include:_spf.example.com ~all"
  ttl: 3600
```

### MX Record
```yaml
apiVersion: dns.example.com/v1alpha1
kind: MXRecord
metadata:
  name: mail
spec:
  zone: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
  ttl: 3600
```

## Useful Commands

```bash
# Check CRDs
kubectl get crd | grep dns.example.com

# List zones
kubectl get dnszones -n dns-system -o wide

# Describe zone
kubectl describe dnszone example-com -n dns-system

# List records
kubectl get arecords -n dns-system -o wide
kubectl get txtrecords -n dns-system -o wide
kubectl get cnamerecords -n dns-system -o wide

# Check controller logs
kubectl logs -n dns-system -l app=bind9-controller -f

# Check zone file
kubectl exec -it <bind9-pod> -n dns-system -- cat /etc/bind/zones/db.example.com

# List all zones
kubectl exec -it <bind9-pod> -n dns-system -- ls /etc/bind/zones/

# Test DNS
kubectl exec -it <bind9-pod> -n dns-system -- nslookup www.example.com localhost

# Validate zone
kubectl exec -it <bind9-pod> -n dns-system -- named-checkzone example.com /etc/bind/zones/db.example.com

# Validate BIND config
kubectl exec -it <bind9-pod> -n dns-system -- named-checkconf
```

## Label Selector Examples

### Simple label matching
```yaml
instanceSelector:
  matchLabels:
    dns-role: primary
```

### Multiple labels (AND)
```yaml
instanceSelector:
  matchLabels:
    dns-role: primary
    environment: production
```

### Complex expressions
```yaml
instanceSelector:
  matchExpressions:
    - key: dns-role
      operator: In
      values:
        - primary
        - secondary
    - key: environment
      operator: In
      values:
        - production
        - staging
```

## Troubleshooting

### Zone not created
```bash
# Check if label selector matches any instances
kubectl get bind9instance --show-labels -n dns-system

# Check controller logs
kubectl logs -n dns-system -l app=bind9-controller | grep example-com
```

### Records not appearing
```bash
# Verify zone exists
kubectl get dnszones -n dns-system | grep example-com

# Check record status
kubectl describe arecord <record-name> -n dns-system

# Check controller logs
kubectl logs -n dns-system -l app=bind9-controller | grep ARecord
```

### DNS queries failing
```bash
# Validate zone file
kubectl exec -it <bind9-pod> -n dns-system -- named-checkzone example.com /etc/bind/zones/db.example.com

# Check BIND9 logs
kubectl logs <bind9-pod> -n dns-system | grep named

# Test with dig
kubectl exec -it <bind9-pod> -n dns-system -- dig www.example.com @localhost
```

## Build & Deploy

### Build controller
```bash
cd controller
cargo build --release
```

### Build Docker image
```bash
docker build -t bind9-controller:latest .
docker push <registry>/bind9-controller:latest
```

### Update deployment
```bash
kubectl set image deployment/bind9-controller \
  bind9-controller=<registry>/bind9-controller:v1.0.0 \
  -n dns-system
```

## Development

### Run locally
```bash
cd controller
export KUBECONFIG=~/.kube/config
cargo run
```

### Run tests
```bash
cd controller
cargo test
```

### Check formatting
```bash
cargo fmt --check
```

### Check for issues
```bash
cargo clippy
```

## Resource Definitions

All resources follow this pattern:

```yaml
apiVersion: dns.example.com/v1alpha1
kind: <Type>
metadata:
  name: <name>
  namespace: <namespace>
spec:
  # type-specific fields
status:
  conditions:
    - type: Ready
      status: "True"
      reason: <reason>
      message: <message>
      lastTransitionTime: <timestamp>
  observedGeneration: <int>
  # type-specific status fields
```

## Performance Tips

1. **Use labels effectively** - Avoid overly broad label selectors
2. **Batch operations** - Create multiple records in one kubectl apply
3. **Monitor resource usage** - Check controller logs for warnings
4. **Regular backups** - Backup zone files regularly
5. **Version control** - Keep manifests in Git

## Contact & Support

- Repository: https://github.com/firestoned/bindy
- Issues: https://github.com/firestoned/bindy/issues
- Documentation: See controller/README.md
- Migration Guide: See MIGRATION.md
