# Basic Concepts

This section introduces the core concepts behind Bindy and how it manages DNS infrastructure in Kubernetes.

## The Kubernetes Way

Bindy follows Kubernetes patterns and idioms:

- **Declarative Configuration** - You declare what DNS records should exist, Bindy makes it happen
- **Custom Resources** - DNS zones and records are Kubernetes resources
- **Controllers** - Bindy watches resources and reconciles state
- **Labels and Selectors** - Target specific BIND9 instances using labels
- **Status Subresources** - Track the health and state of DNS resources

## Core Resources

Bindy introduces these Custom Resource Definitions (CRDs):

### Infrastructure Resources

- **[Bind9Instance](./bind9instance.md)** - Represents a BIND9 DNS server deployment

### DNS Resources

- **[DNSZone](./dnszone.md)** - Defines a DNS zone with SOA record
- **[DNS Records](./records.md)** - Individual DNS record types:
  - ARecord (IPv4)
  - AAAARecord (IPv6)
  - CNAMERecord (Canonical Name)
  - MXRecord (Mail Exchange)
  - TXTRecord (Text)
  - NSRecord (Name Server)
  - SRVRecord (Service)
  - CAARecord (Certificate Authority Authorization)

## How It Works

```
┌─────────────────────────────────────────────────────────────┐
│                     Kubernetes API                          │
│  ┌──────────┐  ┌─────────┐  ┌──────────┐  ┌──────────┐    │
│  │  DNSZone │  │ ARecord │  │ MXRecord │  │ TXTRecord│ ...│
│  └────┬─────┘  └────┬────┘  └────┬─────┘  └────┬─────┘    │
│       │             │            │             │           │
└───────┼─────────────┼────────────┼─────────────┼───────────┘
        │             │            │             │
        └─────────────┴────────────┴─────────────┘
                      │
                      ▼
        ┌─────────────────────────────┐
        │   Bindy Controller          │
        │   • Watches CRDs            │
        │   • Reconciles state        │
        │   • Generates zone files    │
        │   • Updates BIND9 config    │
        └──────────────┬──────────────┘
                       │
                       ▼
        ┌─────────────────────────────┐
        │   BIND9 Instances           │
        │   • Primary servers         │
        │   • Secondary servers       │
        │   • Zone files              │
        │   • DNS queries             │
        └─────────────────────────────┘
```

### Reconciliation Loop

1. **Watch** - Controller watches for changes to DNS resources
2. **Evaluate** - Determines which BIND9 instances should host the zone
3. **Generate** - Creates BIND9 zone file configuration
4. **Apply** - Updates BIND9 configuration
5. **Status** - Reports success or failure via status conditions

## Label Selectors

Label selectors are the key to targeting specific BIND9 instances:

```yaml
# DNS Zone selects BIND9 instances
instanceSelector:
  matchLabels:
    dns-role: primary
    region: us-east
```

This allows:
- **Multi-region** DNS deployments
- **Primary/Secondary** architectures
- **Environment** separation (dev/staging/prod)
- **Custom** targeting based on your needs

## Resource Relationships

```
Bind9Instance (has labels)
    ↑
    │ selected by
    │
DNSZone (has instanceSelector)
    ↑
    │ referenced by
    │
DNS Records (A, CNAME, MX, etc.)
```

- **Bind9Instance** - Standalone resource with labels
- **DNSZone** - Selects instances and defines the zone
- **DNS Records** - Reference the zone they belong to

## Status and Conditions

All resources report their status:

```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Synchronized
      message: Zone created for 2 instances
      lastTransitionTime: 2024-01-01T00:00:00Z
  observedGeneration: 1
  matchedInstances: 2
```

Status conditions follow Kubernetes conventions:
- **Type** - What aspect (Ready, Synced, etc.)
- **Status** - True, False, or Unknown
- **Reason** - Machine-readable reason code
- **Message** - Human-readable description

## Next Steps

- [Architecture Overview](./architecture.md) - Deep dive into Bindy's architecture
- [Custom Resource Definitions](./crds.md) - Detailed CRD specifications
- [Bind9Instance](./bind9instance.md) - Learn about BIND9 instance resources
- [DNSZone](./dnszone.md) - Learn about DNS zone resources
- [DNS Records](./records.md) - Learn about DNS record types
