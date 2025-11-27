# Primary DNS Instances

Primary DNS instances are authoritative DNS servers that host the master copies of your DNS zones. They are the source of truth for DNS data and handle zone updates.

## Creating a Primary Instance

Here's a basic example of a primary DNS instance:

```yaml
apiVersion: dns.firestoned.io/v1alpha1
kind: Bind9Instance
metadata:
  name: primary-dns
  namespace: dns-system
  labels:
    dns-role: primary
    environment: production
spec:
  replicas: 2
  version: "9.18"
  config:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    allowTransfer:
      - "10.0.0.0/8"  # Allow zone transfers to secondary servers
    dnssec:
      enabled: true
      validation: true
```

Apply it with:

```bash
kubectl apply -f primary-instance.yaml
```

## Configuration Options

### Replicas

The `replicas` field controls how many BIND9 pods to run:

```yaml
spec:
  replicas: 2  # Run 2 pods for high availability
```

### BIND9 Version

Specify the BIND9 version to use:

```yaml
spec:
  version: "9.18"  # Use BIND 9.18
```

### Query Access Control

Control who can query your DNS server:

```yaml
spec:
  config:
    allowQuery:
      - "0.0.0.0/0"      # Allow queries from anywhere
      - "10.0.0.0/8"     # Or restrict to specific networks
```

### Zone Transfer Control

Restrict zone transfers to authorized servers (typically secondaries):

```yaml
spec:
  config:
    allowTransfer:
      - "10.0.0.0/8"     # Allow transfers to secondary network
      - "192.168.1.0/24" # Or specific secondary server network
```

### DNSSEC Configuration

Enable DNSSEC signing and validation:

```yaml
spec:
  config:
    dnssec:
      enabled: true      # Enable DNSSEC signing
      validation: true   # Enable DNSSEC validation
```

### Recursion

Primary authoritative servers should disable recursion:

```yaml
spec:
  config:
    recursion: false  # Disable recursion for authoritative servers
```

## Labels

Use labels to organize and select instances:

```yaml
metadata:
  labels:
    dns-role: primary        # Indicates this is a primary server
    environment: production  # Environment designation
    region: us-east-1       # Geographic location
```

These labels are used by DNSZone resources to select which instances should host their zones.

## Verifying Deployment

Check the instance status:

```bash
kubectl get bind9instances -n dns-system
kubectl describe bind9instance primary-dns -n dns-system
```

Check the created resources:

```bash
# View the deployment
kubectl get deployment -n dns-system -l instance=primary-dns

# View the pods
kubectl get pods -n dns-system -l instance=primary-dns

# View the service
kubectl get service -n dns-system -l instance=primary-dns
```

## Testing DNS Resolution

Once deployed, test DNS queries:

```bash
# Get the service IP
SERVICE_IP=$(kubectl get svc -n dns-system primary-dns -o jsonpath='{.spec.clusterIP}')

# Test DNS query
dig @$SERVICE_IP example.com
```

## Next Steps

- [Create DNS Zones](./creating-zones.md) to host on this instance
- [Setup Secondary Instances](./secondary-instance.md) for redundancy
- [Configure Multi-Region Setup](./multi-region.md) for geographic distribution
