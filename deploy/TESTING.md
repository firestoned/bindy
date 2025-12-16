# Testing Guide for Bindy Controller

This guide provides comprehensive instructions for testing the Bindy controller locally using Kind (Kubernetes in Docker).

## Prerequisites

Before you begin, ensure you have the following installed:

- **Docker** (version 20.10+)
  ```bash
  docker --version
  ```

- **Kind** (version 0.20.0+)
  ```bash
  kind --version
  # If not installed:
  brew install kind  # macOS
  # or visit https://kind.sigs.k8s.io/docs/user/quick-start/#installation
  ```

- **kubectl** (version 1.24+)
  ```bash
  kubectl version --client
  ```

- **Rust** (version 1.82+)
  ```bash
  rustc --version
  ```

## Quick Start

The fastest way to get started is using the provided scripts:

```bash
# 1. Deploy everything (creates cluster, builds image, deploys controller)
./deploy/kind-deploy.sh

# 2. Run integration tests
./deploy/kind-test.sh

# 3. Clean up when done
./deploy/kind-cleanup.sh
```

## Using Make

Alternatively, use the Makefile targets:

```bash
# Deploy to Kind
make kind-deploy

# Run tests
make kind-test

# Watch logs
make kind-logs

# Clean up
make kind-cleanup
```

## Manual Testing Workflow

### Step 1: Create the Cluster

```bash
kind create cluster --config deploy/kind-config.yaml --name bindy-test
```

This creates a 3-node cluster (1 control-plane, 2 workers) with port mappings for DNS testing.

### Step 2: Build the Controller

```bash
# Build the Rust binary
cargo build --release

# Build the Docker image
docker build -t bindy:latest .

# Load image into Kind
kind load docker-image bindy:latest --name bindy-test
```

### Step 3: Deploy CRDs

```bash
kubectl apply -k deploy/crds/
```

Verify CRDs are installed:

```bash
kubectl get crds | grep dns.firestoned.io
```

Expected output:
```
aaaarecords.dns.firestoned.io
arecords.dns.firestoned.io
bind9instances.dns.firestoned.io
caarecords.dns.firestoned.io
cnamerecords.dns.firestoned.io
dnszones.dns.firestoned.io
mxrecords.dns.firestoned.io
nsrecords.dns.firestoned.io
srvrecords.dns.firestoned.io
txtrecords.dns.firestoned.io
```

### Step 4: Deploy RBAC and Controller

```bash
# Create namespace
kubectl create namespace dns-system

# Deploy RBAC
kubectl apply -f deploy/rbac/

# Deploy controller
kubectl apply -f deploy/controller/deployment.yaml
```

### Step 5: Verify Deployment

```bash
# Check controller pod
kubectl get pods -n dns-system

# Check logs
kubectl logs -n dns-system -l app=bindy -f
```

Expected log output:
```
INFO bindy_controller: Starting Bindy DNS Controller
INFO bindy_controller: Watching for DNSZone resources
INFO bindy_controller: Watching for ARecord resources
...
```

## Testing Scenarios

### Test 1: Create a Bind9Instance

Create a test Bind9Instance:

```bash
kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: test-primary
  namespace: dns-system
  labels:
    role: primary
    environment: test
spec:
  replicas: 1
  version: "9.18"
EOF
```

Verify:
```bash
kubectl get bind9instances -n dns-system
kubectl describe bind9instance test-primary -n dns-system
```

### Test 2: Create a DNS Zone

Create a DNS zone that targets the Bind9Instance:

```bash
kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  type: primary
  instanceSelector:
    matchLabels:
      role: primary
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin@example.com
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
EOF
```

Check the zone status:
```bash
kubectl get dnszones -n dns-system -o wide
kubectl describe dnszone example-com -n dns-system
```

Look for status conditions in the controller logs:
```bash
kubectl logs -n dns-system -l app=bindy | grep "example-com"
```

### Test 3: Add DNS Records

#### A Record

```bash
kubectl apply -f - <<EOF
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
EOF
```

#### TXT Record

```bash
kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: TXTRecord
metadata:
  name: spf-example
  namespace: dns-system
spec:
  zone: example-com
  name: "@"
  text:
    - "v=spf1 include:_spf.example.com ~all"
  ttl: 3600
EOF
```

#### CNAME Record

```bash
kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: blog-example
  namespace: dns-system
spec:
  zone: example-com
  name: blog
  target: www.example.com.
  ttl: 300
EOF
```

Verify all records:
```bash
kubectl get arecords,txtrecords,cnamerecords -n dns-system
```

### Test 4: Label Selector Testing

Create a secondary instance with different labels:

```bash
kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: test-secondary
  namespace: dns-system
  labels:
    role: secondary
    environment: test
EOF
```

Create a zone that matches both instances:

```bash
kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: internal-local
  namespace: dns-system
spec:
  zoneName: internal.local
  type: primary
  instanceSelector:
    matchLabels:
      environment: test
  soaRecord:
    primaryNS: ns1.internal.local.
    adminEmail: admin@internal.local
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
EOF
```

The zone should be reconciled for both instances (check status).

### Test 5: Update and Delete

Update a record:
```bash
kubectl patch arecord www-example -n dns-system --type merge -p '{"spec":{"ipv4Address":"192.0.2.100"}}'
```

Delete a record:
```bash
kubectl delete arecord www-example -n dns-system
```

Watch the controller handle the changes:
```bash
kubectl logs -n dns-system -l app=bindy -f
```

## Debugging

### Check Controller Status

```bash
# Pod status
kubectl get pods -n dns-system -o wide

# Detailed pod info
kubectl describe pod -n dns-system -l app=bindy

# Resource usage
kubectl top pod -n dns-system -l app=bindy
```

### View Logs

```bash
# Follow logs
kubectl logs -n dns-system -l app=bindy -f

# Last 100 lines
kubectl logs -n dns-system -l app=bindy --tail=100

# Search for errors
kubectl logs -n dns-system -l app=bindy | grep -i error

# Previous container logs (if crashed)
kubectl logs -n dns-system -l app=bindy --previous
```

### Check Events

```bash
# All events in namespace
kubectl get events -n dns-system --sort-by='.lastTimestamp'

# Events for a specific resource
kubectl describe dnszone example-com -n dns-system
```

### Interactive Debugging

```bash
# Shell into controller pod
kubectl exec -it -n dns-system <pod-name> -- /bin/sh

# Run debug pod
kubectl run -it --rm debug --image=nicolaka/netshoot --restart=Never -- /bin/bash
```

### Common Issues

**Controller not starting:**
- Check image is loaded: `docker exec -it bindy-test-control-plane crictl images | grep bindy`
- Check RBAC permissions: `kubectl auth can-i list dnszones --as=system:serviceaccount:dns-system:bindy`

**Resources not reconciling:**
- Check controller logs for errors
- Verify CRDs are installed: `kubectl get crd`
- Check resource status: `kubectl describe <resource-type> <name> -n dns-system`

**Port conflicts:**
- Ensure ports 30053 and 30953 are available on your host

## Performance Testing

### Load Testing

Create many resources at once:

```bash
for i in {1..100}; do
  kubectl apply -f - <<EOF
apiVersion: dns.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: test-${i}
  namespace: dns-system
spec:
  zone: example-com
  name: test-${i}
  ipv4Address: "192.0.2.${i}"
  ttl: 300
EOF
done
```

Monitor controller performance:
```bash
kubectl top pod -n dns-system -l app=bindy
```

### Reconciliation Speed

Time how long it takes to reconcile:

```bash
time kubectl apply -f examples/dns-zone.yaml
kubectl logs -n dns-system -l app=bindy | grep "reconciled"
```

## Cleanup

### Remove Test Resources

```bash
kubectl delete arecords,txtrecords,cnamerecords,dnszones,bind9instances --all -n dns-system
```

### Remove Controller

```bash
kubectl delete -f deploy/controller/deployment.yaml
kubectl delete -f deploy/rbac/
kubectl delete -f deploy/crds/dns-crds.yaml
kubectl delete namespace dns-system
```

### Delete Cluster

```bash
kind delete cluster --name bindy-test
# or
./deploy/kind-cleanup.sh
```

## CI/CD Integration

Example GitHub Actions workflow snippet:

```yaml
- name: Setup Kind
  uses: helm/kind-action@v1.8.0
  with:
    cluster_name: bindy-test
    config: deploy/kind-config.yaml

- name: Build and Deploy
  run: |
    docker build -t bindy:test .
    kind load docker-image bindy:test --name bindy-test
    kubectl apply -k deploy/crds/
    kubectl apply -f deploy/rbac/
    kubectl apply -f deploy/controller/

- name: Run Tests
  run: ./deploy/kind-test.sh
```

## Next Steps

- Review [deploy/README.md](README.md) for production deployment
- Check [../README.md](../README.md) for full documentation
- Explore [../examples/](../examples/) for more examples
- Read [../ARCHITECTURE.md](../ARCHITECTURE.md) for system design

## Support

For issues or questions:
- GitHub Issues: https://github.com/firestoned/bindy/issues
- Project Documentation: [README.md](../README.md)
