# Architecture Diagrams

Comprehensive visual diagrams showing Bindy's architecture, components, and data flows.

## System Architecture

```mermaid
graph TB
    subgraph "Kubernetes Cluster"
        subgraph "Custom Resources"
            BC[Bind9Cluster]
            BI[Bind9Instance]
            DZ[DNSZone]
            AR[ARecord]
            CR[CNAMERecord]
            MR[MXRecord]
            TR[TXTRecord]
        end

        subgraph "Bindy Operator (Rust)"
            WA[Watch API<br/>kube-rs]

            subgraph "Reconcilers"
                BCR[Bind9Cluster<br/>Reconciler]
                BIR[Bind9Instance<br/>Reconciler]
                DZR[DNSZone<br/>Reconciler]
                RR[Record<br/>Reconcilers]
            end

            subgraph "Core Components"
                BM[Bind9Manager<br/>RNDC Client]
                RES[Resource<br/>Builders]
            end
        end

        subgraph "Kubernetes Resources"
            DEP[Deployments]
            CM[ConfigMaps]
            SEC[Secrets]
            SVC[Services]
        end

        subgraph "BIND9 Pods"
            P1[Primary DNS<br/>us-east]
            P2[Secondary DNS<br/>us-west]
            P3[Secondary DNS<br/>eu]
        end
    end

    subgraph "External"
        CLI[DNS Clients]
    end

    %% Custom Resource relationships
    BC -.inherits.-> BI
    BI -.references.-> DZ
    DZ -.contains.-> AR
    DZ -.contains.-> CR
    DZ -.contains.-> MR
    DZ -.contains.-> TR

    %% Watch relationships
    BC --> WA
    BI --> WA
    DZ --> WA
    AR --> WA
    CR --> WA
    MR --> WA
    TR --> WA

    %% Reconciler routing
    WA --> BCR
    WA --> BIR
    WA --> DZR
    WA --> RR

    %% Component interactions
    BCR --> RES
    BIR --> RES
    DZR --> BM
    RR --> BM

    %% K8s resource creation
    RES --> DEP
    RES --> CM
    RES --> SEC
    RES --> SVC

    %% RNDC communication
    BM -.RNDC:9530.-> P1
    BM -.RNDC:9530.-> P2
    BM -.RNDC:9530.-> P3

    %% DNS deployment
    DEP --> P1
    DEP --> P2
    DEP --> P3
    CM --> P1
    CM --> P2
    CM --> P3
    SEC --> P1

    %% Zone transfers
    P1 -.AXFR/IXFR.-> P2
    P1 -.AXFR/IXFR.-> P3

    %% DNS queries
    CLI -.DNS:53.-> P1
    CLI -.DNS:53.-> P2
    CLI -.DNS:53.-> P3

    style BC fill:#e1f5ff
    style BI fill:#e1f5ff
    style DZ fill:#e1f5ff
    style AR fill:#fff4e1
    style CR fill:#fff4e1
    style MR fill:#fff4e1
    style TR fill:#fff4e1
    style WA fill:#f0f0f0
    style BCR fill:#d4e8d4
    style BIR fill:#d4e8d4
    style DZR fill:#d4e8d4
    style RR fill:#d4e8d4
    style BM fill:#ffd4d4
    style RES fill:#ffd4d4
```

## Rust Component Architecture

