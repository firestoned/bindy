# Bind9 Kubernetes Cluster - Deployment Guide

## Architecture Overview

This deployment creates a **dedicated, isolated Bind9 DNS cluster** that:
- Exposes DNS on **NodePort 30053** (UDP/TCP)
- Provides RNDC access on port **953** via Linkerd for mothership communication
- Integrates with **F5 Load Balancer** for external port 53 exposure
- Uses 3 replicas with anti-affinity for high availability
- Persists configuration and zone data using PVCs

## Prerequisites

1. **k0s Kubernetes cluster** with:
   - Storage provisioner (local-path, ceph, etc.)
   - Linkerd service mesh (for secure mothership communication)
   - Optional: F5 iControl REST API access (for LB integration)

2. **kubectl** configured to access the cluster

## Deployment Steps

### 1. Create Namespace and ConfigMap
```bash
kubectl apply -f namespace.yaml
kubectl apply -f configmap.yaml
```

### 2. Create RBAC Resources
```bash
kubectl apply -f rbac.yaml
```

### 3. Create Secrets
```bash
kubectl apply -f secret-rndc.yaml
```

### 4. Create Storage
```bash
kubectl apply -f pvc.yaml
```

Wait for PVCs to be bound:
```bash
kubectl get pvc -n bind9 -w
```

### 5. Deploy Bind9
```bash
kubectl apply -f deployment.yaml
```

Monitor rollout:
```bash
kubectl rollout status deployment/bind9 -n bind9
kubectl get pods -n bind9 -o wide
```

### 6. Create Services

#### Option A: NodePort Only (for testing)
```bash
kubectl apply -f service-dns.yaml
kubectl apply -f service-rndc.yaml
```

#### Option B: With LoadBalancer (for production F5 integration)
```bash
kubectl apply -f service-loadbalancer.yaml
```

### 7. Optional: Network Policies
```bash
kubectl apply -f networkpolicy.yaml
```

## Verification

### Check Deployment Status
```bash
kubectl get all -n bind9
```

### Test DNS Resolution
```bash
# Port-forward to test locally
kubectl port-forward -n bind9 svc/bind9-dns 5353:53 &

# Test with dig
dig @127.0.0.1 -p 5353 localhost

# Test RNDC
kubectl port-forward -n bind9 svc/bind9-rndc 9953:953 &
dig @127.0.0.1 -p 5353 localhost
```

### View Logs
```bash
kubectl logs -n bind9 -l app=bind9 -f
```

## Configuration Management

### Update Bind9 Configuration
Edit `configmap.yaml` and apply changes:
```bash
kubectl apply -f configmap.yaml
```

Optionally restart pods to pick up changes:
```bash
kubectl rollout restart deployment/bind9 -n bind9
```

### Update RNDC Key
Edit `secret-rndc.yaml` and apply:
```bash
kubectl apply -f secret-rndc.yaml
kubectl rollout restart deployment/bind9 -n bind9
```

## Mothership Integration via Linkerd

### From k0rdent Mothership:

1. **Create a service in mothership to reach bind9-rndc:**
```bash
kubectl --context=mothership create -f - <<EOF
apiVersion: v1
kind: Service
metadata:
  name: bind9-dns-external
  namespace: default
spec:
  type: ExternalName
  externalName: bind9-rndc.bind9.svc.cluster.local
  ports:
  - port: 953
    targetPort: 953
    protocol: TCP
EOF
```

2. **Use Linkerd to create a mTLS tunnel** (handles automatic certificate rotation):
```bash
# Linkerd will automatically inject sidecars and encrypt traffic
# Deploy your DNS update client in the mothership with:
# labels:
#   linkerd.io/inject: enabled
```

3. **Connect to RNDC from mothership:**
```bash
# From any pod in mothership with Linkerd injection:
rndc -s bind9-dns-external -p 953 -k /etc/bind/rndc.key status
```

## F5 Load Balancer Integration

### Option 1: Manual F5 Configuration
Create a Virtual Server on your F5 that:
- **Frontend**: 0.0.0.0:53 (UDP/TCP)
- **Backend Pool**: Kubernetes cluster nodes on port 30053
- **Health Monitor**: TCP port 30053

### Option 2: F5 CIS (Controller Ingress Services)
If using F5 CIS controller in your cluster, the LoadBalancer service will auto-create the VS.

### Node Firewall Rules
Ensure your F5 can reach k0s cluster nodes on port 30053:
```bash
# Example UFW rule (adjust per your firewall)
ufw allow from <F5_IP> to any port 30053
```

## Troubleshooting

### Pods not starting
```bash
kubectl describe pod -n bind9 -l app=bind9
kubectl logs -n bind9 -l app=bind9 --previous
```

### DNS queries failing
```bash
# Check if service is accessible
kubectl get svc -n bind9
kubectl exec -n bind9 -it <pod-name> -- dig @127.0.0.1 localhost
```

### RNDC connection issues
```bash
# Test RNDC inside pod
kubectl exec -n bind9 -it <pod-name> -- rndc -s 127.0.0.1 -p 953 status

# Check linkerd injection
kubectl get pods -n bind9 -o json | grep linkerd
```

### Storage issues
```bash
kubectl get pv,pvc -n bind9
kubectl describe pvc bind9-cache -n bind9
```

## Scaling

### Add more replicas
```bash
kubectl scale deployment/bind9 -n bind9 --replicas=5
```

### Zone file management
Upload zone files to the bind9-zones PVC:
```bash
kubectl cp <local-zone-file> bind9/<pod-name>:/etc/bind/zones/
```

## Security Considerations

1. **RNDC Key**: Change `K8s+Bind9RndcKey==` in `secret-rndc.yaml` and `configmap.yaml`
2. **Allow-recursion**: Update ACLs in `named.conf` to match your network
3. **DNSSEC**: Enabled by default in this config
4. **Linkerd mTLS**: Automatically encrypts mothership-to-RNDC traffic

## Cleanup

```bash
kubectl delete namespace bind9
```

## Additional Resources

- [Bind9 Documentation](https://bind9.readthedocs.io/)
- [k0s Documentation](https://docs.k0sproject.io/)
- [Linkerd Service Mesh](https://linkerd.io/)
- [RNDC Manual](https://bind9.readthedocs.io/en/latest/reference/man_rndc.html)
