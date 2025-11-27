# Architecture Deep Dive

Technical architecture of the Bindy DNS operator.

## System Architecture

```
┌─────────────────────────────────────┐
│     Kubernetes API Server           │
└──────────────┬──────────────────────┘
               │ Watch/Update
     ┌─────────▼────────────┐
     │  Bindy Controller    │
     │  ┌────────────────┐  │
     │  │ Reconcilers    │  │
     │  │  - Bind9Inst   │  │
     │  │  - DNSZone     │  │
     │  │  - Records     │  │
     │  └────────────────┘  │
     └──────┬───────────────┘
            │ Manages
     ┌──────▼────────────────┐
     │  BIND9 Pods           │
     │  ┌──────────────────┐ │
     │  │ ConfigMaps       │ │
     │  │ Deployments      │ │
     │  │ Services         │ │
     │  └──────────────────┘ │
     └───────────────────────┘
```

## Components

### Controller
- Watches CRD resources
- Reconciles desired vs actual state
- Manages Kubernetes resources

### Reconcilers
- Per-resource reconciliation logic
- Idempotent operations
- Error handling and retries

### BIND9 Integration
- Configuration generation
- Zone file management
- BIND9 lifecycle management

See detailed docs:
- [Controller Design](./controller-design.md)
- [Reconciliation Logic](./reconciliation.md)
- [BIND9 Integration](./bind9-integration.md)
