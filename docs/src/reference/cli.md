# CLI Reference

`bindy` is a single binary with multiple subcommands, each running as an independent controller loop.

```text
bindy <SUBCOMMAND> [OPTIONS]

Subcommands:
  run         Run the main BIND9 DNS operator (all controllers)
  scout       Run the Ingress-to-ARecord scout controller
  completion  Output shell completion code for the specified shell
```

---

## `bindy run`

Starts the main operator. Manages the full lifecycle of all BIND9 DNS custom resources:
`ClusterBind9Provider`, `Bind9Cluster`, `Bind9Instance`, `DNSZone`, and all DNS record types.

All configuration is via environment variables. See [Environment Variables](../operations/env-vars.md) for the full reference.

```bash
bindy run
```

### What it starts

| Controller | Watches | Manages |
|---|---|---|
| `ClusterBind9Provider` | ClusterBind9Provider | Bind9Cluster |
| `Bind9Cluster` | Bind9Cluster | Bind9Instance |
| `Bind9Instance` | Bind9Instance, Deployment, Service, … | DNS server pods |
| `DNSZone` | DNSZone, all record types, Bind9Instance | Zone files |
| `ARecord` | ARecord | A record in BIND9 |
| `AAAARecord` | AAAARecord | AAAA record in BIND9 |
| `CNAMERecord` | CNAMERecord | CNAME record in BIND9 |
| `MXRecord` | MXRecord | MX record in BIND9 |
| `NSRecord` | NSRecord | NS record in BIND9 |
| `TXTRecord` | TXTRecord | TXT record in BIND9 |
| `SRVRecord` | SRVRecord | SRV record in BIND9 |
| `CAARecord` | CAARecord | CAA record in BIND9 |

### Environment variables

#### Kubernetes client

| Variable | Default | Description |
|---|---|---|
| `BINDY_KUBE_QPS` | `50.0` | API server request rate (queries per second) |
| `BINDY_KUBE_BURST` | `100` | API server burst cap above QPS |

#### Leader election

| Variable | Default | Description |
|---|---|---|
| `BINDY_ENABLE_LEADER_ELECTION` | `true` | Set to `false` to disable. Only disable for local development. |
| `BINDY_LEASE_NAME` | `bindy-leader` | Name of the Kubernetes `Lease` object used for leader election. |
| `BINDY_LEASE_NAMESPACE` | `$POD_NAMESPACE` or `bindy-system` | Namespace where the `Lease` object lives. |
| `BINDY_LEASE_DURATION_SECONDS` | `15` | Lease duration. A new leader cannot be elected until this expires. |
| `BINDY_LEASE_RENEW_DEADLINE_SECONDS` | `10` | Time within which the leader must renew its lease. |
| `BINDY_LEASE_RETRY_PERIOD_SECONDS` | `2` | How often non-leaders attempt to acquire the lease. |
| `POD_NAME` | — | Leader election identity. Falls back to `$HOSTNAME`, then a random string. Set via Kubernetes downward API. |
| `POD_NAMESPACE` | — | Pod namespace. Injected by Kubernetes downward API. Used for lease namespace fallback. |

#### Metrics

| Variable | Default | Description |
|---|---|---|
| `BINDY_METRICS_BIND_ADDRESS` | `0.0.0.0` | Prometheus scrape endpoint bind address. |
| `BINDY_METRICS_PORT` | `8080` | Prometheus scrape endpoint port. |
| `BINDY_METRICS_PATH` | `/metrics` | HTTP path for Prometheus scraping. |

#### Logging

| Variable | Default | Description |
|---|---|---|
| `RUST_LOG` | `info` | Log level filter. Values: `trace`, `debug`, `info`, `warn`, `error`. Supports module-level filters, e.g. `bindy=debug,kube=warn`. |
| `RUST_LOG_FORMAT` | `text` | Log output format. `text` = compact human-readable; `json` = structured for log aggregators (Loki, ELK, Splunk). |

