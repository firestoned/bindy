# Migration Guide: Single Controller to Two-Level Operator Architecture

This document explains the architectural changes made to bindy and how to migrate from the old single-controller architecture to the new two-level operator architecture.

## What Changed?

### Before (v0.x)

Bindy ran as a single cluster-level controller that:
- Watched `Bind9Instance` resources
- Watched all DNS zone and record resources across all namespaces
- Managed both BIND9 instances AND DNS records in the same process

**Problems with this approach:**
- Single point of failure for all DNS management
- No isolation between different BIND9 instances
- High resource usage as the controller watched all resources cluster-wide
- Difficult to scale horizontally

### After (v1.0+)

Bindy now uses a two-level operator architecture:

1. **bindy-operator** (Cluster-level)
   - Single instance per cluster
   - Watches `Bind9Instance` resources only
   - Creates and manages BIND9 Deployments, Services, and ConfigMaps
   - Deploys bindy-controller as a sidecar with each BIND9 pod

2. **bindy-controller** (Instance-level, sidecar)
   - One instance per BIND9 pod
   - Watches DNS zones and records in its namespace
   - Writes zone files to shared volume with BIND9 container
   - Updates BIND9 configuration

**Benefits:**
- Better isolation: Each BIND9 instance has its own controller
- Improved scalability: Controllers scale with BIND9 instances
- Better performance: Zone file updates happen locally in the pod
- Fault isolation: Controller failure only affects one BIND9 instance

## Architecture Comparison

### Old Architecture

```
┌──────────────────────────────────────────────┐
│          Bindy Controller (Single)           │
│  - Watches Bind9Instances                    │
│  - Watches ALL DNSZones                      │
│  - Watches ALL DNS Records                   │
│  - Manages BIND9 deployments                 │
│  - Updates zone files remotely               │
└──────────────────────────────────────────────┘
                    │
                    ▼
        ┌───────────────────────┐
        │   BIND9 Pod 1         │
        │   - Serves DNS        │
        └───────────────────────┘
        ┌───────────────────────┐
        │   BIND9 Pod 2         │
        │   - Serves DNS        │
        └───────────────────────┘
```

### New Architecture

```
┌──────────────────────────────────────────────┐
│        bindy-operator (Cluster-level)        │
│  - Watches Bind9Instances                    │
│  - Creates Deployments/Services/ConfigMaps   │
└──────────────────────────────────────────────┘
                    │ creates
                    ▼
        ┌───────────────────────────────┐
        │   BIND9 Pod 1                 │
        │  ┌──────────┬──────────────┐  │
        │  │  BIND9   │  bindy-      │  │
        │  │  Container│  controller  │  │
        │  │          │  - Watches   │  │
        │  │          │    zones     │  │
        │  │          │  - Updates   │  │
        │  │          │    files     │  │
        │  └──────────┴──────────────┘  │
        │       Shared /etc/bind/zones  │
        └───────────────────────────────┘
```

## Migration Steps

### 1. Backup Your Configuration

Before migrating, backup all your DNS resources:

```bash
# Backup Bind9Instances
kubectl get bind9instances -A -o yaml > bind9instances-backup.yaml

# Backup DNSZones
kubectl get dnszones -A -o yaml > dnszones-backup.yaml

# Backup DNS Records
kubectl get arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords -A -o yaml > records-backup.yaml
```

### 2. Uninstall Old Version

```bash
# Delete the old controller deployment
kubectl delete deployment bindy -n dns-system

# Delete old RBAC (optional - you can keep the ServiceAccount)
kubectl delete clusterrolebinding bindy-rolebinding
kubectl delete clusterrole bindy-role
```

### 3. Install New Version

#### Install New RBAC

```bash
# Install operator RBAC
kubectl apply -f deploy/rbac/operator-serviceaccount.yaml
kubectl apply -f deploy/rbac/operator-role.yaml
kubectl apply -f deploy/rbac/operator-rolebinding.yaml

# Install controller RBAC
kubectl apply -f deploy/rbac/controller-serviceaccount.yaml
kubectl apply -f deploy/rbac/controller-role.yaml
kubectl apply -f deploy/rbac/controller-rolebinding.yaml
```

