# GitOps for DNS: Why the World Needs Declarative DNS Management

*Or: How I Built a Kubernetes Operator to Stop Scripting DNS Like It's 1999*

---

## The Problem: DNS in the Age of GitOps

Picture this: You're managing a modern Kubernetes platform. Your infrastructure is fully declarative, GitOps pipelines deploying services, ConfigMaps, secrets, everything versioned in Git. Your deployments are audited, tracked, and reproducible.

Then you need to make a DNS change.

Suddenly, you're SSH-ing into a server, hand-editing zone files, running scripts, reloading named, crossing your fingers, and hoping you didn't typo a semicolon that brings down your entire domain.

**Why is DNS still stuck in the scripting dark ages?**

In 2025, when we have infrastructure-as-code for everything from Terraform to Kubernetes, DNS remains stubbornly manual. We have:
- Shell scripts that edit BIND zone files with `sed` and `awk`
- Ansible playbooks that template configuration and pray
- Python scripts that parse zone files (hopefully correctly)
- Manual `nsupdate` commands typed at 2 AM during outages
- Zero audit trail for who changed what and when
- No rollback mechanism when things go wrong

**This is insane.**

A small DNS misconfiguration can:
- Take down an entire domain
- Break authentication flows
- Cause cascading service failures
- Violate compliance requirements (SOX, PCI-DSS)
- Cost millions in downtime

And yet, we're still scripting DNS like it's 1999.

---

## The Vision: DNS Deserves GitOps Too

What if DNS management looked like this?

```yaml
# dns/zones/example-com.yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  clusterRef: prod-dns

---
# dns/records/www.yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
spec:
  zoneRef: example-com
  ipv4Address: "192.0.2.1"
```

Then:
```bash
git commit -m "Add www A record"
git push
# GitOps pipeline applies changes
kubectl apply -f dns/
```

**That's it.**

No SSH. No zone file editing. No manual reloads. Just:
1. Declare what you want
2. Commit to Git
3. Let the system make it so

Full audit trail, rollback capabilities, peer review via pull requests, and Kubernetes-native status tracking.

**This is GitOps-driven DNS. This is `bindy`.**

---

## Introducing `bindy`: Kubernetes-Native DNS Management