```mermaid
graph TB
    subgraph "Main Process"
        MAIN[main.rs<br/>Tokio Runtime]
    end

    subgraph "CRD Definitions (src/crd.rs)"
        CRD_BC[Bind9Cluster]
        CRD_BI[Bind9Instance]
        CRD_DZ[DNSZone]
        CRD_REC[Record Types<br/>A, AAAA, CNAME,<br/>MX, NS, TXT,<br/>SRV, CAA]
    end

    subgraph "Reconcilers (src/reconcilers/)"
        RECON_BC[bind9cluster.rs]
        RECON_BI[bind9instance.rs]
        RECON_DZ[dnszone.rs]
        RECON_REC[records.rs]
    end

    subgraph "BIND9 Management (src/bind9/)"
        BM_MGR[Bind9Manager]
        BM_KEY[RndcKeyData]
        BM_CMD[Zone Operations<br/>HTTP API & RNDC<br/>addzone, delzone,<br/>reload, freeze,<br/>thaw, notify]
    end

    subgraph "Resource Builders (src/bind9_resources.rs)"
        RB_DEP[build_deployment]
        RB_CM[build_configmap]
        RB_SVC[build_service]
        RB_VOL[build_volumes]
        RB_POD[build_podspec]
    end

    subgraph "External Dependencies"
        KUBE[kube-rs<br/>Kubernetes Client]
        RNDC[rndc-rs<br/>RNDC Protocol]
        TOKIO[tokio<br/>Async Runtime]
        SERDE[serde<br/>Serialization]
    end

    %% Main process spawns reconcilers
    MAIN --> RECON_BC
    MAIN --> RECON_BI
    MAIN --> RECON_DZ
    MAIN --> RECON_REC

    %% Reconcilers use CRD types
    RECON_BC -.uses.-> CRD_BC
    RECON_BI -.uses.-> CRD_BI
    RECON_DZ -.uses.-> CRD_DZ
    RECON_REC -.uses.-> CRD_REC

    %% Reconcilers call managers
    RECON_BI --> RB_DEP
    RECON_BI --> RB_CM
    RECON_BI --> RB_SVC
    RECON_DZ --> BM_MGR
    RECON_REC --> BM_MGR

    %% Resource builders use components
    RB_DEP --> RB_POD
    RB_DEP --> RB_VOL
    RB_CM --> RB_VOL

    %% BIND9 manager components
    BM_MGR --> BM_KEY
    BM_MGR --> BM_CMD

    %% External dependencies
    MAIN --> TOKIO
    RECON_BC --> KUBE
    RECON_BI --> KUBE
    RECON_DZ --> KUBE
    RECON_REC --> KUBE
    BM_CMD --> RNDC
    CRD_BC --> SERDE
    CRD_BI --> SERDE
    CRD_DZ --> SERDE
    CRD_REC --> SERDE

    style MAIN fill:#e1f5ff
    style CRD_BC fill:#d4e8d4
    style CRD_BI fill:#d4e8d4
    style CRD_DZ fill:#d4e8d4
    style CRD_REC fill:#d4e8d4
    style RECON_BC fill:#fff4e1
    style RECON_BI fill:#fff4e1
    style RECON_DZ fill:#fff4e1
    style RECON_REC fill:#fff4e1
    style BM_MGR fill:#ffd4d4
    style BM_KEY fill:#ffd4d4
    style BM_CMD fill:#ffd4d4
    style RB_DEP fill:#e8d4f8
    style RB_CM fill:#e8d4f8
    style RB_SVC fill:#e8d4f8
    style RB_VOL fill:#e8d4f8
    style RB_POD fill:#e8d4f8
```

## DNS Record Creation Data Flow