#### Update CRDs (if needed)

```bash
# Update all CRDs to latest version
kubectl apply -k deploy/crds
```

#### Deploy the Operator

```bash
kubectl apply -f deploy/operator/deployment.yaml
```

### 4. Verify Migration

Check that the operator is running:

```bash
kubectl get pods -n dns-system -l app=bindy-operator
```

The operator will automatically:
1. Discover existing `Bind9Instance` resources
2. Create new Deployments with the sidecar architecture
3. Migrate zone file management to the sidecar controllers

Verify BIND9 pods have the sidecar:

```bash
kubectl get pods -n dns-system -l app=bind9 -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{range .spec.containers[*]}{.name}{" "}{end}{"\n"}{end}'
```

You should see both `bind9` and `bindy-controller` containers.

### 5. Verify DNS Resolution

Test that DNS still works:

```bash
# Port-forward to test
kubectl port-forward -n dns-system svc/your-bind9-instance 5353:53

# Test resolution
dig @localhost -p 5353 your-domain.com
```

## Breaking Changes

### Container Names

**Old:** Deployments had a single container named `controller`

**New:** Deployments have two containers:
- `bind9` - BIND9 DNS server
- `bindy-controller` - Zone/record management sidecar

If you have scripts or monitoring that references container names, update them:

```bash
# Old
kubectl logs deployment/my-dns -c controller

# New
kubectl logs deployment/my-dns -c bindy-controller
kubectl logs deployment/my-dns -c bind9
```

### RBAC Changes

**Old:** Single `bindy` ServiceAccount with cluster-wide permissions

**New:** Two ServiceAccounts:
- `bindy-operator` - For the operator (cluster-wide)
- `bindy-controller` - For the sidecar controllers

### Environment Variables

The bindy-controller sidecar requires these environment variables:
- `BIND9_ZONES_DIR` - Path to zones directory (default: `/etc/bind/zones`)
- `WATCH_NAMESPACE` - Namespace to watch (set automatically)
- `BIND9_INSTANCE_NAME` - Name of the BIND9 instance (set automatically)

These are set automatically by the operator when creating deployments.

## Rollback Procedure

If you need to rollback to the old architecture:

1. Scale down the operator:
   ```bash
   kubectl scale deployment bindy-operator -n dns-system --replicas=0
   ```

2. Restore the old controller:
   ```bash
   kubectl apply -f old-deployment-backup.yaml
   ```

3. Restore old RBAC if deleted:
   ```bash
   kubectl apply -f old-rbac-backup.yaml
   ```

## Troubleshooting

### Operator Not Creating Resources

Check operator logs:
```bash
kubectl logs -n dns-system -l app=bindy-operator
```

Common issues:
- Missing RBAC permissions
- CRDs not updated
- Invalid Bind9Instance spec

### Controller Sidecar Not Starting

Check controller logs:
```bash
kubectl logs -n dns-system <pod-name> -c bindy-controller
```

Common issues:
- Missing ServiceAccount
- Environment variables not set
- Volume mount issues

### DNS Queries Failing

1. Check BIND9 container logs:
   ```bash
   kubectl logs -n dns-system <pod-name> -c bind9
   ```

2. Verify zone files exist:
   ```bash
   kubectl exec -n dns-system <pod-name> -c bindy-controller -- ls -la /etc/bind/zones/
   ```

3. Check zone file contents:
   ```bash
   kubectl exec -n dns-system <pod-name> -c bindy-controller -- cat /etc/bind/zones/db.your-domain.com
   ```

## Support

For issues or questions:
- GitHub Issues: https://github.com/firestoned/bindy/issues
- Documentation: https://firestoned.github.io/bindy/

## See Also

- [ARCHITECTURE.md](ARCHITECTURE.md) - Detailed architecture documentation
- [deploy/DEPLOYMENT_GUIDE.md](deploy/DEPLOYMENT_GUIDE.md) - Fresh installation guide
