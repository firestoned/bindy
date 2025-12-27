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
| `clusterProviderRef` | string | No | Reference to a cluster-scoped \`ClusterBind9Provider\`.  Must match the name of a \`ClusterBind9Provider\` resource (cluster-scoped). The zone will be added to all instances in this provider.  Either \`clusterRef\` or \`clusterProviderRef\` must be specified (not both). |
| `clusterRef` | string | No | Reference to a namespace-scoped \`Bind9Cluster\` in the same namespace.  Must match the name of a \`Bind9Cluster\` resource in the same namespace. The zone will be added to all instances in this cluster.  Either \`clusterRef\` or \`clusterProviderRef\` must be specified (not both). |
| `nameServerIps` | object | No | Map of nameserver hostnames to IP addresses for glue records.  Glue records provide IP addresses for nameservers within the zone's own domain. This is necessary when delegating subdomains where the nameserver is within the delegated zone itself.  Example: When delegating \`sub.example.com\` with nameserver \`ns1.sub.example.com\`, you must provide the IP address of \`ns1.sub.example.com\` as a glue record.  Format: \`{"ns1.example.com.": "192.0.2.1", "ns2.example.com.": "192.0.2.2"}\`  Note: Nameserver hostnames should end with a dot (.) for FQDN. |
| `recordsFrom` | array | No | Sources for DNS records to include in this zone.  This field defines label selectors that automatically associate DNS records with this zone. Records with matching labels will be included in the zone's DNS configuration.  This follows the standard Kubernetes selector pattern used by Services, \`NetworkPolicies\`, and other resources for declarative resource association.  # Example: Match podinfo records in dev/staging environments  \`\`\`yaml recordsFrom:   - selector:       matchLabels:         app: podinfo       matchExpressions:         - key: environment           operator: In           values:             - dev             - staging \`\`\`  # Selector Operators  - **In**: Label value must be in the specified values list - **\`NotIn\`**: Label value must NOT be in the specified values list - **Exists**: Label key must exist (any value) - **\`DoesNotExist\`**: Label key must NOT exist  # Use Cases  - **Multi-environment zones**: Dynamically include records based on environment labels - **Application-specific zones**: Group all records for an application using \`app\` label - **Team-based zones**: Use team labels to automatically route records to team-owned zones - **Temporary records**: Use labels to include/exclude records without changing \`zoneRef\` |
| `soaRecord` | object | Yes | SOA (Start of Authority) record - defines zone authority and refresh parameters.  The SOA record is required for all authoritative zones and contains timing information for zone transfers and caching. |
| `ttl` | integer | No | Default TTL (Time To Live) for records in this zone, in seconds.  If not specified, individual records must specify their own TTL. Typical values: 300-86400 (5 minutes to 1 day). |
| `zoneName` | string | Yes | DNS zone name (e.g., "example.com").  Must be a valid DNS zone name. Can be a domain or subdomain. Examples: "example.com", "internal.example.com", "10.in-addr.arpa" |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |
| `recordCount` | integer | No |  |
| `records` | array | No | List of DNS records successfully associated with this zone. Updated by the zone reconciler when records are added/removed. |
| `secondaryIps` | array | No | IP addresses of secondary servers configured for zone transfers. Used to detect when secondary IPs change and zones need updating. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `observedGeneration` | integer | No |  |
| `zone` | string | No | The FQDN of the zone that owns this record (set by \`DNSZone\` controller).  When a \`DNSZone\`'s label selector matches this record, the \`DNSZone\` controller sets this field to the zone's FQDN (e.g., \`"example.com"\`). The record reconciler uses this to determine which zone to update in BIND9.  If this field is empty, the record is not matched by any zone and should not be reconciled into BIND9. |

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
| `rndcSecretRef` | object | No | Reference to an existing Kubernetes Secret containing RNDC key.  If specified, uses this existing Secret instead of auto-generating one. The Secret must contain the keys specified in the reference (defaults: "key-name", "algorithm", "secret", "rndc.key"). This allows sharing RNDC keys across instances or using externally managed secrets.  If not specified, a Secret will be auto-generated for this instance. |
| `role` | string | Yes | Role of this instance (primary or secondary).  Primary instances are authoritative for zones. Secondary instances replicate zones from primaries via AXFR/IXFR. |
| `storage` | object | No | Storage configuration for zone files.  Specifies how zone files should be stored. Defaults to emptyDir (ephemeral storage). For persistent storage, use persistentVolumeClaim. |
| `version` | string | No | BIND9 version override. Inherits from cluster if not specified.  Example: "9.18", "9.16" |
| `volumeMounts` | array | No | Volume mounts override for this instance. Inherits from cluster if not specified.  These mounts override cluster-level volume mounts. |
| `volumes` | array | No | Volumes override for this instance. Inherits from cluster if not specified.  These volumes override cluster-level volumes. Common use cases include instance-specific \`PersistentVolumeClaims\` for zone data storage. |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |
| `readyReplicas` | integer | No |  |
| `replicas` | integer | No |  |
| `serviceAddress` | string | No | IP or hostname of this instance's service |

---

