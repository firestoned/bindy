# FAQ (Frequently Asked Questions)

## General

### What is Bindy?

Bindy is a Kubernetes operator that manages BIND9 DNS servers using Custom Resource Definitions (CRDs). It allows you to manage DNS zones and records declaratively using Kubernetes resources.

### Why use Bindy instead of manual BIND9 configuration?

- **Declarative**: Define DNS infrastructure as Kubernetes resources
- **GitOps-friendly**: Version control your DNS configuration
- **Kubernetes-native**: Uses familiar kubectl commands
- **Automated**: Operator handles BIND9 configuration and reloading
- **Scalable**: Easy multi-region, multi-instance deployments

### What BIND9 versions are supported?

Bindy supports BIND 9.16 and 9.18. The version is configurable per Bind9Instance.

## Installation

### Can I run Bindy in a namespace other than dns-system?

Yes, you can deploy Bindy in any namespace. Update the namespace in deployment YAMLs and RBAC resources.

### Do I need cluster-admin permissions?

You need permissions to:
- Create CRDs (cluster-scoped)
- Create ClusterRole and ClusterRoleBinding
- Create resources in the operator namespace

A cluster administrator can pre-install CRDs and RBAC, then delegate namespace management.

## Configuration

### How do I update BIND9 configuration?

Edit the Bind9Instance resource:

```bash
kubectl edit bind9instance primary-dns -n dns-system
```

The operator will automatically update the ConfigMap and restart pods if needed.

### Can I use external BIND9 servers?

No, Bindy manages BIND9 instances running in Kubernetes. For external servers, consider DNS integration tools.

### How do I enable query logging?

Currently, enable it manually in the BIND9 pod:

```bash
kubectl exec -n dns-system deployment/primary-dns -- rndc querylog on
```

Future versions may support configuration through Bind9Instance spec.

## DNS Zones

### How many zones can one instance host?

BIND9 can handle thousands of zones. Practical limits depend on:
- Resource allocation (CPU/memory)
- Query volume
- Zone size

Start with 100-500 zones per instance and scale as needed.

### Can I host the same zone on multiple instances?

Yes! Use label selectors to target multiple instances:

```yaml
instanceSelector:
  matchLabels:
    environment: production
```

This deploys the zone to all matching instances.

### How do I migrate zones between instances?

Update the DNSZone's instance Selector:

```yaml
instanceSelector:
  matchLabels:
    dns-role: new-primary
```

The zone will be created on new instances and you can delete from old ones.

## DNS Records

### How do I create multiple A records for the same name?

Create multiple ARecord resources with different names but same spec.name:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-1
spec:
  zone: example-com
  name: www
  ipv4Addresses:
    - "192.0.2.1"
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www-2
spec:
  zone: example-com
  name: www
  ipv4Addresses:
    - "192.0.2.2"
```

### Can I import existing zone files?

Not directly. You need to convert zone files to Bindy CRD resources. Future versions may include an import tool.

### How do I delete all records in a zone?

```bash
kubectl delete arecords,aaaarecords,cnamerecords,mxrecords,txtrecords \
  -n dns-system -l zone=example-com
```

(If you label records with their zone)

## Operations

### How do I upgrade Bindy?

1. Update CRDs: `kubectl apply -k deploy/crds/`
2. Update operator: `kubectl set image deployment/bindy operator=new-image`
3. Monitor rollout: `kubectl rollout status deployment/bindy -n dns-system`

### How do I backup DNS configuration?

```bash
# Export all CRDs
kubectl get bind9instances,dnszones,arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords \
  -A -o yaml > bindy-backup.yaml
```

Store in version control or backup storage.

### How do I restore from backup?

```bash
kubectl apply -f bindy-backup.yaml
```

### Can I run Bindy in high availability mode?

Yes, run multiple operator replicas:

```yaml
spec:
  replicas: 2  # Multiple operator replicas
```

Only one will be active (leader election), others are standby.

## Troubleshooting

### Pods are crashlooping

Check pod logs and events:

```bash
kubectl logs -n dns-system <pod-name>
kubectl describe pod -n dns-system <pod-name>
```

Common causes:
- Invalid BIND9 configuration
- Insufficient resources
- Image pull errors

### DNS queries timing out

Check:
1. Service is correctly exposing pods
2. Pods are ready
3. Query is reaching BIND9 (check logs)
4. Zone is loaded
5. Record exists

```bash
kubectl get svc -n dns-system
kubectl get pods -n dns-system
kubectl logs -n dns-system -l instance=primary-dns
```

### Zone transfers not working

Ensure:
1. Primary allows transfers: `spec.config.allowTransfer`
2. Network connectivity between primary and secondary
3. Secondary has correct primary server IPs
4. Firewall rules allow TCP port 53

## Performance

### How do I optimize for high query volume?

1. **Increase replicas**: More pods = more capacity
2. **Add resources**: Increase CPU/memory limits
3. **Use caching**: If appropriate for your use case
4. **Geographic distribution**: Deploy instances near clients
5. **Load balancing**: Use service load balancing

### What are typical resource requirements?

| Deployment Size | CPU Request | Memory Request | CPU Limit | Memory Limit |
|----------------|-------------|----------------|-----------|--------------|
| Small (<50 zones) | 100m | 128Mi | 500m | 512Mi |
| Medium (50-500 zones) | 200m | 256Mi | 1000m | 1Gi |
| Large (500+ zones) | 500m | 512Mi | 2000m | 2Gi |

Adjust based on actual usage monitoring.

## Security

### Is DNSSEC supported?

Yes, enable DNSSEC in Bind9Instance spec:

```yaml
spec:
  config:
    dnssec:
      enabled: true
      validation: true
```

### How do I restrict access to DNS queries?

Use `allowQuery` in Bind9Instance spec:

```yaml
spec:
  config:
    allowQuery:
      - "10.0.0.0/8"  # Only internal network
```

### Are zone transfers secure?

Zone transfers occur over TCP and can be restricted by IP address using `allowTransfer`. For additional security, consider:
- Network policies
- IPsec or VPN between regions
- TSIG keys (future enhancement)

## Integration

### Can I use Bindy with external-dns?

Bindy manages internal DNS infrastructure. external-dns manages external DNS providers. They serve different purposes and can coexist.

### Does Bindy work with Linkerd?

Yes, Bindy DNS servers can be used by Linkerd for internal DNS resolution. The DNS service has Linkerd injection disabled (DNS doesn't work well with mesh sidecars), while management services can be Linkerd-injected for secure mTLS communication.

### Can I integrate with existing DNS infrastructure?

Yes, configure Bindy instances as secondaries receiving transfers from existing primaries, or vice versa.

## Next Steps

- [Troubleshooting](./troubleshooting.md) - Debug issues
- [Common Issues](./common-issues.md) - Known problems
- [Debugging](./debugging.md) - Detailed debugging steps
