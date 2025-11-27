# Bind9 Kubernetes Cluster - Complete Deployment Package

## 📋 Package Contents

This repository contains a complete, production-ready Bind9 DNS deployment for an isolated k0s-based Kubernetes cluster with Linkerd integration for mothership communication.

### Core Manifests

| File | Purpose |
|------|---------|
| `namespace.yaml` | Bind9 namespace with Linkerd injection labels |
| `configmap.yaml` | Bind9 configuration (named.conf, rndc.conf) with zones |
| `rbac.yaml` | ServiceAccount, Role, and RoleBinding for security |
| `secret-rndc.yaml` | RNDC authentication key for remote management |
| `pvc.yaml` | PersistentVolumeClaims for cache and zone data |
| `deployment.yaml` | 3-replica Bind9 Deployment with high availability |
| `service-dns.yaml` | NodePort service exposing DNS on port 30053 |
| `service-rndc.yaml` | ClusterIP service for RNDC access (Linkerd-enabled) |
| `service-loadbalancer.yaml` | Optional LoadBalancer service for F5 integration |
| `networkpolicy.yaml` | Network policies for production security |
| `kustomization.yaml` | Kustomize configuration for environment management |

### Documentation

| File | Purpose |
|------|---------|
| `README.md` | Complete deployment guide and troubleshooting |
| `MOTHERSHIP_INTEGRATION.md` | Detailed guide for k0rdent mothership integration |
| `DEPLOYMENT.md` | This file - architecture and deployment overview |

### Scripts

| File | Purpose |
|------|---------|
| `deploy.sh` | Automated deployment script (dev/prod modes) |
| `verify.sh` | Verification and testing script |

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────┐
│           k0rdent Mothership                    │
│  (k0rdent cluster with Linkerd service mesh)   │
│                                                 │
│  ┌─────────────────────────────────────────┐  │
│  │  DNS Management Pod (Linkerd-injected)  │  │
│  │  - bind9-utils for RNDC commands       │  │
│  │  - Updates Bind9 via RNDC port 953    │  │
│  └─────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
                      │
        Linkerd mTLS Encrypted Tunnel
                      │
┌─────────────────────────────────────────────────┐
│    Isolated Bind9 DNS Cluster (k0s-based)      │
│                                                 │
│  ┌─────────────────────────────────────────┐  │
│  │  Bind9 Deployment (3 replicas)         │  │
│  │  ├─ DNS: UDP/TCP port 53               │  │
│  │ └─ RNDC: TCP port 953 (for updates)   │  │
│  └─────────────────────────────────────────┘  │
│                                                 │
│  Services:                                     │
│  ├─ bind9-dns (NodePort 30053) → DNS port 53 │
│  ├─ bind9-rndc (ClusterIP) → RNDC port 953  │
│  └─ bind9-loadbalancer (Optional for F5)     │
│                                                 │
│  Storage:                                      │
│  ├─ bind9-cache (5Gi) - DNS query cache      │
│  └─ bind9-zones (10Gi) - Zone files          │
└─────────────────────────────────────────────────┘
                      │
                F5 Load Balancer
                (Port 53 ↔ NodePort 30053)
```

## 🚀 Quick Start

### Prerequisites
- k0s Kubernetes cluster (1.21+)
- kubectl configured
- Linkerd installed on BOTH clusters
- Storage provisioner (local-path, ceph, etc.)

### Option 1: Automated Deployment

```bash
# Make script executable
chmod +x deploy.sh verify.sh

# Deploy to dev environment
./deploy.sh dev

# Or deploy to prod environment (includes NetworkPolicies)
./deploy.sh prod

# Verify deployment
./verify.sh
```

### Option 2: Manual Deployment with kubectl

```bash
# Deploy all resources
kubectl apply -f namespace.yaml
kubectl apply -f rbac.yaml
kubectl apply -f secret-rndc.yaml
kubectl apply -f configmap.yaml
kubectl apply -f pvc.yaml
kubectl apply -f deployment.yaml
kubectl apply -f service-dns.yaml
kubectl apply -f service-rndc.yaml

# Optional: Add network policies for production
kubectl apply -f networkpolicy.yaml

# Optional: Add F5 LoadBalancer
kubectl apply -f service-loadbalancer.yaml
```

### Option 3: Deployment with Kustomize

```bash
# Deploy using Kustomization
kubectl apply -k .

# Or with overlays for different environments
kubectl apply -k overlays/prod/
```

## ✅ Verification

```bash
# Check deployment status
kubectl get all -n bind9

# View logs
kubectl logs -n bind9 -l app=bind9 -f

# Test DNS resolution
kubectl exec -n bind9 -it $(kubectl get pods -n bind9 -o jsonpath='{.items[0].metadata.name}') -- \
  dig @127.0.0.1 localhost

# Port-forward for local testing
kubectl port-forward -n bind9 svc/bind9-dns 5353:53 &
dig @127.0.0.1 -p 5353 localhost
```

## 🔗 Mothership Integration

After deploying Bind9, follow the detailed integration guide:

```bash
# See MOTHERSHIP_INTEGRATION.md for step-by-step instructions
cat MOTHERSHIP_INTEGRATION.md

