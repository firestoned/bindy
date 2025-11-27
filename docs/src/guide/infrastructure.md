# Creating DNS Infrastructure

This section guides you through setting up your DNS infrastructure using Bindy. A typical DNS setup consists of:

- **Primary DNS Instances**: Authoritative DNS servers that host the master copies of your zones
- **Secondary DNS Instances**: Replica servers that receive zone transfers from primaries
- **Multi-Region Setup**: Geographically distributed DNS servers for redundancy

## Overview

Bindy uses Kubernetes Custom Resources to define DNS infrastructure. The `Bind9Instance` resource creates and manages BIND9 DNS server deployments, including:

- BIND9 Deployment pods
- ConfigMaps for BIND9 configuration
- Services for DNS traffic (TCP/UDP port 53)

## Infrastructure Components

### Bind9Instance

A `Bind9Instance` represents a single BIND9 DNS server deployment. You can create multiple instances for:

- **High availability** - Multiple replicas of the same instance
- **Role separation** - Separate primary and secondary instances
- **Geographic distribution** - Instances in different regions or availability zones

## Planning Your Infrastructure

Before creating instances, consider:

1. **Zone Hosting Strategy**
   - Which zones will be primary vs. secondary?
   - How will zones be distributed across instances?

2. **Redundancy Requirements**
   - How many replicas per instance?
   - How many geographic locations?

3. **Label Strategy**
   - How will you select instances for zones?
   - Common labels: `dns-role`, `region`, `environment`

## Next Steps

- [Create Primary DNS Instances](./primary-instance.md)
- [Create Secondary DNS Instances](./secondary-instance.md)
- [Setup Multi-Region Infrastructure](./multi-region.md)
