# Mothership Integration Guide - Bind9 RNDC Management

This guide explains how to integrate the dedicated Bind9 cluster with your k0rdent mothership to manage DNS records via RNDC over Linkerd.

## Architecture

```
k0rdent Mothership
    └── DNS Management Pod (Linkerd-injected)
            └── Linkerd mTLS Tunnel
                    └── Bind9 Cluster (bind9-rndc service)
                            └── RNDC Port 953
```

## Prerequisites

- Bind9 cluster deployed with `bind9-rndc` service
- Linkerd service mesh installed on BOTH clusters
- Network connectivity between clusters
- Cluster context names configured in `~/.kube/config`

## Step 1: Export RNDC Configuration from Bind9 Cluster

```bash
# Get the RNDC key from the Bind9 cluster secret
kubectl get secret -n bind9 bind9-rndc-key -o jsonpath='{.data.rndc\.conf}' | base64 -d > ~/rndc.conf

# Set proper permissions
chmod 600 ~/rndc.conf
```

## Step 2: Create External Service in Mothership

Create a service that acts as a proxy to the Bind9 cluster:

```bash
# Switch to mothership context
kubectl config use-context <MOTHERSHIP_CONTEXT>

# Create namespace for DNS management
kubectl create namespace dns-management --dry-run=client -o yaml | kubectl apply -f -

# Create the external service pointing to bind9-rndc
kubectl apply -f - <<EOF
apiVersion: v1
kind: Service
metadata:
  name: bind9-rndc-external
  namespace: dns-management
spec:
  type: ExternalName
  externalName: bind9-rndc.bind9.svc.cluster.local
  ports:
  - name: rndc
    port: 953
    protocol: TCP
---
apiVersion: v1
kind: Secret
metadata:
  name: bind9-rndc-key
  namespace: dns-management
type: Opaque
stringData:
  rndc.conf: |
    key "rndc-key" {
      algorithm hmac-sha256;
      secret "K8s+Bind9RndcKey==";
    };

    options {
      default-key "rndc-key";
      default-server bind9-rndc-external;
      default-port 953;
    };
EOF
```

## Step 3: Enable Service-to-Service Communication with Linkerd

If clusters are in different namespaces, create a Linkerd ServiceImport:

```bash
# On Bind9 cluster: Create a pod that exports the RNDC service
kubectl apply -f - <<EOF
apiVersion: multicluster.io/v1alpha1
kind: ServiceExport
metadata:
  name: bind9-rndc
  namespace: bind9
EOF

# On Mothership: Import the service
kubectl apply -f - <<EOF
apiVersion: multicluster.io/v1alpha1
kind: ServiceImport
metadata:
  name: bind9-rndc
  namespace: dns-management
spec:
  ports:
  - port: 953
    protocol: TCP
  type: ClusterSetIP
EOF
```

## Step 4: Create DNS Management Deployment in Mothership

Create a deployment with DNS management tools that can update Bind9:

```bash
kubectl apply -f - <<EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: dns-manager
  namespace: dns-management
spec:
  replicas: 1
  selector:
    matchLabels:
      app: dns-manager
  template:
    metadata:
      labels:
        app: dns-manager
        linkerd.io/inject: enabled  # ⚠️ CRITICAL: Enable Linkerd injection
      annotations:
        linkerd.io/skip-inbound: "false"
    spec:
      serviceAccountName: dns-manager
      containers:
      - name: manager
        image: debian:bookworm-slim
        imagePullPolicy: IfNotPresent
        
        # Install bind9-utils and dnsutils for RNDC and DNS tools
        command: ["/bin/bash"]
        args:
          - -c
          - |
            apt-get update && apt-get install -y bind9-utils dnsutils curl
            # Keep container running
            sleep infinity
        
        volumeMounts:
        - name: rndc-key
          mountPath: /etc/bind
          readOnly: true
        
        env:
        - name: RNDC_SERVER
          value: "bind9-rndc-external.dns-management.svc.cluster.local"
        - name: RNDC_PORT
          value: "953"
      
      volumes:
      - name: rndc-key
        secret:
          secretName: bind9-rndc-key
          defaultMode: 0600

---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: dns-manager
  namespace: dns-management

---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: dns-manager
  namespace: dns-management
rules:
- apiGroups: [""]
  resources: ["secrets"]
  verbs: ["get", "list"]
  resourceNames: ["bind9-rndc-key"]

---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: dns-manager
  namespace: dns-management
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: dns-manager
subjects:
- kind: ServiceAccount
  name: dns-manager
  namespace: dns-management
EOF
```

## Step 5: Test Mothership-to-Bind9 Communication

