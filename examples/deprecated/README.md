# Deprecated Examples

This directory contains examples from previous versions of Bindy that demonstrate deprecated architectures or patterns.

**These examples are NOT recommended for new deployments.**

## Why These Examples Are Deprecated

### Architecture Change (v0.2.x → v0.3.x)

The zone/instance selection architecture was reversed between v0.2.x and v0.3.x:

**OLD Architecture (v0.2.x and earlier):**
- `Bind9Cluster.spec.common.zonesFrom` - Instances selected zones
- Zones were passive, waiting to be discovered by instances
- Polling-based discovery

**NEW Architecture (v0.3.x and later):**
- `DNSZone.spec.bind9InstancesFrom` - Zones select instances
- Zones are active, discovering and binding to instances
- Event-driven watch-based discovery (sub-second response time)

## Migration

If you are using patterns from these deprecated examples, please refer to:

- [Migration Guide](../../docs/src/operations/dnszone-migration-troubleshooting.md) - Step-by-step migration instructions
- [Current Examples](../) - Updated examples using the new architecture
- [dnszone-selection-methods.yaml](../dnszone-selection-methods.yaml) - Comprehensive example showing all zone selection methods

## Files in This Directory

- `zone-label-selector.yaml` - OLD pattern where instances selected zones via `zonesFrom`

## Removal Timeline

These deprecated examples will be removed in v0.4.0 (estimated Q2 2026).