---

## `bindy scout`

Starts the Scout controller. Watches `Ingress` resources cluster-wide and creates `ARecord` CRs for annotated hosts. See [Bindy Scout](../guide/scout.md) for the full conceptual guide.

```bash
bindy scout [OPTIONS]
```

### Options

| Flag | Env var | Default | Description |
|---|---|---|---|
| `--bind9-cluster-name <NAME>` | `BINDY_SCOUT_CLUSTER_NAME` | — | **Required.** Logical name of this cluster, stamped on all created `ARecord` labels as `bindy.firestoned.io/source-cluster`. Used to distinguish records from multiple workload clusters writing to the same bindy namespace. |
| `--namespace <NS>` | `BINDY_SCOUT_NAMESPACE` | `bindy-system` | Namespace where `ARecord` CRs are created. |

!!! note "CLI takes precedence"
    When both a CLI flag and the corresponding environment variable are set, the **CLI flag wins**.

### Additional environment variables

| Variable | Default | Description |
|---|---|---|
| `POD_NAMESPACE` | `default` | Scout's own namespace. Always excluded from Ingress watching. Set via Kubernetes downward API. |
| `BINDY_SCOUT_EXCLUDE_NAMESPACES` | — | Comma-separated list of namespaces to skip in addition to `POD_NAMESPACE`. |
| `RUST_LOG` | `info` | Log level filter. |
| `RUST_LOG_FORMAT` | `text` | Log format (`text` or `json`). |

### Examples

```bash
# Minimal — cluster name required, everything else defaults
bindy scout --bind9-cluster-name prod

# Explicit namespace
bindy scout --bind9-cluster-name prod --namespace bindy-system

# Using environment variables only (typical Kubernetes deployment)
BINDY_SCOUT_CLUSTER_NAME=prod BINDY_SCOUT_NAMESPACE=bindy-system bindy scout

# Mix: cluster name from CLI flag, namespace from environment
BINDY_SCOUT_NAMESPACE=bindy-system bindy scout --bind9-cluster-name staging

# Debug logging
RUST_LOG=bindy=debug bindy scout --bind9-cluster-name dev
```

### Ingress annotations

Scout reads the following annotations from `Ingress` resources:

| Annotation | Required | Description |
|---|---|---|
| `bindy.firestoned.io/recordKind` | **Yes** (`"ARecord"`) | Specifies the DNS record kind to create. Currently only `"ARecord"` is supported. |
| `bindy.firestoned.io/zone` | **Yes** | DNS zone that owns the Ingress hosts (e.g. `example.com`). |
| `bindy.firestoned.io/ip` | No | Explicit IP override. Defaults to LoadBalancer status IP. |
| `bindy.firestoned.io/ttl` | No | TTL override in seconds. |

---

## `bindy completion`

Outputs shell completion code for the specified shell. No environment variables or Kubernetes access required.

```bash
bindy completion <SHELL>
```

### Supported shells

| Shell | Value |
|---|---|
| Bash | `bash` |
| Zsh | `zsh` |
| Fish | `fish` |
| PowerShell | `powershell` |

### Installation

=== "Bash"

    Add to `~/.bashrc`:

    ```bash
    source <(bindy completion bash)
    ```

    Or for a system-wide install:

    ```bash
    bindy completion bash > /etc/bash_completion.d/bindy
    ```

=== "Zsh"

    Add to `~/.zshrc`:

    ```bash
    source <(bindy completion zsh)
    ```

    Or with `oh-my-zsh`:

    ```bash
    bindy completion zsh > "${fpath[1]}/_bindy"
    ```

=== "Fish"

    ```bash
    bindy completion fish | source
    ```

    Or to persist across sessions:

    ```bash
    bindy completion fish > ~/.config/fish/completions/bindy.fish
    ```

=== "PowerShell"

    Add to `$PROFILE`:

    ```powershell
    bindy completion powershell | Out-String | Invoke-Expression
    ```