# Quick summary:
# 1. Create ExternalName service in mothership pointing to bind9-rndc
# 2. Deploy DNS manager pod with Linkerd injection enabled
# 3. Use RNDC commands to manage DNS zones
# 4. Optional: Create CronJob for automated DNS sync
```

## 🔒 Security Considerations

### RNDC Key Management
**⚠️ CRITICAL: Change the default RNDC key before production deployment!**

The default key `K8s+Bind9RndcKey==` is used in:
- `configmap.yaml` - named.conf controls section
- `secret-rndc.yaml` - RNDC authentication

Generate a new key:
```bash
# Using dnssec-keygen
dnssec-keygen -a HMAC-SHA256 -b 256 -n HOST rndc-key

# Or generate random base64
openssl rand -base64 32
```

### Network Security
- DNSSEC validation enabled by default
- Network policies restrict RNDC access (production)
- Linkerd mTLS encrypts mothership communication
- Pod security context: non-root user (UID 101)

## 📊 Resource Requirements

### Per Pod
- **CPU**: 250m request, 1000m limit
- **Memory**: 256Mi request, 512Mi limit
- **Storage**: 5Gi cache + 10Gi zones per PVC

### Total (3 replicas)
- **CPU**: 750m request, 3000m limit
- **Memory**: 768Mi request, 1536Mi limit
- **Storage**: 15Gi cache + 30Gi zones

## 🔄 Scaling

### Add Replicas
```bash
kubectl scale deployment/bind9 -n bind9 --replicas=5
```

### Zone Management
Upload zone files to PVC:
```bash
POD=$(kubectl get pods -n bind9 -o jsonpath='{.items[0].metadata.name}')
kubectl cp example.com.zone bind9/$POD:/etc/bind/zones/

# Reload zones via RNDC
kubectl exec -n bind9 $POD -- rndc reload example.com
```

## 🔧 Configuration Management

### Update Configuration
Edit `configmap.yaml` and apply:
```bash
kubectl apply -f configmap.yaml

# Restart pods to pick up changes
kubectl rollout restart deployment/bind9 -n bind9
```

### Add New Zone
Add to named.conf in `configmap.yaml`:
```yaml
zone "example.com" {
  type master;
  file "/etc/bind/zones/example.com";
  allow-update { 10.0.0.0/8; };
};
```

Then reload:
```bash
kubectl apply -f configmap.yaml
# Or via RNDC from mothership
rndc reload example.com
```

## 📈 Monitoring & Logging

### Prometheus Metrics
Bind9 pod annotations enable Prometheus scraping:
```yaml
prometheus.io/scrape: "true"
prometheus.io/port: "9119"
prometheus.io/path: "/metrics"
```

### Query Logging
Uncomment in configmap.yaml for detailed logging:
```yaml
querylog yes;
```

### View Logs
```bash
# All Bind9 pods
kubectl logs -n bind9 -l app=bind9 -f

# Specific pod
kubectl logs -n bind9 <pod-name> -f

# Previous logs (if crashed)
kubectl logs -n bind9 <pod-name> --previous
```

## 🛠️ Troubleshooting

### Common Issues

**Pods not starting**
```bash
kubectl describe pod -n bind9 -l app=bind9
kubectl logs -n bind9 -l app=bind9 --previous
```

**DNS queries failing**
```bash
kubectl exec -n bind9 <pod> -- dig @127.0.0.1 localhost
kubectl exec -n bind9 <pod> -- named-checkconf /etc/bind/named.conf
```

**RNDC connection issues**
```bash
kubectl exec -n bind9 <pod> -- rndc -s 127.0.0.1 status
kubectl get svc -n bind9 bind9-rndc
```

**Storage issues**
```bash
kubectl get pv,pvc -n bind9
kubectl describe pvc bind9-cache -n bind9
```

## 📚 Additional Resources

- [Bind9 Documentation](https://bind9.readthedocs.io/)
- [k0s Documentation](https://docs.k0sproject.io/)
- [Linkerd Service Mesh](https://linkerd.io/)
- [Kubernetes Storage](https://kubernetes.io/docs/concepts/storage/)
- [RNDC Manual](https://bind9.readthedocs.io/en/latest/reference/man_rndc.html)

## 📞 Support

For issues specific to:
- **Bind9**: Check official Bind9 documentation
- **Kubernetes**: Check k0s or general Kubernetes docs
- **Linkerd**: Check Linkerd documentation
- **Deployment**: See README.md and MOTHERSHIP_INTEGRATION.md

## 📝 Version Information

- **Bind9 Version**: 9.18
- **Kubernetes Version**: 1.21+
- **Linkerd Version**: 2.13+
- **k0s Version**: Compatible with all versions

## 🎯 Next Steps

1. Review and customize `configmap.yaml` with your zones
2. Change RNDC key in both `configmap.yaml` and `secret-rndc.yaml`
3. Deploy using `./deploy.sh dev` (or your preferred method)
4. Verify with `./verify.sh`
5. Integrate with mothership using MOTHERSHIP_INTEGRATION.md
6. Configure F5 LoadBalancer for external access
7. Set up monitoring and alerts

---

**Created**: November 25, 2025
**Status**: Production-Ready
**Maintained**: GitHub Copilot Assistant
