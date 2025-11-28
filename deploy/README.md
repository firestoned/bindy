# Deployment Guide

This directory contains all the necessary files to deploy the Bindy operator to a Kubernetes cluster.

## Directory Structure

```
deploy/
├── crds/
│   ├── aaaarecords.crd.yaml       # AAAA record CRD
│   ├── arecords.crd.yaml          # A record CRD
│   ├── bind9clusters.crd.yaml     # Bind9Cluster CRD (logical DNS cluster)
│   ├── bind9instances.crd.yaml    # Bind9Instance CRD (physical DNS server)
│   ├── caarecords.crd.yaml        # CAA record CRD
│   ├── cnamerecords.crd.yaml      # CNAME record CRD
│   ├── dnszones.crd.yaml          # DNSZone CRD
│   ├── mxrecords.crd.yaml         # MX record CRD
│   ├── nsrecords.crd.yaml         # NS record CRD
│   ├── srvrecords.crd.yaml        # SRV record CRD
│   ├── txtrecords.crd.yaml        # TXT record CRD
│   └── kustomization.yaml         # Kustomize config for CRDs
├── operator/
│   └── deployment.yaml            # Bindy operator deployment
├── rbac/
│   ├── serviceaccount.yaml        # Service account for bindy operator
│   ├── role.yaml                  # RBAC ClusterRole for bindy
│   └── rolebinding.yaml           # RBAC ClusterRoleBinding for bindy
├── kind-config.yaml               # Kind cluster configuration
├── kind-deploy.sh                 # Automated Kind deployment script
├── kind-cleanup.sh                # Kind cluster cleanup script
├── kind-test.sh                   # Integration test script
├── README.md                      # This file
└── TESTING.md                     # Testing documentation
```

## Quick Start with Kind

For local testing using [Kind](https://kind.sigs.k8s.io/):

### Prerequisites

- [Docker](https://docs.docker.com/get-docker/)
- [Kind](https://kind.sigs.k8s.io/docs/user/quick-start/#installation)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)

### Deploy

```bash
./deploy/kind-deploy.sh
```

This script will:
1. Create a Kind cluster with the right configuration
2. Install the CRDs
3. Create the namespace and RBAC
4. Build the Docker image
5. Load the image into Kind
6. Deploy the operator

### Test

After deployment, test with the examples:

```bash
# Create a Bind9 cluster
kubectl apply -f examples/bind9-cluster.yaml

# Create a Bind9 instance
kubectl apply -f examples/bind9-instance.yaml

# Create a DNS zone
kubectl apply -f examples/dns-zone.yaml

# Add DNS records
kubectl apply -f examples/dns-records.yaml

# Watch the operator logs
kubectl logs -n dns-system -l app=bindy -f
```

### Cleanup

```bash
./deploy/kind-cleanup.sh
```

## Manual Deployment

For production clusters or manual deployment:

### 1. Install CRDs

Use kustomize to apply the split CRD manifests:

```bash
kubectl apply -k deploy/crds
```

### 2. Create Namespace

```bash
kubectl create namespace dns-system
```

### 3. Install RBAC

```bash
kubectl apply -f deploy/rbac/
```

### 4. Build and Push Image

```bash
# Build the image
docker build -t <your-registry>/bindy:latest .

# Push to your registry
docker push <your-registry>/bindy:latest
```

### 5. Update Deployment

Edit `deploy/operator/deployment.yaml` to use your image:

```yaml
spec:
  template:
    spec:
      containers:
      - name: bindy
        image: <your-registry>/bindy:latest
```

### 6. Deploy Operator

```bash
kubectl apply -f deploy/operator/deployment.yaml
```

### 7. Verify Deployment

```bash
kubectl get pods -n dns-system
kubectl logs -n dns-system -l app=bindy
```

## Configuration

### Environment Variables

The operator supports these environment variables (set in `deployment.yaml`):

- `RUST_LOG` - Log level (default: `info`, options: `trace`, `debug`, `info`, `warn`, `error`)
- `POD_NAMESPACE` - Namespace where the operator is running (auto-populated from pod metadata)

### Resource Limits

Default resource limits in `deployment.yaml`:

```yaml
resources:
  requests:
    memory: "128Mi"
    cpu: "100m"
  limits:
    memory: "512Mi"
    cpu: "500m"
```

Adjust based on your needs.

## Troubleshooting

### Operator Not Starting

```bash
# Check pod status
kubectl get pods -n dns-system

# Check pod events
kubectl describe pod -n dns-system -l app=bindy

# Check logs
kubectl logs -n dns-system -l app=bindy --previous
```

### CRD Issues

```bash
# Verify CRDs are installed
kubectl get crd | grep dns.firestoned.io

# Check CRD details
kubectl describe crd dnszones.dns.firestoned.io
```

### RBAC Issues

```bash
# Check service account
kubectl get sa -n dns-system

# Check role binding
kubectl describe clusterrolebinding bindy-rolebinding

# Check permissions
kubectl auth can-i list dnszones --as=system:serviceaccount:dns-system:bindy
kubectl auth can-i list bind9instances --as=system:serviceaccount:dns-system:bindy
```

### Resource Not Reconciling

```bash
# Check resource status
kubectl get dnszones -n dns-system
kubectl describe dnszone <name> -n dns-system

# Check operator logs for errors
kubectl logs -n dns-system -l app=bindy | grep ERROR
```

## Upgrading

### Upgrade CRDs

```bash
kubectl apply -k deploy/crds
```

### Upgrade Operator

```bash
# Build new image
docker build -t <your-registry>/bindy:v1.1.0 .
docker push <your-registry>/bindy:v1.1.0

# Update deployment
kubectl set image deployment/bindy \
  bindy=<your-registry>/bindy:v1.1.0 \
  -n dns-system

# Watch rollout
kubectl rollout status deployment/bindy -n dns-system
```

## Uninstall

```bash
# Delete operator
kubectl delete -f deploy/operator/deployment.yaml

# Delete RBAC
kubectl delete -f deploy/rbac/

# Delete CRDs (this will delete all custom resources!)
kubectl delete -k deploy/crds

# Delete namespace
kubectl delete namespace dns-system
```

## Security Considerations

1. **Service Account** - The operator runs with minimal RBAC permissions
2. **Non-root** - Container runs as non-root user (UID 65534)
3. **Read-only Root** - Root filesystem is read-only
4. **Network Policies** - Consider adding network policies to restrict operator access
5. **Image Scanning** - Scan images for vulnerabilities before deployment
6. **Secrets Management** - TSIG keys and sensitive data should use Kubernetes Secrets

## Production Checklist

- [ ] Use a specific version tag (not `latest`)
- [ ] Configure appropriate resource limits
- [ ] Set up monitoring and alerting
- [ ] Configure log aggregation
- [ ] Test backup and restore procedures
- [ ] Document disaster recovery procedures
- [ ] Set up multiple replicas for HA (when supported)
- [ ] Configure Pod Disruption Budgets
- [ ] Enable network policies
- [ ] Implement image scanning in CI/CD

## Additional Resources

- [Kind Documentation](https://kind.sigs.k8s.io/)
- [Kubernetes Documentation](https://kubernetes.io/docs/)
- [Rust Kubernetes Operator Guide](https://docs.rs/kube/latest/kube/)
- [Main Project README](../README.md)