```mermaid
sequenceDiagram
    participant User
    participant K8sAPI as Kubernetes API
    participant Watch as Watch Stream
    participant RecRec as Record Reconciler
    participant ZoneRec as DNSZone Reconciler
    participant BindMgr as Bind9Manager
    participant Primary as Primary BIND9
    participant Secondary as Secondary BIND9
    participant Client as DNS Client

    Note over User,Client: Record Creation Flow

    User->>K8sAPI: kubectl apply -f arecord.yaml
    K8sAPI->>K8sAPI: Validate CRD schema
    K8sAPI->>K8sAPI: Store in etcd
    K8sAPI-->>User: ARecord created

    K8sAPI->>Watch: Event: ARecord Added
    Watch->>RecRec: Trigger reconciliation

    RecRec->>K8sAPI: Get referenced DNSZone
    K8sAPI-->>RecRec: DNSZone details

    RecRec->>K8sAPI: Get Bind9Instance (via clusterRef)
    K8sAPI-->>RecRec: Bind9Instance details

    RecRec->>K8sAPI: Get RNDC Secret
    K8sAPI-->>RecRec: RNDC key data

    RecRec->>BindMgr: Call add_a_record()
    Note over BindMgr: Currently placeholder<br/>Will use nsupdate
    BindMgr-->>RecRec: Ok(())

    RecRec->>BindMgr: Call reload_zone(zone_name)
    BindMgr->>Primary: RNDC reload zone
    activate Primary
    Primary->>Primary: Reload zone file
    Primary-->>BindMgr: Success
    deactivate Primary
    BindMgr-->>RecRec: Zone reloaded

    RecRec->>K8sAPI: Update ARecord status
    K8sAPI-->>RecRec: Status updated

    Note over Primary,Secondary: Zone Transfer (AXFR/IXFR)

    Primary->>Secondary: NOTIFY (zone updated)
    activate Secondary
    Secondary->>Primary: SOA query (check serial)
    Primary-->>Secondary: SOA record

    alt Serial increased
        Secondary->>Primary: IXFR/AXFR request
        Primary-->>Secondary: Zone transfer
        Secondary->>Secondary: Update zone
    else Serial unchanged
        Secondary->>Secondary: No update needed
    end
    deactivate Secondary

    Note over Client,Secondary: DNS Query

    Client->>Secondary: DNS query (www.example.com A?)
    activate Secondary
    Secondary->>Secondary: Lookup in zone
    Secondary-->>Client: Answer: 192.0.2.1
    deactivate Secondary
```

## Zone Creation and Synchronization Flow

```mermaid
stateDiagram-v2
    [*] --> ZoneCreated: User creates DNSZone

    ZoneCreated --> Validating: Operator watches event

    Validating --> ValidatingInstance: Validate zone spec
    ValidatingInstance --> ValidatingCluster: Find Bind9Instance
    ValidatingCluster --> GeneratingConfig: Find Bind9Cluster

    GeneratingConfig --> CreatingRNDCKey: Generate zone config
    CreatingRNDCKey --> StoringSecret: Generate RNDC key
    StoringSecret --> AddingZone: Store in Secret

    AddingZone --> ConnectingRNDC: Call rndc addzone
    ConnectingRNDC --> ExecutingCommand: Connect via port 9530
    ExecutingCommand --> VerifyingZone: Execute addzone command

    VerifyingZone --> Ready: Verify zone exists
    Ready --> [*]: Update status to Ready

    ValidatingInstance --> Failed: Instance not found
    ValidatingCluster --> Failed: Cluster not found
    AddingZone --> Failed: RNDC command failed
    ConnectingRNDC --> Failed: Connection failed

    Failed --> [*]: Update status conditions

    note right of GeneratingConfig
        Creates zone with:
        - SOA record
        - Default TTL
        - Zone file path
    end note

    note right of AddingZone
        Uses RNDC protocol:
        addzone example.com
        '{ type master;
           file "zones/example.com"; }'
    end note
```

## Primary to Secondary Zone Transfer Flow

