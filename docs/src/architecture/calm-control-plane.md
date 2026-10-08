<!--
  GENERATED FILE: DO NOT EDIT.
  Source: calm/bindy-control-plane.architecture.json
  Regenerate with: make calm-docs
-->

# Control Plane: Reconcilers, CRDs & Operands

> Auto-generated from [`calm/bindy-control-plane.architecture.json`](https://github.com/firestoned/bindy/blob/main/calm/bindy-control-plane.architecture.json)
> via `make calm-docs`. Edit the CALM model, not this page.

```mermaid
---
config:
  theme: base
  themeVariables:
    fontFamily: -apple-system, BlinkMacSystemFont, 'Segoe WPC', 'Segoe UI', system-ui, 'Ubuntu', sans-serif
    darkMode: false
    fontSize: 14px
    edgeLabelBackground: '#d5d7e1'
    lineColor: '#000000'
---
%%{init: {"layout": "dagre", "flowchart": {"htmlLabels": false}}}%%
flowchart TB
classDef boundary fill:#e1e4f0,stroke:#204485,stroke-dasharray: 5 4,stroke-width:1px,color:#000000;
classDef node fill:#eef1ff,stroke:#007dff,stroke-width:1px,color:#000000;
classDef iface fill:#f0f0f0,stroke:#b6b6b6,stroke-width:1px,font-size:10px,color:#000000;
classDef highlight fill:#fdf7ec,stroke:#f0c060,stroke-width:1px,color:#000000;

        subgraph bindy-system["bindy-system Namespace"]
        direction TB
            bind9-svc["BIND9 Service"]:::node
            bindy-operator["Bindy Operator"]:::node
                subgraph bind9-pod["BIND9 Operand Pod"]
                direction TB
                    named["BIND9 named"]:::node
                    bindcar["bindcar API Sidecar"]:::node
                    dnssec-keys-init["DNSSEC key init container"]:::node
                end
                class bind9-pod boundary
        end
        class bindy-system boundary

    crd-cluster["Bind9Cluster #40;CRD#41;"]:::node
    crd-instance["Bind9Instance #40;CRD#41;"]:::node
    crd-provider["ClusterBind9Provider #40;CRD#41;"]:::node
    dns-client["DNS Client"]:::node
    crd-records["DNS Record CRDs"]:::node
    dnssec-key-secret["DNSSEC key Secret"]:::node
    crd-dnszone["DNSZone #40;CRD#41;"]:::node
    k8s-api["Kubernetes API Server"]:::node
    admission-policies["ValidatingAdmissionPolicies"]:::node

    bindy-operator -->|watches and patches custom resources: one shared watch per kind and namespace target, subscribed to by every controller #40;ADR-0009#41;; client-side rate limited at 20 QPS / 30 burst default, paginated LISTs, retries with exponential backoff #40;ADR-0005#41;; non-watch requests bounded by a 30 s client-side deadline, watches exempt #40;ADR-0014#41;| k8s-api
    k8s-api -->|enforces CEL policies on CR and pod admission| admission-policies
    bindy-operator -->|reconciles| crd-cluster
    bindy-operator -->|reconciles| crd-dnszone
    bindy-operator -->|reconciles #40;8 record kinds#41;| crd-records
    crd-provider -->|creates / owns| crd-cluster
    crd-cluster -->|creates / owns| crd-instance
    crd-instance -->|creates / owns Deployment; a pod-template change is applied only while no instance sharing a zone or a cluster with it is mid-rollout, one at a time in queue order #40;ADR-0018#41;| bind9-pod
    crd-dnszone -->|selects member records via label selector| crd-records
    bindy-operator -->|watches BIND9 pods #40;label-selected#41; and patches the bindy.firestoned.io/zones-loaded condition on pods/status once every live zone and its records are loaded on the pod, and back to False when the pod starts terminating; the Ready condition of the pod follows on the next kubelet status sync #40;ADR-0017, amended and corrected 2026-10-07#41;| bind9-pod
    bindy-operator -->|add / delete / notify zones #40;SA token, TokenReview#41;, on every container-ready pod, including pods the zones-loaded gate holds out of Service #40;ADR-0017#41;; keeps each zone's transfer peers in step with the pods: rewrites a primary's allow-transfer / also-notify, replaces a secondary zone whose primaries moved, records the peers in DNSZone status.transferPeers #40;ADR-0019#41;| bindcar
    bindy-operator -->|DNS UPDATE #40;RFC 2136, TSIG#41;| named
    bindy-operator -->|queries DNSKEY #40;read-only, DNS over UDP :5353#41; to derive DS records for DNSZone status, ADR-0006| named
    bindcar -->|rndc / nsupdate #40;local#41;| named
    dnssec-keys-init -->|reads the shared DNSSEC keys #40;Secret volume, read-only, init container only#41; - ADR-0012| dnssec-key-secret
    dnssec-keys-init -->|copies the keys into named's writable key-directory #40;emptyDir#41; before named starts - ADR-0012| named
    dns-client -->|DNS query| bind9-svc
    bind9-svc -->|routes :53 to named :5353 on Ready pods only; a pod is Ready once its zones-loaded readiness gate is True, and stops being Ready once the kubelet syncs the closed gate of a terminating pod #40;ADR-0017#41;| named
    named -->|zone transfer between BIND9 pods: a secondary named transfers #40;AXFR/IXFR, TCP :5353#41; from the primary pod IPs in its primaries list, allowed by the primaries' allow-transfer #40;exactly the zone's live secondary pod IPs#41;; a primary sends NOTIFY to each secondary instance's Service ClusterIP on :53 #40;ADR-0019#41;| bind9-svc



```