```bash
# Get the DNS manager pod
POD_NAME=$(kubectl get pods -n dns-management -l app=dns-manager -o jsonpath='{.items[0].metadata.name}')

# Test DNS resolution through Bind9
kubectl exec -n dns-management "$POD_NAME" -- \
  dig @bind9-rndc-external.dns-management.svc.cluster.local -p 53 localhost

# Test RNDC status (requires proper RNDC key)
kubectl exec -n dns-management "$POD_NAME" -- \
  rndc -s bind9-rndc-external.dns-management.svc.cluster.local -p 953 status

# Check Linkerd mTLS tunnel
kubectl get pods -n dns-management -o json | jq '.items[].metadata.annotations."linkerd.io/inject"'
```

## Step 6: Automated DNS Record Management

Create a CronJob in mothership to periodically update DNS records:

```bash
kubectl apply -f - <<EOF
apiVersion: batch/v1
kind: CronJob
metadata:
  name: dns-sync
  namespace: dns-management
spec:
  schedule: "*/5 * * * *"  # Every 5 minutes
  jobTemplate:
    spec:
      template:
        metadata:
          labels:
            linkerd.io/inject: enabled
        spec:
          serviceAccountName: dns-manager
          containers:
          - name: sync
            image: debian:bookworm-slim
            imagePullPolicy: IfNotPresent
            
            command:
            - /bin/bash
            - -c
            - |
              apt-get update && apt-get install -y bind9-utils curl
              
              # Example: Add new zone
              rndc -s bind9-rndc-external -p 953 \
                addzone example.com '{type master; file "/etc/bind/zones/example.com"; };'
              
              # Example: Reload zones
              rndc -s bind9-rndc-external -p 953 reload
              
              # Log successful sync
              echo "DNS sync completed at $(date)"
            
            volumeMounts:
            - name: rndc-key
              mountPath: /etc/bind
              readOnly: true
          
          volumes:
          - name: rndc-key
            secret:
              secretName: bind9-rndc-key
              defaultMode: 0600
          
          restartPolicy: OnFailure
EOF
```

## Step 7: Verification

```bash
# Check Linkerd tunnel status
kubectl get svc -n dns-management
kubectl get pods -n dns-management -o json | jq '.items[] | {name: .metadata.name, linkerd: .metadata.annotations."linkerd.io/inject"}'

# Check Linkerd mTLS metrics
linkerd tap -n dns-management deploy/dns-manager --to-namespace=bind9

# Monitor cross-cluster communication
linkerd stat -n dns-management deploy/dns-manager

# View pod logs for errors
kubectl logs -n dns-management -l app=dns-manager -f
```

## RNDC Command Reference

Common commands to manage Bind9 from mothership:

```bash
# Get pod for exec
POD_NAME=$(kubectl get pods -n dns-management -l app=dns-manager -o jsonpath='{.items[0].metadata.name}')
RNDC_CMD="rndc -s bind9-rndc-external.dns-management.svc.cluster.local -p 953"

# Status
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD status

# Reload configuration
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD reload

# Reload specific zone
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD reload example.com

# Add zone
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD addzone example.com '{type master; file "/etc/bind/zones/example.com"; };'

# Remove zone
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD delzone example.com

# Flush cache
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD flush

# Query zones
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD zonestatus example.com

# Freeze zone (prevent updates)
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD freeze example.com

# Thaw zone (allow updates)
kubectl exec -n dns-management "$POD_NAME" -- $RNDC_CMD thaw example.com
```

## Troubleshooting

### Connection Refused

```bash
# Check if RNDC service is accessible
kubectl exec -n dns-management "$POD_NAME" -- \
  nc -zv bind9-rndc-external.dns-management.svc.cluster.local 953

# Check Linkerd injection
kubectl get pods -n dns-management -l app=dns-manager -o json | \
  jq '.items[].metadata.annotations | select(has("linkerd.io/inject"))'
```

### Authentication Failed

```bash
# Verify RNDC key matches on both clusters
kubectl get secret -n bind9 bind9-rndc-key -o jsonpath='{.data.rndc\.conf}' | base64 -d | grep secret
kubectl get secret -n dns-management bind9-rndc-key -o jsonpath='{.data.rndc\.conf}' | base64 -d | grep secret
```

### mTLS Certificate Issues

```bash
# Check Linkerd cert status
linkerd cert
linkerd check

# Restart pods to refresh certs
kubectl rollout restart deployment/dns-manager -n dns-management
```

## Security Best Practices

1. **RNDC Key Rotation**: Periodically update the RNDC key in both clusters
2. **RBAC Restrictions**: Limit who can access the RNDC secret
3. **Network Policies**: Restrict traffic to only necessary pods
4. **Audit Logging**: Monitor all RNDC commands executed
5. **mTLS Verification**: Ensure Linkerd is properly injecting sidecars

## References

- [RNDC Manual](https://bind9.readthedocs.io/en/latest/reference/man_rndc.html)
- [Linkerd Multicluster](https://linkerd.io/2.14/features/multicluster/)
- [Bind9 Remote Management](https://bind9.readthedocs.io/en/latest/reference/config.html#stmt-controls)
