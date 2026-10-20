<!--
  GENERATED FILE: DO NOT EDIT.
  Source: calm/bindy-multi-cluster.architecture.json
  Regenerate with: make calm-docs
-->

# Multi-Cluster: Queen Bee & Scout Fan-in

> Auto-generated from [`calm/bindy-multi-cluster.architecture.json`](https://github.com/firestoned/bindy/blob/main/calm/bindy-multi-cluster.architecture.json)
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

        subgraph child-cluster-a["Child Cluster A"]
        direction TB
            scout-a["Bindy Scout A"]:::node
            ingress-a["Ingress / Gateway #40;A#41;"]:::node
        end
        class child-cluster-a boundary
        subgraph child-cluster-b["Child Cluster B"]
        direction TB
            scout-b["Bindy Scout B"]:::node
            ingress-b["Ingress / Gateway #40;B#41;"]:::node
        end
        class child-cluster-b boundary
        subgraph queen-cluster["Queen Bee Cluster"]
        direction TB
            arecords["ARecord CRs #40;bindy-system#41;"]:::node
            queen-bind9["BIND9 Operands"]:::node
            queen-operator["Bindy Operator"]:::node
            scout-source-policy["bindy-scout-source-binding"]:::node
            queen-api["Queen Kubernetes API"]:::node
            scout-source-registry["Scout source registry #40;bindy-scout-sources ConfigMap#41;"]:::node
        end
        class queen-cluster boundary


    scout-a -->|watches Ingress / Service / HTTPRoute / TLSRoute| ingress-a
    scout-b -->|watches Ingress / Service / HTTPRoute / TLSRoute| ingress-b
    scout-a -->|writes one ARecord per DNS name as a complete, conditional replace #40;read, update its own contributor entry, write with the read resourceVersion as precondition, retry on 409; ADR-0026#41; via remote kubeconfig Secret or endpoint + token-file override, as its own ServiceAccount minted by the queen cluster #40;ADR-0008#41; and registered for its cluster names and zones #40;ADR-0025#41;| queen-api
    scout-b -->|writes one ARecord per DNS name as a complete, conditional replace #40;read, update its own contributor entry, write with the read resourceVersion as precondition, retry on 409; ADR-0026#41; via remote kubeconfig Secret or endpoint + token-file override, as its own ServiceAccount minted by the queen cluster #40;ADR-0008#41; and registered for its cluster names and zones #40;ADR-0025#41;| queen-api
    queen-api -->|persists fanned-in ARecords| arecords
    queen-api -->|evaluates the Scout source-binding policy on every ARecord write by a ServiceAccount of the record's namespace| scout-source-policy
    scout-source-policy -->|reads the record namespace's bindy-scout-sources ConfigMap as its parameter| scout-source-registry
    queen-operator -->|reconciles ARecords; a DNSZone selects a Scout-managed record only when its zone label equals the zone's name #40;ADR-0025#41;| arecords
    queen-operator -->|programs zones and records| queen-bind9



```