```mermaid
sequenceDiagram
    participant Ctl as Bindy Operator
    participant Pri as Primary BIND9<br/>(us-east)
    participant Sec1 as Secondary BIND9<br/>(us-west)
    participant Sec2 as Secondary BIND9<br/>(eu)

    Note over Ctl,Sec2: Initial Zone Setup

    Ctl->>Pri: RNDC addzone example.com
    activate Pri
    Pri->>Pri: Create zone file
    Pri-->>Ctl: Zone added
    deactivate Pri

    Ctl->>Sec1: RNDC addzone example.com (type secondary)
    activate Sec1
    Sec1->>Sec1: Configure as secondary
    Sec1-->>Ctl: Zone added as secondary
    deactivate Sec1

    Ctl->>Sec2: RNDC addzone example.com (type secondary)
    activate Sec2
    Sec2->>Sec2: Configure as secondary
    Sec2-->>Ctl: Zone added as secondary
    deactivate Sec2

    Note over Pri,Sec2: Initial Zone Transfer

    Sec1->>Pri: SOA query (get serial)
    Pri-->>Sec1: SOA serial=2024010101
    Sec1->>Pri: AXFR request (full transfer)
    Pri-->>Sec1: Complete zone data
    Sec1->>Sec1: Write zone file

    Sec2->>Pri: SOA query (get serial)
    Pri-->>Sec2: SOA serial=2024010101
    Sec2->>Pri: AXFR request (full transfer)
    Pri-->>Sec2: Complete zone data
    Sec2->>Sec2: Write zone file

    Note over Ctl,Sec2: Record Update

    Ctl->>Ctl: User adds new ARecord
    Ctl->>Pri: Update zone + reload
    activate Pri
    Pri->>Pri: Update zone file
    Pri->>Pri: Increment serial to 2024010102
    Pri-->>Ctl: Zone reloaded
    deactivate Pri

    Note over Pri,Sec2: NOTIFY and Incremental Transfer

    Pri->>Sec1: NOTIFY (zone updated)
    Pri->>Sec2: NOTIFY (zone updated)

    activate Sec1
    Sec1->>Pri: SOA query (check serial)
    Pri-->>Sec1: SOA serial=2024010102
    Sec1->>Sec1: Compare: 2024010102 > 2024010101
    Sec1->>Pri: IXFR request (incremental)
    Pri-->>Sec1: Only changed records
    Sec1->>Sec1: Apply changes
    Sec1-->>Pri: ACK
    deactivate Sec1

    activate Sec2
    Sec2->>Pri: SOA query (check serial)
    Pri-->>Sec2: SOA serial=2024010102
    Sec2->>Sec2: Compare: 2024010102 > 2024010101
    Sec2->>Pri: IXFR request (incremental)
    Pri-->>Sec2: Only changed records
    Sec2->>Sec2: Apply changes
    Sec2-->>Pri: ACK
    deactivate Sec2

    Note over Pri,Sec2: All zones synchronized
```

## Reconciliation Loop

```mermaid
flowchart TD
    Start([Watch Event Received]) --> CheckType{Event Type?}

    CheckType -->|Added/Modified| GetResource[Get Resource from API]
    CheckType -->|Deleted| Cleanup[Run Cleanup Logic]
    CheckType -->|Restarted| RefreshAll[Refresh All Resources]

    GetResource --> CheckGen{observedGeneration<br/>== metadata.generation?}
    CheckGen -->|Yes| SkipRecon[Skip: Already reconciled]
    CheckGen -->|No| ValidateSpec[Validate Spec]

    ValidateSpec --> CheckValid{Valid?}
    CheckValid -->|No| UpdateFailed[Update Status: Failed]
    CheckValid -->|Yes| Reconcile[Execute Reconciliation]

    Reconcile --> ReconcileResult{Success?}
    ReconcileResult -->|Yes| UpdateReady[Update Status: Ready]
    ReconcileResult -->|No| CheckRetry{Retryable?}

    CheckRetry -->|Yes| Requeue[Requeue with backoff]
    CheckRetry -->|No| UpdateError[Update Status: Error]

    UpdateReady --> UpdateGen[Update observedGeneration]
    UpdateError --> Requeue
    UpdateFailed --> End

    UpdateGen --> End([Done])
    Cleanup --> End
    RefreshAll --> End
    SkipRecon --> End
    Requeue --> End

    style Start fill:#e1f5ff
    style End fill:#e1f5ff
    style Reconcile fill:#d4e8d4
    style UpdateReady fill:#d4f8d4
    style UpdateError fill:#f8d4d4
    style UpdateFailed fill:#f8d4d4
    style CheckType fill:#fff4e1
    style CheckGen fill:#fff4e1
    style CheckValid fill:#fff4e1
    style ReconcileResult fill:#fff4e1
    style CheckRetry fill:#fff4e1
```