[`bindy`](https://github.com/firestoned/bindy) is a high-performance Kubernetes operator that brings declarative DNS management to BIND9. It lets you manage DNS zones and records as native Kubernetes Custom Resources, following GitOps principles.

**What makes `bindy` different:**

1. **True Declarative Management** - No scripts, no zone file editing, just YAML
2. **Event-Driven Architecture** - Watches Kubernetes API, reacts to changes instantly
3. **HTTP REST API Sidecar** - `bindcar` sidecar translates HTTP to RNDC, simplifying communication
4. **Kubernetes-Native** - Status conditions, events, RBAC, ServiceAccount authentication, all built-in
5. **Written in Rust** - Memory-safe, high-performance, minimal overhead (both controller and sidecar)
6. **Production-Ready** - SOX, NIST, CIS compliant, SLSA Level 3 provenance, signed releases

---

## How `bindy` Works: Architecture

`bindy` follows a three-tier architecture with a critical sidecar pattern for BIND9 communication.

### The Sidecar Pattern: `bindcar` HTTP API

Before diving into the three tiers, it's essential to understand how `bindy` communicates with BIND9. Rather than implementing the complex RNDC protocol directly in the controller, `bindy` uses a **sidecar container pattern** with `bindcar`:

```
┌─────────────────────────────────────────────────────────┐
│                     BIND9 Pod                           │
│                                                         │
│  ┌──────────────┐          ┌──────────────────────┐    │
│  │ BIND9        │          │ bindcar Sidecar      │    │
│  │ Container    │          │                      │    │
│  │              │◀─────────│ HTTP REST API        │    │
│  │ Port 53      │  rndc    │ Port 8080            │    │
│  │ (DNS)        │  local   │                      │    │
│  │              │          │ - Receives HTTP      │    │
│  │ Port 9530     │          │ - Executes rndc      │    │
│  │ (RNDC)       │          │ - Manages zones      │    │
│  │              │          │                      │    │
│  │ /var/cache/  │◀────────▶│ /var/cache/bind      │    │
│  │ bind         │  shared  │ (shared volume)      │    │
│  └──────────────┘  volume  └──────────────────────┘    │
│         ▲                            ▲                 │
└─────────┼────────────────────────────┼─────────────────┘
          │                            │
          │ DNS queries                │ HTTP REST API
          │ (UDP/TCP 53)               │ (TCP 8080)
          │                            │
    DNS Clients              bindy Controller
```

**Why the sidecar pattern?**

1. **Simplified protocol**: `bindy` uses simple HTTP REST instead of implementing the binary RNDC protocol
2. **Better error handling**: Structured JSON responses with detailed error messages
3. **Authentication**: Uses Kubernetes ServiceAccount tokens (Bearer authentication)
4. **Local RNDC**: `bindcar` executes `rndc` commands locally (localhost:9530) - no network complexity
5. **Shared storage**: Both containers access `/var/cache/bind` for zone file operations
6. **Language-agnostic**: Any HTTP client can interact with BIND9 via `bindcar`

**Communication flow:**
```
kubectl apply -f dnszone.yaml
    ↓
Kubernetes API (etcd)
    ↓ watch event
bindy Controller
    ↓ HTTP POST /api/v1/zones (port 8080)
bindcar Sidecar
    ↓ writes zone file to /var/cache/bind
    ↓ executes: rndc addzone example.com
BIND9 Container
    ↓ loads zone, serves DNS
DNS Clients
```

Now let's see how this sidecar pattern fits into the three-tier architecture:

### 1. Infrastructure Layer: Bind9Cluster

Define your DNS infrastructure declaratively:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: prod-dns
spec:
  version: "9.18"
  global:
    recursion: false
    dnssec: true
  primary:
    replicas: 2
  secondary:
    replicas: 3
    zones:
      - us-west-2
      - eu-west-1
```

This creates:
- 2 primary BIND9 instances (for zone management)
- 3 secondary instances (for zone transfers and failover)
- Automatic TSIG key generation for authenticated zone transfers
- DNSSEC key management
- Kubernetes Deployments, Services, ConfigMaps, Secrets (all auto-managed)

### 2. Zone Layer: DNSZone

Create DNS zones with SOA records:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  clusterRef: prod-dns
  soaRecord:
    primaryNameServer: ns1.example.com
    adminEmail: hostmaster.example.com
    ttl: 3600
```

The controller:
1. Selects a primary Bind9Instance from the cluster
2. Sends HTTP POST request to `bindcar` API sidecar (port 8080)
3. `bindcar` writes zone file to `/var/cache/bind` (shared volume)
4. `bindcar` executes RNDC `addzone` command locally (localhost:9530)
5. BIND9 loads the zone and begins serving DNS queries
6. Controller updates resource status with zone placement

**No zone files edited manually. No server SSH. Just Kubernetes API and REST.**

### 3. Record Layer: DNS Records

Add DNS records as individual resources:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
spec:
  zoneRef: example-com
  ipv4Address: "192.0.2.1"
  ttl: 300

---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: mail
spec:
  zoneRef: example-com
  priority: 10
  mailServer: mail.example.com
```

`bindy` reconciles these into the zone via the `bindcar` HTTP API.

**Workflow:**
```
Kubernetes API (etcd)
    ↓ Watch events
bindy Controller (Rust)
    ↓ HTTP REST API (port 8080)
bindcar Sidecar
    ↓ RNDC protocol (localhost:9530)
BIND9 Pods
    ↓ Zone transfers (AXFR/IXFR)
Secondary Instances (multi-region)
```

---

## The Secret Sauce: `bindcar` HTTP API Sidecar

One of `bindy`'s key innovations is the `bindcar` sidecar - a lightweight Rust HTTP server that runs alongside every BIND9 instance. This architectural choice dramatically simplifies DNS management:

**Why not use RNDC directly from the controller?**

RNDC (Remote Name Daemon Control) is BIND9's native management protocol, but it has significant challenges:
- **Binary protocol**: Complex to implement correctly
- **TSIG authentication**: Requires managing shared secrets and HMAC signing
- **Network overhead**: Controller needs direct network access to every BIND9 pod
- **Error handling**: Cryptic error messages that are hard to parse
- **State management**: Complex connection pooling and retry logic

**How `bindcar` solves this:**

Instead of implementing RNDC in the controller, `bindcar` acts as a translation layer:

```
┌──────────────────────┐         ┌──────────────────────┐
│ bindy Controller     │         │ bindcar Sidecar      │
│                      │         │                      │
│ Simple HTTP client   │  HTTP   │ HTTP REST API        │
│ reqwest (Rust)       │────────▶│ Axum server (Rust)   │
│                      │ POST    │                      │
│ ServiceAccount token │ 8080    │ Validates auth       │
│ Bearer auth          │         │ Executes rndc        │
│                      │         │ Returns JSON         │
└──────────────────────┘         └──────────────────────┘
                                            │
                                            │ rndc (local)
                                            ▼
                                  ┌──────────────────────┐
                                  │ BIND9                │
                                  │ localhost:9530        │
                                  │ No network exposure  │
                                  └──────────────────────┘
```

**Key advantages:**

1. **Simplified controller**: `bindy` uses standard HTTP - no custom protocol implementation
2. **Better errors**: Structured JSON responses with detailed error messages and stack traces
3. **Native Kubernetes auth**: Uses ServiceAccount tokens instead of managing TSIG keys
4. **Security**: RNDC port (9530) only exposed to localhost, not the cluster network
5. **Observability**: Easy to add Prometheus metrics, request logging, and tracing
6. **Language-agnostic**: Any tool that speaks HTTP can manage BIND9 (curl, Python, Go, etc.)
7. **Testability**: API can be tested independently of the controller

**Example HTTP request:**
```bash
POST http://bind9-primary:8080/api/v1/zones
Authorization: Bearer <k8s-serviceaccount-token>
Content-Type: application/json

{
  "zoneName": "example.com",
  "zoneType": "primary",
  "zoneContent": "$TTL 3600\n@ IN SOA ns1.example.com. ...",
  "updateKeyName": "bindcar-operator"
}

# Response:
{
  "success": true,
  "message": "Zone example.com created successfully",
  "details": "zone example.com/IN: loaded serial 2025010101"
}
```

**Performance:**
- Startup time: <500ms
- Memory footprint: ~30MB per sidecar
- Zone creation: ~600ms (50ms HTTP overhead + 550ms BIND9)
- Written in Rust: Memory-safe, no garbage collection pauses

The `bindcar` sidecar is open-source, MIT licensed, and available at [github.com/firestoned/bindcar](https://github.com/firestoned/bindcar).

---

## Multi-Region DNS: Global Distribution Made Easy

`bindy` supports primary-secondary architecture with automatic zone transfers:

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: global-dns
spec:
  primary:
    replicas: 2
    region: us-east-1
  secondary:
    replicas: 4
    zones:
      - region: us-west-2
        replicas: 2
      - region: eu-west-1
        replicas: 2
```

**What happens:**
1. Primary instances host authoritative zones (us-east-1)
2. Zone changes trigger NOTIFY to secondaries
3. Secondaries perform zone transfers (AXFR for full, IXFR for incremental)
4. Automatic SOA serial number management
5. GeoDNS or Anycast routes clients to nearest instance

**Result:** Global DNS with automatic failover, geographic distribution, and no manual synchronization.

---

## GitOps Workflow: DNS Changes via Pull Requests

Traditional DNS change workflow:
1. Someone requests DNS change via ticket
2. DNS admin SSH to server
3. Edit zone file manually
4. Run reload script
5. Hope it works
6. No audit trail of what changed

**`bindy` GitOps workflow:**

1. Developer opens PR:
   ```yaml
   # add-api-record.yaml
   apiVersion: bindy.firestoned.io/v1beta1
   kind: ARecord
   metadata:
     name: api
   spec:
     zoneRef: example-com
     ipv4Address: "192.0.2.50"
   ```

2. Team reviews PR (peer review, approval gates)

3. PR merged to main branch

4. FluxCD GitOps pipeline applies changes:
   ```bash
   kubectl apply -f dns/
   ```

5. `bindy` controller reconciles:
   - Detects new ARecord resource
   - Sends HTTP POST to `bindcar` sidecar
   - `bindcar` executes RNDC update command
   - Updates status condition to "Ready"
   - Emits Kubernetes event

6. Full audit trail:
   - Git commit history (who, what, when, why)
   - PR review thread (approval, discussion)
   - Kubernetes events (reconciliation status)
   - `bindcar` API logs (HTTP requests)
   - BIND9 logs (RNDC commands executed)

**Rollback:**
```bash
git revert <commit-hash>
git push
# FluxCD pipeline reverts DNS change
```

---

## Coupling `bindy` with ConfigHub: A Complete DNS Solution

While `bindy` handles the declarative DNS management layer, pairing it with [ConfigHub](https://confighub.com) creates a complete enterprise DNS solution:

**`bindy`'s Role:**
- Kubernetes-native DNS management
- BIND9 infrastructure provisioning
- Zone and record reconciliation
- GitOps integration

**ConfigHub's Role:**
- Multi-environment configuration management (dev, staging, prod)
- Secret management (TSIG keys, DNSSEC keys)
- Configuration versioning and rollback
- Compliance and audit reporting
- Team collaboration and approval workflows

**Combined Architecture:**

```
ConfigHub (Configuration Source of Truth)
    ↓ Configuration injection
GitOps Repository (FluxCD)
    ↓ Sync to cluster
Kubernetes API
    ↓ Watch events
bindy Controller
    ↓ HTTP REST API
bindcar Sidecar
    ↓ RNDC protocol (local)
BIND9 Infrastructure
```

**Example Workflow:**

1. **ConfigHub stores environment-specific DNS configs:**
   ```yaml
   # ConfigHub: prod environment
   dns:
     zones:
       example.com:
         records:
           - name: www
             type: A
             value: "192.0.2.1"
           - name: api
             type: CNAME
             value: "api-prod.example.com"
   ```

2. **ConfigHub injects configs into GitOps repo** (per environment)

3. **FluxCD syncs to Kubernetes:**
   ```yaml
   apiVersion: bindy.firestoned.io/v1beta1
   kind: ARecord
   metadata:
     name: www
     namespace: prod-dns
   spec:
     zoneRef: example-com
     ipv4Address: "192.0.2.1"
   ```

4. **`bindy` reconciles to BIND9**

**Benefits:**
- **Single source of truth**: ConfigHub manages all DNS configs
- **Environment promotion**: Dev → Staging → Prod with approval gates
- **Secret rotation**: ConfigHub handles TSIG/DNSSEC key lifecycle
- **Compliance**: ConfigHub provides audit reports, RBAC, approval workflows
- **Disaster recovery**: ConfigHub backs up all configurations
- **Team collaboration**: ConfigHub UI for non-Kubernetes users

This combination gives you:
- **Declarative DNS** (`bindy`)
- **Configuration management** (ConfigHub)
- **GitOps automation** (FluxCD)
- **Enterprise compliance** (ConfigHub audit trails)

---

## Real-World Use Cases

### 1. Platform Engineering: Multi-Tenant Kubernetes DNS

**Scenario:** You run a Kubernetes platform for 50+ teams. Each team needs their own DNS zones for services.

**Without `bindy`:**
- Central DNS team bottleneck
- Manual zone provisioning (hours to days)
- No self-service for teams
- Error-prone scripting

**With `bindy`:**
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: team-payments
  namespace: team-payments
spec:
  zoneName: payments.internal.example.com
  clusterRef: platform-dns
```

Teams self-service provision zones. Platform team manages the Bind9Cluster. RBAC controls who can create zones.

### 2. Replacing External DNS Providers

**Scenario:** Paying $50K/year to external DNS provider. Want control and cost savings.

**With `bindy`:**
- Self-hosted BIND9 on Kubernetes
- Multi-region distribution
- DNSSEC built-in
- Full control over DNS data
- Cost: Infrastructure only (savings: ~80%)

### 3. Compliance-Heavy Environments (Banking, Healthcare)

**Scenario:** SOX 404 requires audit trail for all DNS changes. PCI-DSS requires access controls.

**With `bindy` + ConfigHub:**
- All DNS changes in Git (commit history = audit trail)
- PR approval workflow (segregation of duties)
- Kubernetes RBAC (access controls)
- ConfigHub compliance reports (SOX, PCI-DSS evidence)
- Immutable infrastructure (delete and recreate, not edit in place)

### 4. Multi-Region Global Services

**Scenario:** SaaS product with users in US, EU, APAC. Need low-latency DNS resolution.

**With `bindy`:**
```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: global-saas-dns
spec:
  primary:
    replicas: 2
    region: us-east-1
  secondary:
    zones:
      - region: us-west-2
      - region: eu-west-1
      - region: ap-southeast-1
```

Automatic zone transfers, GeoDNS routing, transparent failover.

---

## Performance & Production Readiness

**`bindy` is built for production:**

- **Startup Time:** <1 second
- **Memory Footprint:** ~50MB baseline
- **Zone Creation:** <500ms per zone
- **Record Addition:** <200ms per record
- **Scalability:** Thousands of zones per instance

**Compliance & Security:**
- SOX 404 controls documented
- NIST 800-53 94% compliant
- CIS Kubernetes Level 1 (84% compliant)
- SLSA Level 3 build provenance
- Signed releases (Cosign, keyless)
- SBOM (CycloneDX) with every release
- Daily vulnerability scanning
- TSIG-authenticated RNDC communication
- Non-root containers

**Language:** Rust (memory-safe, high-performance, no garbage collection pauses)

---

## Getting Started

### Installation (3 Steps):

```bash
# 1. Install CRDs
kubectl create -f https://github.com/firestoned/bindy/releases/latest/download/crds.yaml

# 2. Deploy bindy controller
kubectl apply -f https://github.com/firestoned/bindy/releases/latest/download/bindy.yaml

# 3. Create your first DNS cluster
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: my-dns
spec:
  primary:
    replicas: 1
EOF
```

### Create a Zone and Record:

```bash
# Create zone
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: example-com
spec:
  zoneName: example.com
  clusterRef: my-dns
EOF

# Add A record
kubectl apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: www
spec:
  zoneRef: example-com
  ipv4Address: "192.0.2.1"
EOF
```

### Verify:

```bash
kubectl get dnszones
kubectl get arecords
kubectl describe arecord www
```

**That's it. Your DNS is live.**

---

## Call to Action

### 1. Try `bindy` Today

- **bindy Controller GitHub:** [github.com/firestoned/bindy](https://github.com/firestoned/bindy)
- **bindcar Sidecar GitHub:** [github.com/firestoned/bindcar](https://github.com/firestoned/bindcar)
- **Documentation:** [firestoned.github.io/bindy](https://firestoned.github.io/bindy)
- **Quick Start Guide:** Get running in 5 minutes
- **Supported Record Types:** A, AAAA, CNAME, MX, TXT, NS, SRV, CAA

### 2. Join the Community

- **Star the repo** if you believe DNS deserves GitOps
- **Report issues** or request features on GitHub
- **Contribute** - `bindy` is MIT licensed, all contributions welcome
- **Share your use case** - I'd love to hear how you're using `bindy`

### 3. Spread the Word

If you think DNS should be declarative, share this post:
- Tweet about #GitOpsDNS and #bindy
- Share in your platform engineering communities
- Tell your DNS-managing friends (they need this)

### 4. Explore ConfigHub Integration

If you're managing multi-environment Kubernetes infrastructure:
- Check out [ConfigHub](https://confighub.com) for configuration management
- See how `bindy` + ConfigHub creates a complete DNS solution
- Reach out for enterprise support and compliance consulting

---

## Why This Matters

DNS is critical infrastructure. Downtime is measured in millions. Manual processes are error-prone. Scripts are fragile.

**We've solved declarative infrastructure for compute, storage, networking, and application deployment.**

**It's time DNS joined the GitOps revolution.**

`bindy` brings DNS into the modern era:
- Declarative management
- Version control
- Audit trails
- Peer review
- Automated reconciliation
- Kubernetes-native

**Stop scripting DNS. Start declaring it.**

---

*`bindy` and `bindcar` are open-source, MIT licensed, and built in Rust.*

*bindy Controller: [github.com/firestoned/bindy](https://github.com/firestoned/bindy)*

*bindcar Sidecar: [github.com/firestoned/bindcar](https://github.com/firestoned/bindcar)*

*Documentation:
- [bindy.firestoned.io](https://bindy.firestoned.io)*
- [bindcar.firestoned.io](https://bindcar.firestoned.io)*

---

**About the Author**

I'm Erick Bourgeois, a platform engineer with 20+ years of experience building infrastructure in highly regulated environments, Tier-1 banks. I built `bindy` because I was tired of SSH-ing into servers to edit zone files in 2025. DNS deserves better.

Follow me on:
- GitHub: [@firestoned](https://github.com/firestoned) and [@ebourgeois](https://github.com/ebourgeois)
- LinkedIn: [linkedin.com/in/erickbourgeois1978/](https://www.linkedin.com/in/erickbourgeois1978)
- Medium: [@erickbourgeois](https://medium.com/@erickbourgeois)

---

**Tags:** #GitOps #DNS #Kubernetes #BIND9 #InfrastructureAsCode #PlatformEngineering #Rust #DevOps #SRE #CloudNative #Declarative #ConfigManagement #Sidecar #HTTPApi #bindcar
