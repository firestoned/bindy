# Status Conditions

This document describes the standardized status conditions used across all Bindy CRDs.

## Condition Types

All Bindy custom resources (Bind9Instance, DNSZone, and all DNS record types) use the following standardized condition types:

### Ready
- **Description**: Indicates whether the resource is fully operational and ready to serve its intended purpose
- **Common Use**: Primary condition type used by all reconcilers
- **Status Values**:
  - `True`: Resource is ready and operational
  - `False`: Resource is not ready (error or in progress)
  - `Unknown`: Status cannot be determined

### Available
- **Description**: Indicates whether the resource is available for use
- **Common Use**: Used to distinguish between "ready" and "available" when resources may be ready but not yet serving traffic
- **Status Values**:
  - `True`: Resource is available
  - `False`: Resource is not available
  - `Unknown`: Availability cannot be determined

### Progressing
- **Description**: Indicates whether the resource is currently being worked on
- **Common Use**: During initial creation or updates
- **Status Values**:
  - `True`: Resource is being created or updated
  - `False`: Resource is not currently progressing
  - `Unknown`: Progress status cannot be determined

### Degraded
- **Description**: Indicates that the resource is functioning but in a degraded state
- **Common Use**: When some replicas are down but service continues, or when non-critical features are unavailable
- **Status Values**:
  - `True`: Resource is degraded
  - `False`: Resource is not degraded
  - `Unknown`: Degradation status cannot be determined

### Failed
- **Description**: Indicates that the resource has failed and cannot fulfill its purpose
- **Common Use**: Permanent failures that require intervention
- **Status Values**:
  - `True`: Resource has failed
  - `False`: Resource has not failed
  - `Unknown`: Failure status cannot be determined

## Condition Structure

All conditions follow this structure:

```yaml
status:
  conditions:
    - type: Ready              # One of: Ready, Available, Progressing, Degraded, Failed
      status: "True"           # One of: "True", "False", "Unknown"
      reason: Ready            # Machine-readable reason (typically same as type)
      message: "Bind9Instance configured with 2 replicas"  # Human-readable message
      lastTransitionTime: "2024-11-26T10:00:00Z"          # RFC3339 timestamp
  observedGeneration: 1        # Generation last observed by controller
  # Resource-specific fields (replicas, recordCount, etc.)
```

## Current Usage

### Bind9Instance
- Uses `Ready` condition type
- Status `True` when Deployment, Service, and ConfigMap are successfully created
- Status `False` when resource creation fails
- Additional status fields:
  - `replicas`: Total number of replicas
  - `readyReplicas`: Number of ready replicas

### DNSZone
- Uses `Ready` condition type
- Status `True` when zone file is created and instances are matched
- Status `False` when zone creation fails
- Additional status fields:
  - `recordCount`: Number of records in the zone
  - `observedGeneration`: Last observed generation

### DNS Records (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)
- All use `Ready` condition type
- Status `True` when record is successfully added to zone
- Status `False` when record creation fails
- Additional status fields:
  - `observedGeneration`: Last observed generation

## Best Practices

1. **Always set the condition type**: Use one of the five standardized types
2. **Include timestamps**: Set `lastTransitionTime` when condition status changes
3. **Provide clear messages**: The `message` field should be human-readable and actionable
4. **Use appropriate reasons**: The `reason` field should be machine-readable and consistent
5. **Update observedGeneration**: Always update to match the resource's current generation
6. **Multiple conditions**: Resources can have multiple conditions simultaneously (e.g., `Ready: True` and `Degraded: True`)

## Examples

### Successful Bind9Instance
```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: Ready
      message: "Bind9Instance configured with 2 replicas"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1
  replicas: 2
  readyReplicas: 2
```

### Failed DNSZone
```yaml
status:
  conditions:
    - type: Ready
      status: "False"
      reason: Failed
      message: "No Bind9Instances matched selector"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1
  recordCount: 0
```

### Progressing Deployment
```yaml
status:
  conditions:
    - type: Progressing
      status: "True"
      reason: Progressing
      message: "Deployment is rolling out"
      lastTransitionTime: "2024-11-26T10:00:00Z"
    - type: Ready
      status: "False"
      reason: Progressing
      message: "Waiting for deployment to complete"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 2
  replicas: 2
  readyReplicas: 1
```

## Validation

All condition types are enforced via CRD validation. Attempting to use a condition type not in the enum will result in a validation error:

```bash
$ kubectl apply -f invalid-condition.yaml
Error from server (Invalid): error when creating "invalid-condition.yaml":
Bind9Instance.dns.firestoned.io "test" is invalid:
status.conditions[0].type: Unsupported value: "CustomType":
supported values: "Ready", "Available", "Progressing", "Degraded", "Failed"
```
