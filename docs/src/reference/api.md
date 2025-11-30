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

**API Version**: `bindy.firestoned.io/v1alpha1`

DNSZone represents an authoritative DNS zone managed by BIND9. Each DNSZone defines a zone (e.g., example.com) with SOA record parameters and is served by a specified Bind9Instance.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `clusterRef` | string | Yes | Reference to the \`Bind9Instance\` that serves this zone.  Must match the name of a \`Bind9Instance\` in the same namespace. The zone will be added to this instance via rndc addzone. |
| `soaRecord` | object | Yes | SOA (Start of Authority) record - defines zone authority and refresh parameters.  The SOA record is required for all authoritative zones and contains timing information for zone transfers and caching. |
| `ttl` | integer | No | Default TTL (Time To Live) for records in this zone, in seconds.  If not specified, individual records must specify their own TTL. Typical values: 300-86400 (5 minutes to 1 day). |
| `zoneName` | string | Yes | DNS zone name (e.g., "example.com").  Must be a valid DNS zone name. Can be a domain or subdomain. Examples: "example.com", "internal.example.com", "10.in-addr.arpa" |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |
| `recordCount` | integer | No |  |

---

## DNS Records

### ARecord

**API Version**: `bindy.firestoned.io/v1alpha1`

ARecord maps a DNS hostname to an IPv4 address. Multiple A records for the same name enable round-robin DNS load balancing.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `ipv4Address` | string | Yes | IPv4 address in dotted-decimal notation.  Must be a valid IPv4 address (e.g., "192.0.2.1"). |
| `name` | string | Yes | Record name within the zone. Use "@" for the zone apex.  Examples: "www", "mail", "ftp", "@" The full DNS name will be: {name}.{zone} |
| `ttl` | integer | No | Time To Live in seconds. Overrides zone default TTL if specified.  Typical values: 60-86400 (1 minute to 1 day). |
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. The controller will search for a \`DNSZone\` with matching \`spec.zoneName\`. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Directly references a \`DNSZone\` resource by its Kubernetes name for more efficient lookup. Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

### AAAARecord

**API Version**: `bindy.firestoned.io/v1alpha1`

AAAARecord maps a DNS hostname to an IPv6 address. This is the IPv6 equivalent of an A record.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `ipv6Address` | string | Yes | IPv6 address in standard notation.  Examples: \`2001:db8::1\`, \`fe80::1\`, \`::1\` |
| `name` | string | Yes | Record name within the zone. |
| `ttl` | integer | No | Time To Live in seconds. |
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

### CNAMERecord

**API Version**: `bindy.firestoned.io/v1alpha1`

CNAMERecord creates a DNS alias from one hostname to another. A CNAME cannot coexist with other record types for the same name.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `name` | string | Yes | Record name within the zone.  Note: CNAME records cannot be created at the zone apex (@). |
| `target` | string | Yes | Target hostname (canonical name).  Should be a fully qualified domain name ending with a dot. Example: "example.com." or "www.example.com." |
| `ttl` | integer | No | Time To Live in seconds. |
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

### MXRecord

**API Version**: `bindy.firestoned.io/v1alpha1`

MXRecord specifies mail exchange servers for a domain. Lower priority values indicate higher preference for mail delivery.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `mailServer` | string | Yes | Fully qualified domain name of the mail server.  Must end with a dot. Example: "mail.example.com." |
| `name` | string | Yes | Record name within the zone. Use "@" for the zone apex. |
| `priority` | integer | Yes | Priority (preference) of this mail server. Lower values = higher priority.  Common values: 0-100. Multiple MX records can exist with different priorities. |
| `ttl` | integer | No | Time To Live in seconds. |
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

### NSRecord

**API Version**: `bindy.firestoned.io/v1alpha1`

NSRecord delegates a subdomain to authoritative nameservers. Used for subdomain delegation to different DNS providers or servers.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `name` | string | Yes | Subdomain to delegate. For zone apex, use "@". |
| `nameserver` | string | Yes | Fully qualified domain name of the nameserver.  Must end with a dot. Example: "ns1.example.com." |
| `ttl` | integer | No | Time To Live in seconds. |
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

### TXTRecord

**API Version**: `bindy.firestoned.io/v1alpha1`

TXTRecord stores arbitrary text data in DNS. Commonly used for SPF, DKIM, DMARC policies, and domain verification.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `name` | string | Yes | Record name within the zone. |
| `text` | array | Yes | Array of text strings. Each string can be up to 255 characters.  Multiple strings are concatenated by DNS resolvers. For long text, split into multiple strings. |
| `ttl` | integer | No | Time To Live in seconds. |
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

### SRVRecord

**API Version**: `bindy.firestoned.io/v1alpha1`

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
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

### CAARecord

**API Version**: `bindy.firestoned.io/v1alpha1`

CAARecord specifies which certificate authorities are authorized to issue certificates for a domain. Enhances domain security and certificate issuance control.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `flags` | integer | Yes | Flags byte. Use 0 for non-critical, 128 for critical.  Critical flag (128) means CAs must understand the tag. |
| `name` | string | Yes | Record name within the zone. Use "@" for the zone apex. |
| `tag` | string | Yes | Property tag. Common values: "issue", "issuewild", "iodef".  - "issue": Authorize CA to issue certificates - "issuewild": Authorize CA to issue wildcard certificates - "iodef": URL/email for violation reports |
| `ttl` | integer | No | Time To Live in seconds. |
| `value` | string | Yes | Property value. Format depends on the tag.  For "issue"/"issuewild": CA domain (e.g., "letsencrypt.org") For "iodef": mailto: or https: URL |
| `zone` | string | No | DNS zone this record belongs to (e.g., "example.com").  Must match the zoneName of an existing \`DNSZone\` resource in the same namespace. Either \`zone\` or \`zoneRef\` must be specified (not both). |
| `zoneRef` | string | No | Reference to a \`DNSZone\` resource by metadata.name.  Either \`zone\` or \`zoneRef\` must be specified (not both). |

#### Status Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `conditions` | array | No |  |
| `observedGeneration` | integer | No |  |

---

## Infrastructure

### Bind9Cluster

**API Version**: `bindy.firestoned.io/v1alpha1`

Bind9Cluster defines a logical grouping of BIND9 DNS server instances with shared configuration. Provides centralized management of BIND9 version, container images, and common settings across multiple instances.

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
| `version` | string | No | Shared BIND9 version for the cluster |
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

**API Version**: `bindy.firestoned.io/v1alpha1`

Bind9Instance represents a BIND9 DNS server deployment in Kubernetes. Each instance creates a Deployment, Service, ConfigMap, and Secret for managing a BIND9 server with RNDC protocol communication.

#### Spec Fields

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `clusterRef` | string | Yes | Reference to the \`Bind9Cluster\` this instance belongs to.  The cluster provides shared configuration and defines the logical grouping. |
| `config` | object | No | Instance-specific BIND9 configuration overrides.  Overrides cluster-level configuration for this instance only. |
| `configMapRefs` | object | No | \`ConfigMap\` references override. Inherits from cluster if not specified. |
| `image` | object | No | Container image configuration override. Inherits from cluster if not specified. |
| `primaryServers` | array | No | Primary server addresses for zone transfers (required for secondary instances).  List of IP addresses or hostnames of primary servers to transfer zones from. Example: \`["10.0.1.10", "primary.example.com"]\` |
| `replicas` | integer | No | Number of pod replicas for high availability.  Defaults to 1 if not specified. For production, use 2+ replicas. |
| `rndcSecretRef` | object | No | Reference to an existing Kubernetes Secret containing RNDC key.  If specified, uses this existing Secret instead of auto-generating one. The Secret must contain the keys specified in the reference (defaults: "key-name", "algorithm", "secret", "rndc.key"). This allows sharing RNDC keys across instances or using externally managed secrets.  If not specified, a Secret will be auto-generated for this instance. |
| `role` | string | Yes | Role of this instance (primary or secondary).  Primary instances are authoritative for zones. Secondary instances replicate zones from primaries via AXFR/IXFR. |
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

