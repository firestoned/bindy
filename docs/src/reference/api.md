   Compiling bindy v0.3.4 (/Users/erick/dev/bindy)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.89s
     Running `target/debug/crddoc`
# API Reference

This document describes the Custom Resource Definitions (CRDs) provided by Bindy.

> **Note**: This file is AUTO-GENERATED from `src/crd.rs`
> DO NOT EDIT MANUALLY - Run `cargo run --bin crddoc` to regenerate

## Table of Contents

- [Zone Management](#zone-management)
  - [DNSZone](#dnszone)
- [DNS Records](#dns-records)
  - [ARecord](#arecord)
  - [AAAARecord](#aaaarecord)
  - [CNAMERecord](#cnamerecord)
  - [MXRecord](#mxrecord)
  - [NSRecord](#nsrecord)
  - [TXTRecord](#txtrecord)
  - [SRVRecord](#srvrecord)
  - [CAARecord](#caarecord)
- [Infrastructure](#infrastructure)
  - [Bind9Cluster](#bind9cluster)
  - [Bind9Instance](#bind9instance)

## Zone Management

### DNSZone

**API Version**: `bindy.firestoned.io/v1beta1`

DNSZone represents an authoritative DNS zone managed by BIND9. Each DNSZone defines a zone (e.g., example.com) with SOA record parameters. Can reference either a namespace-scoped Bind9Cluster or cluster-scoped ClusterBind9Provider.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `bind9InstancesFrom` | array | No | Select \`Bind9Instance\` resources to target for zone configuration using label selectors.  This field enables dynamic, label-based selection of DNS instances to serve this zone. Instances matching these selectors will automatically receive zone configuration from the \`DNSZone\` controller.  This follows the standard Kubernetes selector pattern used by Services, \`NetworkPolicies\`, and other resources for declarative resource association.  **IMPORTANT**: This is the **preferred** method for zone-instance association. It provides: - **Decoupled Architecture**: Zones select instances, not vice versa - **Zone Ownership**: Zone authors control which instances serve their zones - **Dynamic Scaling**: New instances matching labels automatically pick up zones - **Multi-Tenancy**: Zones can target specific instance groups (prod, staging, team-specific)  # Example: Target production primary instances  \`\`\`yaml apiVersion: bindy.firestoned.io/v1beta1 kind: DNSZone metadata:   name: example-com   namespace: dns-system spec:   zoneName: example.com   bind9InstancesFrom:     - selector:         matchLabels:           environment: production           bindy.firestoned.io/role: primary \`\`\`  # Example: Target instances by region and tier  \`\`\`yaml bind9InstancesFrom:   - selector:       matchLabels:         tier: frontend       atchExpressions:         - key: region           operator: In           values:             - us-east-1             - us-west-2 \`\`\`  # Selector Operators  - **In**: Label value must be in the specified values list - **\`NotIn\`**: Label value must NOT be in the specified values list - **Exists**: Label key must exist (any value) - **\`DoesNotExist\`**: Label key must NOT exist  # Use Cases  - **Environment Isolation**: Target only production instances (\`environment: production\`) - **Role-Based Selection**: Select only primary or secondary instances - **Geographic Distribution**: Target instances in specific regions - **Team Boundaries**: Select instances managed by specific teams - **Testing Zones**: Target staging instances for non-production zones  # Relationship with \`clusterRef\`  - **\`clusterRef\`**: Explicitly assigns zone to ALL instances in a cluster - **\`bind9InstancesFrom\`**: Dynamically selects specific instances using labels (more flexible)  You can use both approaches together - the zone will target the **union** of: - All instances in \`clusterRef\` cluster - Plus any additional instances matching \`bind9InstancesFrom\` selectors  # Event-Driven Architecture  The \`DNSZone\` controller watches both \`DNSZone\` and \`Bind9Instance\` resources. When labels change on either: 1. Controller re-evaluates label selector matching 2. Automatically configures zones on newly-matched instances 3. Removes zone configuration from instances that no longer match |
| `clusterRef` | string | No | Reference to a \`Bind9Cluster\` or \`ClusterBind9Provider\` to serve this zone.  When specified, this zone will be automatically configured on all \`Bind9Instance\` resources that belong to the referenced cluster. This provides a simple way to assign zones to entire clusters.  **Relationship with \`bind9_instances_from\`:** - If only \`cluster_ref\` is specified: Zone targets all instances in that cluster - If only \`bind9_instances_from\` is specified: Zone targets instances matching label selectors - If both are specified: Zone targets union of cluster instances AND label-selected instances  # Example  \`\`\`yaml spec:   clusterRef: production-dns  # Target all instances in this cluster   zoneName: example.com \`\`\` |
| `dnssecPolicy` | string | No | Override DNSSEC policy for this zone  Allows per-zone override of the cluster's global DNSSEC signing policy. If not specified, the zone inherits the DNSSEC configuration from the cluster's \`global.dnssec.signing.policy\`.  Use this to: - Disable signing for specific zones in a signing-enabled cluster - Use stricter security policies for sensitive zones - Test different signing algorithms on specific zones  # Example: Custom High-Security Policy  \`\`\`yaml apiVersion: bindy.firestoned.io/v1beta1 kind: DNSZone metadata:   name: secure-zone spec:   zoneName: secure.example.com   clusterRef: production-dns   dnssecPolicy: "high-security"  # Override cluster default \`\`\`  # Example: Disable Signing for One Zone  \`\`\`yaml dnssecPolicy: "none"  # Disable signing (cluster has signing enabled) \`\`\`  **Note**: Custom policies require BIND9 \`dnssec-policy\` configuration. Built-in policies: \`"default"\`, \`"none"\` |
| `nameServerIps` | object | No | (DEPRECATED in v0.4.0) Map of nameserver hostnames to IP addresses for glue records.  **Use \`nameServers\` instead.** This field will be removed in v1.0.0.  Glue records provide IP addresses for nameservers within the zone's own domain. This is necessary when delegating subdomains where the nameserver is within the delegated zone itself.  Example: When delegating \`sub.example.com\` with nameserver \`ns1.sub.example.com\`, you must provide the IP address of \`ns1.sub.example.com\` as a glue record.  Format: \`{"ns1.example.com.": "192.0.2.1", "ns2.example.com.": "192.0.2.2"}\`  Note: Nameserver hostnames should end with a dot (.) for FQDN.  **Migration to \`nameServers\`:** \`\`\`yaml # Old (deprecated): nameServerIps:   ns2.example.com.: "192.0.2.2"  # New (recommended): nameServers:   - hostname: ns2.example.com.     ipv4Address: "192.0.2.2" \`\`\` |
| `nameServers` | array | No | Authoritative nameservers for this zone (v0.4.0+).  NS records are automatically generated at the zone apex (@) for all entries. The primary nameserver from \`soaRecord.primaryNs\` is always included automatically.  Each entry can optionally include IP addresses to generate glue records (A/AAAA) for in-zone nameservers. Glue records are required when the nameserver is within the zone's own domain to avoid circular dependencies.  # Examples  \`\`\`yaml # In-zone nameservers with glue records nameServers:   - hostname: ns2.example.com.     ipv4Address: "192.0.2.2"   - hostname: ns3.example.com.     ipv4Address: "192.0.2.3"     ipv6Address: "2001:db8::3"  # Out-of-zone nameserver (no glue needed)   - hostname: ns4.external-provider.net. \`\`\`  **Generated Records:** - \`@ IN NS ns2.example.com.\` (NS record) - \`ns2.example.com. IN A 192.0.2.2\` (glue record for in-zone NS) - \`@ IN NS ns3.example.com.\` (NS record) - \`ns3.example.com. IN A 192.0.2.3\` (IPv4 glue) - \`ns3.example.com. IN AAAA 2001:db8::3\` (IPv6 glue) - \`@ IN NS ns4.external-provider.net.\` (NS record only, no glue)  **Benefits over \`nameServerIps\` (deprecated):** - Clearer purpose: authoritative nameservers, not just glue records - IPv6 support via \`ipv6Address\` field - Automatic NS record generation (no manual \`NSRecord\` CRs needed)  **Migration:** See [docs/src/operations/migration-guide.md](../operations/migration-guide.md) |
| `recordsFrom` | array | No | Sources for DNS records to include in this zone.  This field defines label selectors that automatically associate DNS records with this zone. Records with matching labels will be included in the zone's DNS configuration.  This follows the standard Kubernetes selector pattern used by Services, \`NetworkPolicies\`, and other resources for declarative resource association.  # Example: Match podinfo records in dev/staging environments  \`\`\`yaml recordsFrom:   - selector:       matchLabels:         app: podinfo       matchExpressions:         - key: environment           operator: In           values:             - dev             - staging \`\`\`  # Selector Operators  - **In**: Label value must be in the specified values list - **\`NotIn\`**: Label value must NOT be in the specified values list - **Exists**: Label key must exist (any value) - **\`DoesNotExist\`**: Label key must NOT exist  # Use Cases  - **Multi-environment zones**: Dynamically include records based on environment labels - **Application-specific zones**: Group all records for an application using \`app\` label - **Team-based zones**: Use team labels to automatically route records to team-owned zones - **Temporary records**: Use labels to include/exclude records without changing \`zoneRef\` |
| `soaRecord` | object | Yes | SOA (Start of Authority) record - defines zone authority and refresh parameters.  The SOA record is required for all authoritative zones and contains timing information for zone transfers and caching. |
| `ttl` | integer | No | Default TTL (Time To Live) for records in this zone, in seconds.  If not specified, individual records must specify their own TTL. Typical values: 300-86400 (5 minutes to 1 day). |
| `zoneName` | string | Yes | DNS zone name (e.g., "example.com").  Must be a valid DNS zone name. Can be a domain or subdomain. Examples: "example.com", "internal.example.com", "10.in-addr.arpa" |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `bind9Instances` | array | No | List of \`Bind9Instance\` resources and their status for this zone.  **Single Source of Truth for Instance-Zone Relationships:** This field tracks all \`Bind9Instances\` selected by this zone via \`bind9InstancesFrom\` selectors, along with the current status of zone configuration on each instance.  **Status Lifecycle:** - \`Claimed\`: Zone selected this instance (via \`bind9InstancesFrom\`), waiting for configuration - \`Configured\`: Zone successfully configured on instance - \`Failed\`: Zone configuration failed on instance - \`Unclaimed\`: Instance no longer selected by this zone (cleanup pending)  **Event-Driven Pattern:** - \`DNSZone\` controller evaluates \`bind9InstancesFrom\` selectors to find matching instances - \`DNSZone\` controller reads this field to track configuration status - \`DNSZone\` controller updates status after configuration attempts  **Automatic Selection:** When a \`DNSZone\` reconciles, the controller automatically: 1. Queries all \`Bind9Instances\` matching \`bind9InstancesFrom\` selectors 2. Adds them to this list with status="Claimed" 3. Configures zones on each instance  # Example  \`\`\`yaml status:   bind9Instances:     - apiVersion: bindy.firestoned.io/v1beta1       kind: Bind9Instance       name: primary-dns-0       namespace: dns-system       status: Configured       lastReconciledAt: "2026-01-03T20:00:00Z"     - apiVersion: bindy.firestoned.io/v1beta1       kind: Bind9Instance       name: secondary-dns-0       namespace: dns-system       status: Claimed       lastReconciledAt: "2026-01-03T20:01:00Z" \`\`\` |
| `bind9InstancesCount` | integer | No | Number of \`Bind9Instance\` resources in the \`bind9_instances\` list.  This field is automatically updated whenever the \`bind9_instances\` list changes. It provides a quick view of how many instances are serving this zone without requiring clients to count array elements. |
| `conditions` | array | No |  |
| `dnssec` | object | No | DNSSEC signing status for this zone  Populated when DNSSEC signing is enabled. Contains DS records, key tags, and rotation information.  **Important**: DS records must be published in the parent zone to complete the DNSSEC chain of trust.  # Example  \`\`\`yaml dnssec:   signed: true   dsRecords:     - "example.com. IN DS 12345 13 2 ABC123..."   keyTag: 12345   algorithm: "ECDSAP256SHA256"   nextKeyRollover: "2026-04-02T00:00:00Z" \`\`\` |
| `observedGeneration` | integer | No |  |
| `records` | array | No | List of DNS records selected by recordsFrom label selectors.  **Event-Driven Pattern:** - Records with \`lastReconciledAt == None\` need reconciliation - Records with \`lastReconciledAt == Some(timestamp)\` are already configured  This field is populated by the \`DNSZone\` controller when evaluating \`recordsFrom\` selectors. The timestamp is set by the record operator after successful BIND9 update.  **Single Source of Truth:** This status field is authoritative for which records belong to this zone and whether they need reconciliation, preventing redundant BIND9 API calls. |
| `recordsCount` | integer | No | Count of records selected by recordsFrom label selectors.  This field is automatically calculated from the length of \`records\`. It provides a quick view of how many records are associated with this zone.  Defaults to 0 when no records are selected. |

---

## DNS Records

### ARecord

**API Version**: `bindy.firestoned.io/v1beta1`

ARecord maps a DNS hostname to an IPv4 address. Multiple A records for the same name enable round-robin DNS load balancing.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `ipv4Address` | string | Yes | IPv4 address in dotted-decimal notation.  Must be a valid IPv4 address (e.g., "192.0.2.1"). |
| `name` | string | Yes | Record name within the zone. Use "@" for the zone apex.  Examples: "www", "mail", "ftp", "@" The full DNS name will be: {name}.{zone} |
| `ttl` | integer | No | Time To Live in seconds. Overrides zone default TTL if specified.  Typical values: 60-86400 (1 minute to 1 day). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

### AAAARecord

**API Version**: `bindy.firestoned.io/v1beta1`

AAAARecord maps a DNS hostname to an IPv6 address. This is the IPv6 equivalent of an A record.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `ipv6Address` | string | Yes | IPv6 address in standard notation.  Examples: \`2001:db8::1\`, \`fe80::1\`, \`::1\` |
| `name` | string | Yes | Record name within the zone. |
| `ttl` | integer | No | Time To Live in seconds. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

### CNAMERecord

**API Version**: `bindy.firestoned.io/v1beta1`

CNAMERecord creates a DNS alias from one hostname to another. A CNAME cannot coexist with other record types for the same name.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `name` | string | Yes | Record name within the zone.  Note: CNAME records cannot be created at the zone apex (@). |
| `target` | string | Yes | Target hostname (canonical name).  Should be a fully qualified domain name ending with a dot. Example: "example.com." or "www.example.com." |
| `ttl` | integer | No | Time To Live in seconds. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

### MXRecord

**API Version**: `bindy.firestoned.io/v1beta1`

MXRecord specifies mail exchange servers for a domain. Lower priority values indicate higher preference for mail delivery.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `mailServer` | string | Yes | Fully qualified domain name of the mail server.  Must end with a dot. Example: "mail.example.com." |
| `name` | string | Yes | Record name within the zone. Use "@" for the zone apex. |
| `priority` | integer | Yes | Priority (preference) of this mail server. Lower values = higher priority.  Common values: 0-100. Multiple MX records can exist with different priorities. |
| `ttl` | integer | No | Time To Live in seconds. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

### NSRecord

**API Version**: `bindy.firestoned.io/v1beta1`

NSRecord delegates a subdomain to authoritative nameservers. Used for subdomain delegation to different DNS providers or servers.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `name` | string | Yes | Subdomain to delegate. For zone apex, use "@". |
| `nameserver` | string | Yes | Fully qualified domain name of the nameserver.  Must end with a dot. Example: "ns1.example.com." |
| `ttl` | integer | No | Time To Live in seconds. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

### TXTRecord

**API Version**: `bindy.firestoned.io/v1beta1`

TXTRecord stores arbitrary text data in DNS. Commonly used for SPF, DKIM, DMARC policies, and domain verification.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `name` | string | Yes | Record name within the zone. |
| `text` | array | Yes | Array of text strings. Each string can be up to 255 characters.  Multiple strings are concatenated by DNS resolvers. For long text, split into multiple strings. |
| `ttl` | integer | No | Time To Live in seconds. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

### SRVRecord

**API Version**: `bindy.firestoned.io/v1beta1`

SRVRecord specifies the hostname and port of servers for specific services. The record name follows the format _service._proto (e.g., _ldap._tcp).

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `name` | string | Yes | Service and protocol in the format: _service._proto  Example: "_ldap._tcp", "_sip._udp", "_http._tcp" |
| `port` | integer | Yes | TCP or UDP port where the service is available. |
| `priority` | integer | Yes | Priority of the target host. Lower values = higher priority. |
| `target` | string | Yes | Fully qualified domain name of the target host.  Must end with a dot. Use "." for "service not available". |
| `ttl` | integer | No | Time To Live in seconds. |
| `weight` | integer | Yes | Relative weight for records with the same priority.  Higher values = higher probability of selection. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

### CAARecord

**API Version**: `bindy.firestoned.io/v1beta1`

CAARecord specifies which certificate authorities are authorized to issue certificates for a domain. Enhances domain security and certificate issuance control.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `flags` | integer | Yes | Flags byte. Use 0 for non-critical, 128 for critical.  Critical flag (128) means CAs must understand the tag. |
| `name` | string | Yes | Record name within the zone. Use "@" for the zone apex. |
| `tag` | string | Yes | Property tag. Common values: "issue", "issuewild", "iodef".  - "issue": Authorize CA to issue certificates - "issuewild": Authorize CA to issue wildcard certificates - "iodef": URL/email for violation reports |
| `ttl` | integer | No | Time To Live in seconds. |
| `value` | string | Yes | Property value. Format depends on the tag.  For "issue"/"issuewild": CA domain (e.g., "letsencrypt.org") For "iodef": mailto: or https: URL |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `lastUpdated` | string | No | Timestamp of the last successful update to BIND9.  This is updated after a successful nsupdate operation. Uses RFC 3339 format (e.g., "2025-12-26T10:30:00Z"). |
| `observedGeneration` | integer | No |  |
| `recordHash` | string | No | SHA-256 hash of the record's spec data.  Used to detect when a record's data has actually changed, avoiding unnecessary BIND9 updates and zone transfers.  The hash is calculated from all fields in the record's spec that affect the DNS record data (name, addresses, TTL, etc.). |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9.  **DEPRECATED**: Use \`zone_ref\` instead for structured zone reference. |
| `zoneRef` | object | No | Structured reference to the \`DNSZone\` that owns this record.  Set by the \`DNSZone\` controller when the zone's \`recordsFrom\` selector matches this record's labels. Contains the complete Kubernetes object reference including apiVersion, kind, name, namespace, and zoneName.  The record reconciler uses this to: 1. Look up the parent \`DNSZone\` resource 2. Find the zone's primary \`Bind9Instance\` servers 3. Add this record to BIND9 on primaries 4. Trigger zone transfer (retransfer) on secondaries  If this field is None, the record is not selected by any zone and will not be added to BIND9. |

---

## Infrastructure

### Bind9Cluster

**API Version**: `bindy.firestoned.io/v1beta1`

Bind9Cluster defines a namespace-scoped logical grouping of BIND9 DNS server instances. Use this for tenant-managed DNS infrastructure isolated to a specific namespace. For platform-managed cluster-wide DNS, use ClusterBind9Provider instead.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `acls` | object | No | ACLs that can be referenced by instances |
| `configMapRefs` | object | No | \`ConfigMap\` references for BIND9 configuration files |
| `global` | object | No | Global configuration shared by all instances in the cluster  This configuration applies to all instances (both primary and secondary) unless overridden at the instance level or by role-specific configuration. |
| `image` | object | No | Container image configuration |
| `primary` | object | No | Primary instance configuration  Configuration specific to primary (authoritative) DNS instances, including replica count and service specifications. |
| `rndcSecretRefs` | array | No | References to Kubernetes Secrets containing RNDC/TSIG keys for authenticated zone transfers.  Each secret should contain the key name, algorithm, and base64-encoded secret value. These secrets are used for secure communication with BIND9 instances via RNDC and for authenticated zone transfers (AXFR/IXFR) between primary and secondary servers. |
| `secondary` | object | No | Secondary instance configuration  Configuration specific to secondary (replica) DNS instances, including replica count and service specifications. |
| `version` | string | No | Shared BIND9 version for the cluster  If not specified, defaults to "9.18". |
| `volumeMounts` | array | No | Volume mounts that specify where volumes should be mounted in containers  These mounts are inherited by all instances unless overridden. |
| `volumes` | array | No | Volumes that can be mounted by instances in this cluster  These volumes are inherited by all instances unless overridden. Common use cases include \`PersistentVolumeClaims\` for zone data storage. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No | Status conditions for this cluster |
| `instanceCount` | integer | No | Number of instances in this cluster |
| `instances` | array | No | Names of \`Bind9Instance\` resources created for this cluster |
| `observedGeneration` | integer | No | Observed generation for optimistic concurrency |
| `readyInstances` | integer | No | Number of ready instances |

---

### Bind9Instance

**API Version**: `bindy.firestoned.io/v1beta1`

Bind9Instance represents a BIND9 DNS server deployment in Kubernetes. Each instance creates a Deployment, Service, ConfigMap, and Secret for managing a BIND9 server with RNDC protocol communication.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `bindcarConfig` | object | No | Bindcar RNDC API sidecar container configuration.  The API container provides an HTTP interface for managing zones via rndc. If not specified, uses default configuration. |
| `clusterRef` | string | Yes | Reference to the cluster this instance belongs to.  Can reference either: - A namespace-scoped \`Bind9Cluster\` (must be in the same namespace as this instance) - A cluster-scoped \`ClusterBind9Provider\` (cluster-wide, accessible from any namespace)  The cluster provides shared configuration and defines the logical grouping. The controller will automatically detect whether this references a namespace-scoped or cluster-scoped cluster resource. |
| `config` | object | No | Instance-specific BIND9 configuration overrides.  Overrides cluster-level configuration for this instance only. |
| `configMapRefs` | object | No | \`ConfigMap\` references override. Inherits from cluster if not specified. |
| `image` | object | No | Container image configuration override. Inherits from cluster if not specified. |
| `primaryServers` | array | No | Primary server addresses for zone transfers (required for secondary instances).  List of IP addresses or hostnames of primary servers to transfer zones from. Example: \`["10.0.1.10", "primary.example.com"]\` |
| `replicas` | integer | No | Number of pod replicas for high availability.  Defaults to 1 if not specified. For production, use 2+ replicas. |
| `rndcKey` | object | No | Instance-level RNDC key configuration with lifecycle management.  Supports automatic key rotation, Secret references, and inline Secret specifications. Overrides role-level and global RNDC configuration for this specific instance.  **Precedence order**: 1. **Instance level** (\`spec.rndcKey\`) - Highest priority 2. Role level (\`spec.primary.rndcKey\` or \`spec.secondary.rndcKey\`) 3. Global level (cluster-wide RNDC configuration) 4. Auto-generated (default)  **Backward compatibility**: If both \`rndc_key\` and \`rndc_secret_ref\` are specified, \`rndc_key\` takes precedence. For smooth migration, \`rndc_secret_ref\` will continue to work but is deprecated.  # Example  \`\`\`yaml apiVersion: bindy.firestoned.io/v1beta1 kind: Bind9Instance spec:   rndcKey:     autoRotate: true     rotateAfter: 2160h  # 90 days     algorithm: hmac-sha512 \`\`\` |
| `rndcSecretRef` | object | No | Reference to an existing Kubernetes Secret containing RNDC key.  If specified, uses this existing Secret instead of auto-generating one. The Secret must contain the keys specified in the reference (defaults: "key-name", "algorithm", "secret", "rndc.key"). This allows sharing RNDC keys across instances or using externally managed secrets.  If not specified, a Secret will be auto-generated for this instance. |
| `role` | string | Yes | Role of this instance (primary or secondary).  Primary instances are authoritative for zones. Secondary instances replicate zones from primaries via AXFR/IXFR. |
| `storage` | object | No | Storage configuration for zone files.  Specifies how zone files should be stored. Defaults to emptyDir (ephemeral storage). For persistent storage, use persistentVolumeClaim. |
| `version` | string | No | BIND9 version override. Inherits from cluster if not specified.  Example: "9.18", "9.16" |
| `volumeMounts` | array | No | Volume mounts override for this instance. Inherits from cluster if not specified.  These mounts override cluster-level volume mounts. |
| `volumes` | array | No | Volumes override for this instance. Inherits from cluster if not specified.  These volumes override cluster-level volumes. Common use cases include instance-specific \`PersistentVolumeClaims\` for zone data storage. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `clusterRef` | object | No | Resolved cluster reference with full object details.  This field is populated by the instance reconciler and contains the full Kubernetes object reference (kind, apiVersion, namespace, name) of the cluster this instance belongs to. This provides backward compatibility with \`spec.clusterRef\` (which is just a string name) and enables proper Kubernetes object references.  For namespace-scoped \`Bind9Cluster\`, includes namespace. For cluster-scoped \`ClusterBind9Provider\`, namespace will be empty. |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |
| `rndcKeyRotation` | object | No | RNDC key rotation status and tracking information.  Populated when \`auto_rotate\` is enabled in the RNDC configuration. Provides visibility into key lifecycle: creation time, next rotation time, and rotation count.  This field is automatically updated by the instance reconciler whenever: - A new RNDC key is generated - An RNDC key is rotated - The rotation configuration changes  **Note**: Only present when using operator-managed RNDC keys. If you specify \`secret_ref\` to use an external Secret, this field will be empty. |
| `serviceAddress` | string | No | IP or hostname of this instance's service |
| `zones` | array | No | List of DNS zones that have selected this instance.  This field is automatically populated by a status-only watcher on \`DNSZones\`. When a \`DNSZone\`'s \`status.bind9Instances\` includes this instance, the zone is added to this list. This provides a reverse lookup: instance → zones.  Updated by: \`DNSZone\` status watcher (not by instance reconciler) Used for: Observability, debugging zone assignments |
| `zonesCount` | integer | No | Number of zones in the \`zones\` list.  This field is automatically updated whenever the \`zones\` list changes. It provides a quick way to see how many zones are selecting this instance without having to count the array elements. |

---

