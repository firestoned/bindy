# Quick Start

Get Bindy running in 5 minutes with this quick start guide.

## Step 1: Install Storage Provisioner (Optional)

For persistent zone data storage, install a storage provisioner. For Kind clusters or local development:

```bash
# Install local-path provisioner
kubectl apply -f https://raw.githubusercontent.com/rancher/local-path-provisioner/v0.0.28/deploy/local-path-storage.yaml

# Wait for provisioner to be ready
kubectl wait --for=condition=available --timeout=60s \
  deployment/local-path-provisioner -n local-path-storage

# Set as default StorageClass (or create one if it doesn't exist)
if kubectl get storageclass local-path &>/dev/null; then
  kubectl patch storageclass local-path -p '{"metadata": {"annotations":{"storageclass.kubernetes.io/is-default-class":"true"}}}'
else
  # Create default StorageClass if local-path wasn't created
  cat <<EOF | kubectl apply -f -
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: default
  annotations:
    storageclass.kubernetes.io/is-default-class: "true"
provisioner: rancher.io/local-path
volumeBindingMode: WaitForFirstConsumer
reclaimPolicy: Delete
EOF
fi

# Verify StorageClass is available
kubectl get storageclass
```

> **Note**: For production clusters, use your cloud provider's StorageClass (AWS EBS, GCP PD, Azure Disk, etc.)

## Step 2: Install Bindy

```bash
# Create namespace
kubectl create namespace dns-system

# Install CRDs
kubectl apply -k https://raw.githubusercontent.com/firestoned/bindy/main/deploy/crds/

# Install RBAC
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/rbac/

# Deploy controller
kubectl apply -f https://raw.githubusercontent.com/firestoned/bindy/main/deploy/operator/deployment.yaml

# Wait for controller to be ready
kubectl wait --for=condition=available --timeout=300s \
  deployment/bind9-controller -n dns-system
```

## Step 3: Create a BIND9 Cluster

First, create a cluster configuration that defines shared settings:

Create a file `bind9-cluster.yaml`:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Cluster
metadata:
  name: production-dns
  namespace: dns-system
spec:
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"
```

Apply it:

```bash
kubectl apply -f bind9-cluster.yaml
```

## Step 4: Create a BIND9 Instance

Now create an instance that references the cluster:

Create a file `bind9-instance.yaml`:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
spec:
  clusterRef: production-dns  # References the Bind9Cluster
  replicas: 1
```

Apply it:

```bash
kubectl apply -f bind9-instance.yaml
```

## Step 5: Create a DNS Zone

Create a file `dns-zone.yaml`:

```yaml
apiVersion: bindy.firestoned.io/v1alpha1
kind: DNSZone
metadata:
  name: example-com
  namespace: dns-system
spec:
  zoneName: example.com
  clusterRef: primary-dns  # References the Bind9Instance
  soaRecord:
    primaryNS: ns1.example.com.
    adminEmail: admin.example.com.  # Note: @ replaced with .
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTTL: 86400
  ttl: 3600
```

Apply it:

```bash
kubectl apply -f dns-zone.yaml
```

## Step 6: Add DNS Records

Create a file `dns-records.yaml`:

```yaml
# Web server A record
apiVersion: bindy.firestoned.io/v1alpha1
kind: ARecord
metadata:
  name: www-example
  namespace: dns-system
spec:
  zone: example-com
  name: www
  ipv4Address: "192.0.2.1"
  ttl: 300

---
# Blog CNAME record
apiVersion: bindy.firestoned.io/v1alpha1
kind: CNAMERecord
metadata:
  name: blog-example
  namespace: dns-system
spec:
  zone: example-com
  name: blog
  target: www.example.com.
  ttl: 300

---
# Mail server MX record
apiVersion: bindy.firestoned.io/v1alpha1
kind: MXRecord
metadata:
  name: mail-example
  namespace: dns-system
spec:
  zone: example-com
  name: "@"
  priority: 10
  mailServer: mail.example.com.
  ttl: 3600

---
# SPF TXT record
apiVersion: bindy.firestoned.io/v1alpha1
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
```

Apply them:

```bash
kubectl apply -f dns-records.yaml
```

## Step 7: Verify Your DNS Configuration

Check the status of your resources:

```bash
# Check BIND9 cluster
kubectl get bind9clusters -n dns-system

# Check BIND9 instance
kubectl get bind9instances -n dns-system

# Check DNS zone
kubectl get dnszones -n dns-system

# Check DNS records
kubectl get arecords,cnamerecords,mxrecords,txtrecords -n dns-system

# View detailed status
kubectl describe dnszone example-com -n dns-system
```

You should see output like:

```
NAME          ZONE          STATUS   AGE
example-com   example.com   Ready    1m
```

## Step 8: Test DNS Resolution

If your BIND9 instance is exposed (via LoadBalancer or NodePort):

```bash
# Get the BIND9 service IP
kubectl get svc -n dns-system

# Test DNS query (replace <BIND9-IP> with actual IP)
dig @<BIND9-IP> www.example.com
dig @<BIND9-IP> blog.example.com
dig @<BIND9-IP> example.com MX
dig @<BIND9-IP> example.com TXT
```

## What's Next?

You've successfully deployed Bindy and created your first DNS zone with records!

### Learn More

- [RNDC-Based Architecture](../concepts/architecture-rndc.md) - Understand the RNDC protocol architecture
- [Architecture Overview](../concepts/architecture.md) - Understand how Bindy works
- [Multi-Region Setup](../guide/multi-region.md) - Deploy across multiple regions
- [Status Conditions](../operations/status.md) - Monitor resource health

### Common Next Steps

1. **Add Secondary DNS Instances** for high availability
2. **Configure Zone Transfers** between primary and secondary
3. **Set up Monitoring** to track DNS performance
4. **Integrate with GitOps** for automated deployments
5. **Configure DNSSEC** for enhanced security

### Production Checklist

Before going to production:

- [ ] Deploy multiple controller replicas for HA
- [ ] Set up primary and secondary DNS instances
- [ ] Configure resource limits and requests
- [ ] Enable monitoring and alerting
- [ ] Set up backup for CRD definitions
- [ ] Configure RBAC properly
- [ ] Review security settings
- [ ] Test disaster recovery procedures

## Troubleshooting

If something doesn't work:

1. **Check controller logs**:
   ```bash
   kubectl logs -n dns-system -l app=bind9-controller -f
   ```

2. **Check resource status**:
   ```bash
   kubectl describe dnszone example-com -n dns-system
   ```

3. **Verify CRDs are installed**:
   ```bash
   kubectl get crd | grep bindy.firestoned.io
   ```

See the [Troubleshooting Guide](../operations/troubleshooting.md) for more help.
