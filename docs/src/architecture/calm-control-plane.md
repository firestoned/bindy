<!--
  GENERATED FILE — DO NOT EDIT.
  Source: calm/bindy-control-plane.architecture.json
  Regenerate with: make calm-docs
-->

# Control Plane — Reconcilers, CRDs & Operands

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
                end
                class bind9-pod boundary
        end
        class bindy-system boundary

    crd-cluster["Bind9Cluster #40;CRD#41;"]:::node
    crd-instance["Bind9Instance #40;CRD#41;"]:::node
    crd-provider["ClusterBind9Provider #40;CRD#41;"]:::node
    dns-client["DNS Client"]:::node
    crd-records["DNS Record CRDs"]:::node
    crd-dnszone["DNSZone #40;CRD#41;"]:::node
    k8s-api["Kubernetes API Server"]:::node
    admission-policies["ValidatingAdmissionPolicies"]:::node

    bindy-operator -->|watches and patches custom resources| k8s-api
    k8s-api -->|enforces CEL policies on CR and pod admission| admission-policies
    bindy-operator -->|reconciles| crd-cluster
    bindy-operator -->|reconciles| crd-dnszone
    bindy-operator -->|reconciles #40;8 record kinds#41;| crd-records
    crd-provider -->|creates / owns| crd-cluster
    crd-cluster -->|creates / owns| crd-instance
    crd-instance -->|creates / owns Deployment| bind9-pod
    crd-dnszone -->|selects member records via label selector| crd-records
    bindy-operator -->|add / delete / notify zones #40;SA token, TokenReview#41;| bindcar
    bindy-operator -->|DNS UPDATE #40;RFC 2136, TSIG#41;| named
    bindcar -->|rndc / nsupdate #40;local#41;| named
    dns-client -->|DNS query| bind9-svc
    bind9-svc -->|routes :53 to named :5353| named



```
