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

### Bind9Cluster
- Uses `Ready` condition type with granular reasons
- Condition reasons:
  - `AllInstancesReady`: All instances in the cluster are ready
  - `SomeInstancesNotReady`: Some instances are not ready (cluster partially functional)
  - `NoInstancesReady`: No instances are ready (cluster not functional)
- Additional status fields:
  - `instanceCount`: Total number of instances
  - `readyInstances`: Number of ready instances
  - `instances`: List of instance names

### DNSZone
- Uses `Progressing`, `Degraded`, and `Ready` condition types with granular reasons
- **Reconciliation Flow**:
  1. `Progressing/PrimaryReconciling`: Before configuring primary instances
  2. `Progressing/PrimaryReconciled`: After successful primary configuration
  3. `Progressing/SecondaryReconciling`: Before configuring secondary instances
  4. `Progressing/SecondaryReconciled`: After successful secondary configuration
  5. `Ready/ReconcileSucceeded`: When all phases complete successfully
- **Error Conditions**:
  - `Degraded/PrimaryFailed`: Primary reconciliation failed (fatal error)
  - `Degraded/SecondaryFailed`: Secondary reconciliation failed (primaries still work, non-fatal)
- Additional status fields:
  - `recordCount`: Number of records in the zone
  - `secondaryIps`: IP addresses of configured secondary servers
  - `observedGeneration`: Last observed generation

### DNS Records (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA)
- Use `Progressing`, `Degraded`, and `Ready` condition types with granular reasons
- **Reconciliation Flow**:
  1. `Progressing/RecordReconciling`: Before configuring record on endpoints
  2. `Ready/ReconcileSucceeded`: When record is successfully configured on all endpoints
- **Error Conditions**:
  - `Degraded/RecordFailed`: Record configuration failed (includes error details)
- Status message includes count of configured endpoints (e.g., "Record configured on 3 endpoint(s)")
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

### DNSZone - Progressing (Primary Reconciliation)
```yaml
status:
  conditions:
    - type: Progressing
      status: "True"
      reason: PrimaryReconciling
      message: "Configuring zone on primary instances"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1
  recordCount: 0
```

### DNSZone - Progressing (Secondary Reconciliation)
```yaml
status:
  conditions:
    - type: Progressing
      status: "True"
      reason: SecondaryReconciling
      message: "Configured on 2 primary server(s), now configuring secondaries"
      lastTransitionTime: "2024-11-26T10:00:01Z"
  observedGeneration: 1
  recordCount: 0
  secondaryIps:
    - "10.42.0.5"
    - "10.42.0.6"
```

### DNSZone - Successfully Reconciled
```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSucceeded
      message: "Configured on 2 primary server(s) and 3 secondary server(s)"
      lastTransitionTime: "2024-11-26T10:00:02Z"
  observedGeneration: 1
  recordCount: 5
  secondaryIps:
    - "10.42.0.5"
    - "10.42.0.6"
    - "10.42.0.7"
```

### DNSZone - Degraded (Secondary Failure)
```yaml
status:
  conditions:
    - type: Degraded
      status: "True"
      reason: SecondaryFailed
      message: "Configured on 2 primary server(s), but secondary configuration failed: connection timeout"
      lastTransitionTime: "2024-11-26T10:00:02Z"
  observedGeneration: 1
  recordCount: 5
  secondaryIps:
    - "10.42.0.5"
    - "10.42.0.6"
```

### DNSZone - Failed (Primary Failure)
```yaml
status:
  conditions:
    - type: Degraded
      status: "True"
      reason: PrimaryFailed
      message: "Failed to configure zone on primaries: No Bind9Instances matched selector"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1
  recordCount: 0
```

### DNS Record - Progressing
```yaml
status:
  conditions:
    - type: Progressing
      status: "True"
      reason: RecordReconciling
      message: "Configuring A record on zone endpoints"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1
```

### DNS Record - Successfully Configured
```yaml
status:
  conditions:
    - type: Ready
      status: "True"
      reason: ReconcileSucceeded
      message: "Record configured on 3 endpoint(s)"
      lastTransitionTime: "2024-11-26T10:00:01Z"
  observedGeneration: 1
```

### DNS Record - Failed
```yaml
status:
  conditions:
    - type: Degraded
      status: "True"
      reason: RecordFailed
      message: "Failed to configure record: Zone not found on primary servers"
      lastTransitionTime: "2024-11-26T10:00:01Z"
  observedGeneration: 1
```

### Bind9Cluster - Partially Ready
```yaml
status:
  conditions:
    - type: Ready
      status: "False"
      reason: SomeInstancesNotReady
      message: "2/3 instances ready"
      lastTransitionTime: "2024-11-26T10:00:00Z"
  observedGeneration: 1
  instanceCount: 3
  readyInstances: 2
  instances:
    - production-dns-primary-0
    - production-dns-primary-1
    - production-dns-secondary-0
```

## Validation

All condition types are enforced via CRD validation. Attempting to use a condition type not in the enum will result in a validation error:

```bash
$ kubectl apply -f invalid-condition.yaml
Error from server (Invalid): error when creating "invalid-condition.yaml":
Bind9Instance.bindy.firestoned.io "test" is invalid:
status.conditions[0].type: Unsupported value: "CustomType":
supported values: "Ready", "Available", "Progressing", "Degraded", "Failed"
```