## RNDC Protocol Communication

```mermaid
sequenceDiagram
    participant BM as Bind9Manager<br/>(Rust)
    participant RC as RNDC Client<br/>(rndc-rs)
    participant Net as TCP Socket<br/>:9530
    participant BIND as BIND9 Server<br/>(rndc daemon)

    Note over BM,BIND: RNDC Key Setup (One-time)

    BM->>BM: generate_rndc_key()
    BM->>BM: Create HMAC-SHA256 key
    BM->>BM: Store in K8s Secret

    Note over BM,BIND: RNDC Command Execution

    BM->>RC: new(server, algorithm, secret)
    RC->>RC: Parse RNDC key
    RC->>RC: Prepare TSIG signature

    BM->>RC: rndc_command("reload zone")
    RC->>Net: Connect to server:9530
    Net->>BIND: TCP handshake

    RC->>RC: Create RNDC message
    RC->>RC: Sign with HMAC-SHA256
    RC->>Net: Send signed message
    Net->>BIND: Forward RNDC message

    activate BIND
    BIND->>BIND: Verify TSIG signature
    BIND->>BIND: Execute: reload zone
    BIND->>BIND: Reload zone file
    BIND->>Net: Response + TSIG
    deactivate BIND

    Net->>RC: Receive response
    RC->>RC: Verify response TSIG
    RC->>RC: Parse result
    RC-->>BM: Ok(result.text)

    alt Authentication Failed
        BIND-->>Net: Error: TSIG verification failed
        Net-->>RC: Error response
        RC-->>BM: Err("RNDC authentication failed")
    end

    alt Command Failed
        BIND-->>Net: Error: Zone not found
        Net-->>RC: Error response
        RC-->>BM: Err("Zone not found")
    end
```

## Multi-Cluster Deployment

```mermaid
graph TB
    subgraph "Cluster: us-east-1"
        BC1[Bind9Cluster:<br/>production-dns]
        BI1[Bind9Instance:<br/>primary-dns]
        DZ1[DNSZone:<br/>example.com]
        P1[Primary BIND9<br/>172.16.1.10]

        BC1 -.-> BI1
        BI1 -.-> DZ1
        DZ1 --> P1
    end

    subgraph "Cluster: us-west-2"
        BC2[Bind9Cluster:<br/>production-dns]
        BI2[Bind9Instance:<br/>secondary-dns-west]
        DZ2[DNSZone:<br/>example.com]
        S1[Secondary BIND9<br/>172.16.2.10]

        BC2 -.-> BI2
        BI2 -.-> DZ2
        DZ2 --> S1
    end

    subgraph "Cluster: eu-central-1"
        BC3[Bind9Cluster:<br/>production-dns]
        BI3[Bind9Instance:<br/>secondary-dns-eu]
        DZ3[DNSZone:<br/>example.com]
        S2[Secondary BIND9<br/>172.16.3.10]

        BC3 -.-> BI3
        BI3 -.-> DZ3
        DZ3 --> S2
    end

    P1 -.AXFR/IXFR.-> S1
    P1 -.AXFR/IXFR.-> S2

    LB[Global Load Balancer<br/>GeoDNS]

    LB -.US Traffic.-> P1
    LB -.US Traffic.-> S1
    LB -.EU Traffic.-> S2

    style BC1 fill:#e1f5ff
    style BC2 fill:#e1f5ff
    style BC3 fill:#e1f5ff
    style BI1 fill:#d4e8d4
    style BI2 fill:#d4e8d4
    style BI3 fill:#d4e8d4
    style P1 fill:#ffd4d4
    style S1 fill:#fff4e1
    style S2 fill:#fff4e1
    style LB fill:#f0f0f0
```

## Related Documentation

- [Architecture Overview](./architecture.md) - Detailed text description
- [Protocol Reference](./architecture-protocols.md) - RNDC and HTTP API protocol details
- [CRD Specifications](./crds.md) - Custom resource definitions
