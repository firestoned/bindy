# Changelog

All notable changes to this project will be documented in this file.

## [2025-12-05] - Document bindcar API Compatibility

**Author:** Erick Bourgeois

### Changed
- `src/bind9.rs`: Enhanced documentation for local `CreateZoneRequest` and `ZoneResponse` structs
  - Clarified that these structs match the bindcar **HTTP API** JSON format (not the Rust library)
  - bindcar HTTP API accepts `zoneContent` as raw BIND9 zone file string
  - bindcar Rust library v0.2+ uses structured `ZoneConfig` type with separate fields (ttl, soa, records)
  - Local structs will remain until bindcar provides HTTP API client library or raw content support

### Why
- **API mismatch**: bindcar v0.2.0 Rust library uses structured configuration, incompatible with our raw zone file workflow
- **HTTP API compatibility**: The bindcar HTTP server still accepts raw zone content via JSON (for flexibility)
- **Clear documentation**: Developers understand why we use local types instead of importing from bindcar crate
- **Future migration path**: When bindcar adds HTTP client support or raw content conversion, we can migrate

### Technical Details

**bindcar HTTP API (what we use)**:
```json
{
  "zoneName": "example.com",
  "zoneType": "master",
  "zoneContent": "$TTL 3600\n@ IN SOA ...",  // Raw BIND9 zone file
  "updateKeyName": "bind9-key"
}
```

**bindcar Rust library v0.2.0 (incompatible)**:
```rust
pub struct ZoneConfig {
    pub ttl: u32,
    pub soa: SoaRecord,
    pub name_servers: Vec<String>,
    pub records: Vec<DnsRecord>,
}
```

**Decision**: Continue using local struct definitions that match the HTTP API JSON contract.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Documentation improvement - clarifies architecture
- [ ] Config change only
- [ ] Documentation only

---

## [2025-12-05] - Fix Docker Build Version Injection

**Author:** Erick Bourgeois

### Fixed
- `Dockerfile`: Moved version update to occur AFTER copying actual source code
  - **Before**: Version was updated in the cached dependency layer, then overwritten by COPY
  - **After**: Version is updated immediately before building the final binary
  - Ensures `cargo build` uses the correct version from the GitHub release tag
  - Binary and package metadata now correctly reflect the release version

### Why
- **Correct version metadata**: The built binary must report the actual release version, not the dev version
- **Docker layer caching bug**: The previous sed command ran too early and was overwritten
- **Release integrity**: Users can verify the binary version matches the release tag

### Technical Details

**Build Flow**:
1. GitHub release created with tag (e.g., `v1.2.3`)
2. Workflow extracts version: `1.2.3` from `github.event.release.tag_name`
3. Docker build receives: `--build-arg VERSION=1.2.3`
4. Dockerfile updates `Cargo.toml`: `version = "1.2.3"` (line 44)
5. Cargo builds binary with correct version metadata
6. Binary reports: `bindy 1.2.3` (matches release tag)

**Verification**:
```bash
# In the container
/usr/local/bin/bindy --version
# Should output: bindy 1.2.3 (not bindy 0.1.0)
```

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Build fix - ensures version metadata is correct
- [ ] Config change only
- [ ] Documentation only

---

## [2025-12-03 23:15] - Add Automatic NOTIFY to Secondaries for Zone Updates

**Author:** Erick Bourgeois

### Added
- `src/reconcilers/records.rs`: Automatic NOTIFY after every DNS record operation
  - Modified `handle_record_operation!` macro to call `notify_zone()` after successful record additions
  - All record types (A, AAAA, TXT, CNAME, MX, NS, SRV, CAA) now trigger NOTIFY automatically
  - NOTIFY failures are logged as warnings and don't fail the record operation
- `src/reconcilers/dnszone.rs`: Automatic NOTIFY after zone creation
  - Added `notify_zone()` call after `create_zone_http()` completes successfully
  - Ensures secondaries receive immediate AXFR after zone is created on primary
  - Added `warn` to tracing imports for notification failure logging

### Changed
- `src/reconcilers/records.rs`: Updated `handle_record_operation!` macro signature
  - Added parameters: `$zone_name`, `$key_data`, `$zone_manager`
  - All 7 record reconcilers updated to pass new parameters
  - Macro now handles both record status updates AND secondary notifications

### Technical Details

**Why This Was Needed:**
- BIND9's dynamic updates (nsupdate protocol) don't trigger NOTIFY by default
- Without explicit NOTIFY, secondaries only sync via SOA refresh timer (can be hours)
- This caused stale data on secondary servers in multi-primary or primary/secondary setups

**How It Works:**
1. Record is successfully added to primary via nsupdate
2. `notify_zone()` sends RFC 1996 DNS NOTIFY packets to secondaries
3. Secondaries respond by initiating IXFR (incremental zone transfer) from primary
4. Updates propagate to secondaries within seconds instead of hours

**NOTIFY Behavior:**
- NOTIFY is sent via HTTP API: `POST /api/v1/zones/{name}/notify`
- Bindcar API sidecar executes `rndc notify {zone}` locally on primary
- BIND9 reads zone configuration for `also-notify` and `allow-transfer` ACLs
- BIND9 sends NOTIFY packets to all configured secondaries
- If NOTIFY fails (network issue, API timeout), operation still succeeds
  - Warning logged: "Failed to notify secondaries for zone X. Secondaries will sync via SOA refresh timer."
  - Ensures record operations are atomic and don't fail due to transient notification issues

**Affected Operations:**
- `reconcile_a_record()` - A record additions
- `reconcile_aaaa_record()` - AAAA record additions
- `reconcile_txt_record()` - TXT record additions
- `reconcile_cname_record()` - CNAME record additions
- `reconcile_mx_record()` - MX record additions
- `reconcile_ns_record()` - NS record additions
- `reconcile_srv_record()` - SRV record additions
- `reconcile_caa_record()` - CAA record additions
- `create_zone()` in DNSZone reconciler - New zone creation

### Why
- **Real-time replication**: Secondaries receive updates immediately instead of waiting for SOA refresh
- **Consistency**: Eliminates stale data windows between primary and secondary servers
- **RFC compliance**: Proper implementation of DNS NOTIFY (RFC 1996) for zone change notifications
- **Production readiness**: Essential for any primary/secondary DNS architecture

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Behavioral change - secondaries now notified automatically
- [ ] Config change only
- [ ] Documentation only

---

## [2025-12-03 22:40] - Standardize on Linkerd Service Mesh References

**Author:** Erick Bourgeois

### Changed
- `CLAUDE.md`: Added service mesh standard - always use Linkerd as the example
- `docs/src/operations/faq.md`: Updated "service meshes" question to specifically reference Linkerd
  - Added details about Linkerd injection being disabled for DNS services
- `docs/src/advanced/integration.md`: Changed "Service Mesh" section to "Linkerd Service Mesh"
  - Removed generic Istio reference, kept Linkerd as the standard
  - Added Linkerd-specific integration details (mTLS, service discovery)
- `core-bind9/service-dns.yaml`: Updated comment from "service mesh sidecar" to "Linkerd sidecar"

### Why
- Consistency: All documentation and examples now use Linkerd as the service mesh standard
- Clarity: Specific examples are more helpful than generic "service mesh" references
- Project standard: Linkerd is the service mesh used in the k0rdent environment

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

---

## [2025-12-03 21:40] - Rename apiConfig to bindcarConfig and Add Volume Support

**Author:** Erick Bourgeois

### Changed
- `src/crd.rs`: Renamed `ApiContainerConfig` to `BindcarConfig`
  - Renamed struct to better reflect its purpose as the Bindcar sidecar configuration
- `src/crd.rs`: Renamed field `api_config` to `bindcar_config`
  - In `Bind9InstanceSpec` - instance-level Bindcar configuration
  - In `Bind9Config` - cluster-level Bindcar configuration (inherited by all instances)
- All code and test references updated to use `bindcar_config` consistently

### Added
- `src/crd.rs`: Added volume and environment support to `BindcarConfig`
  - `env_vars: Option<Vec<EnvVar>>` - Environment variables for the Bindcar container
  - `volumes: Option<Vec<Volume>>` - Volumes that can be mounted by the Bindcar container
  - `volume_mounts: Option<Vec<VolumeMount>>` - Volume mounts for the Bindcar container

### Fixed
- `src/bind9_resources.rs`: Fixed `bindcarConfig` inheritance from cluster to instances
  - Added `bindcar_config` field to `DeploymentConfig` struct
  - Updated `resolve_deployment_config()` to resolve `bindcar_config` from cluster global config
  - Instance-level `bindcarConfig` now correctly overrides cluster-level configuration
  - `build_pod_spec()` now receives resolved `bindcar_config` instead of only instance-level config
  - Fixes issue where `bind9cluster.spec.global.bindcarConfig.image` was not being honored
- `Cargo.toml`: Switched from native TLS (OpenSSL) to rustls for HTTP client
  - Changed `reqwest` to use `rustls-tls` feature instead of default native-tls
  - Eliminates OpenSSL dependency, enabling clean musl static builds
  - Docker builds now succeed without OpenSSL build dependencies
- `Dockerfile`: Simplified build process by removing OpenSSL dependencies
  - Removed unnecessary packages: `pkg-config`, `libssl-dev`, `musl-dev`, `make`, `perl`
  - Pure Rust TLS stack (rustls) works perfectly with musl static linking

### Impact
- [x] Breaking change - Field name changed from `apiConfig` to `bindcarConfig` in CRDs
- [ ] Requires cluster rollout
- [x] Config change only
- [ ] Documentation only

### Why
- Improved naming consistency: "bindcar" better represents the sidecar's purpose
- Added flexibility: Users can now customize environment variables and mount volumes in the Bindcar sidecar
- Docker builds: rustls (pure Rust TLS) ensures reliable static builds across all platforms without C dependencies

---

## [2025-12-03 11:45] - Integrate HTTP API Sidecar (bindcar) for BIND9 Management

**Author:** Erick Bourgeois

### Added
- `src/bind9.rs`: New HTTP API integration for all RNDC operations
  - Added `create_zone_http()` method for zone creation via API
  - Converted `exec_rndc_command()` to use HTTP endpoints instead of RNDC protocol
  - Added `HttpClient` and ServiceAccount token authentication
  - Added request/response types: `CreateZoneRequest`, `ZoneResponse`, `ServerStatusResponse`
- `src/bind9_resources.rs`: API sidecar container deployment
  - Added `build_api_sidecar_container()` function to create API sidecar
  - Modified `build_pod_spec()` to include API sidecar alongside BIND9 container
  - Updated `build_service()` to expose API on port 80 (maps to container port 8080)
- `src/crd.rs`: New `BindcarConfig` struct for API sidecar configuration
  - Added `bindcar_config` field to `Bind9InstanceSpec` and `Bind9Config`
  - Configurable: image, imagePullPolicy, resources, port, logLevel
- `Cargo.toml`: Added `reqwest` dependency for HTTP client

### Changed
- `templates/named.conf.tmpl`: RNDC now listens only on localhost (127.0.0.1)
  - Changed from `inet * port 953 allow { any; }` to `inet 127.0.0.1 port 953 allow { localhost; }`
  - API sidecar now handles all external RNDC access via HTTP
- `src/bind9_resources.rs`: Service port configuration
  - **Removed:** RNDC port 953 from Service (no longer exposed externally)
  - **Added:** HTTP port 80 → API sidecar port (default 8080, configurable)
  - Service now exposes: DNS (53 TCP/UDP) and API (80 HTTP)

### Why
**Architecture Migration:** Moved from direct RNDC protocol access to HTTP API sidecar pattern for better:
- **Security**: RNDC no longer exposed to network, only accessible via localhost
- **Flexibility**: RESTful API is easier to integrate with modern tooling
- **Standardization**: HTTP on port 80 follows standard conventions
- **Scalability**: API sidecar can handle authentication, rate limiting, etc.

The `bindcar` sidecar runs alongside BIND9 in the same pod, sharing volumes for zone files and RNDC keys.

### Impact
- [x] Breaking change (RNDC port no longer exposed, all management via HTTP API)
- [x] Requires cluster rollout (new pod template with sidecar container)
- [x] Config change (new `bindcar_config` CRD field)
- [ ] Documentation only

### Technical Details

**HTTP API Endpoints** (in `bindcar` sidecar):
- `POST /api/v1/zones` - Create zone
- `POST /api/v1/zones/:name/reload` - Reload zone
- `DELETE /api/v1/zones/:name` - Delete zone
- `POST /api/v1/zones/:name/freeze` - Freeze zone
- `POST /api/v1/zones/:name/thaw` - Thaw zone
- `POST /api/v1/zones/:name/notify` - Notify secondaries
- `GET /api/v1/zones/:name/status` - Zone status
- `GET /api/v1/server/status` - Server status

**Default Sidecar Configuration:**
```yaml
apiConfig:
  image: ghcr.io/firestoned/bindcar:latest
  imagePullPolicy: IfNotPresent
  port: 8080
  logLevel: info
```

**Authentication:** Uses Kubernetes ServiceAccount tokens mounted at `/var/run/secrets/kubernetes.io/serviceaccount/token`

**Shared Volumes:**
- `/var/cache/bind` - Zone files (shared between BIND9 and API)
- `/etc/bind/keys` - RNDC keys (shared, read-only for API)

## [2025-12-02 14:30] - Fix RNDC addzone Command Quoting

**Author:** Erick Bourgeois

### Fixed
- `src/bind9.rs`: Removed extra single quotes from `addzone` command formatting that caused "unknown option" errors in BIND9
- `src/bind9_tests.rs`: Removed unused `RndcError` import

### Why
The `addzone` RNDC command was wrapping the zone configuration in single quotes, which caused BIND9 to fail with:
```
addzone: unknown option '''
```

The rndc library already handles proper quoting, so the extra quotes around the zone configuration were being interpreted as part of the command itself rather than string delimiters.

### Impact
- [x] Breaking change (fixes broken zone creation)
- [ ] Requires cluster rollout
- [ ] Config change only
- [ ] Documentation only

### Details
Changed from:
```rust
addzone {zone_name} '{{ type {zone_type}; file "{zone_file}"; allow-update {{ key "{update_key_name}"; }}; }};'
```

To:
```rust
addzone {zone_name} {{ type {zone_type}; file "{zone_file}"; allow-update {{ key "{update_key_name}"; }}; }};
```

## [2025-12-02 14:27] - Increase Page TOC Font Size

**Author:** Erick Bourgeois

### Changed
- `docs/theme/custom.css`: Increased font sizes for page-toc navigation elements

### Why
The font sizes for the in-page table of contents (page-toc) on the right side were too small, making navigation difficult to read.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

### Details
Increased font sizes:
- `.page-toc-title`: 0.875rem → 1rem
- `.page-toc nav`: 0.875rem → 1rem
- `.page-toc nav .toc-h3`: 0.8125rem → 0.9375rem
- `.page-toc nav .toc-h4`: 0.8125rem → 0.9375rem

## [2025-12-02 10:15] - Fix ASCII Diagram Alignment in Documentation

**Author:** Erick Bourgeois

### Fixed
- `docs/src/guide/multi-region.md`: Fixed alignment of region boxes in Primary-Secondary deployment pattern diagram
- `docs/src/advanced/ha.md`: Fixed vertical line alignment in Active-Passive HA pattern diagram
- `docs/src/advanced/ha.md`: Fixed vertical line alignment in Anycast pattern diagram
- `docs/src/advanced/zone-transfers.md`: Fixed line spacing in NOTIFY message flow diagram
- `docs/src/development/architecture.md`: Fixed vertical line alignment in Data Flow diagram showing bindy-operator and BIND9 pod structure
- `docs/src/development/cluster-architecture.md`: Reorganized and aligned Bind9Cluster architecture diagram for better readability
- `docs/src/concepts/architecture.md`: Fixed vertical line alignment in High-Level Architecture diagram

### Why
ASCII diagrams had misaligned vertical lines, shifted boxes, and inconsistent spacing that made them difficult to read in monospace environments. This affected the visual clarity of architecture documentation.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

## [2025-12-02 00:30] - Add Structured RNDC Error Parsing with RndcError Type

**Author:** Erick Bourgeois

### Added
- [src/bind9.rs:71-128](src/bind9.rs#L71-L128): New `RndcError` type with structured fields (command, error, details)
- [src/bind9_tests.rs:1866-1945](src/bind9_tests.rs#L1866-L1945): Comprehensive unit tests for RNDC error parsing (8 test cases)

### Fixed
- [src/bind9.rs:415-444](src/bind9.rs#L415-L444): Enhanced `exec_rndc_command` to parse structured RNDC errors
- [src/bind9.rs:520-522](src/bind9.rs#L520-L522): Simplified `zone_exists` to rely on improved error handling

### Why
**Root Cause:** The `exec_rndc_command` method was returning `Ok(response_text)` even when BIND9 included error messages in the response (like "not found", "does not exist", "failed", or "error"). This caused ALL RNDC command methods to incorrectly treat failures as successes.

**Impact on All RNDC Methods:**
- `zone_exists()` - Returned `true` for non-existent zones → zones not created
- `add_zone()` - Skipped zone creation thinking zones already existed
- `reload_zone()` - Silent failures if zone didn't exist
- `delete_zone()` - No error if zone already deleted
- `freeze_zone()`, `thaw_zone()` - Silent failures
- `zone_status()` - Returned "success" with error text
- `retransfer()`, `notify()` - Could fail silently

**Bug Symptoms:**
- Zones not being provisioned despite CRD creation
- Silent failures during reconciliation
- Inconsistent state between Kubernetes resources and BIND9 configuration
- No error logs despite actual BIND9 failures

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Critical bug fix - affects all RNDC operations
- [ ] Config change only

### Technical Details

**Root Fix in `exec_rndc_command`:**

**Before:**
```rust
async fn exec_rndc_command(...) -> Result<String> {
    // ... execute command ...
    Ok(result.text.unwrap_or_default())  // ❌ Always returns Ok, even with error text
}
```

**After (with structured error parsing):**
```rust
// New RndcError type for structured error handling
#[derive(Debug, Clone, thiserror::Error)]
#[error("RNDC command '{command}' failed: {error}")]
pub struct RndcError {
    pub command: String,    // e.g., "zonestatus"
    pub error: String,      // e.g., "not found"
    pub details: Option<String>, // e.g., "no matching zone 'example.com' in any view"
}

async fn exec_rndc_command(...) -> Result<String> {
    // ... execute command ...
    let response_text = result.text.unwrap_or_default();

    // Parse structured RNDC errors (format: "rndc: 'command' failed: error\ndetails")
    if let Some(rndc_error) = RndcError::parse(&response_text) {
        error!(
            server = %server_name,
            command = %rndc_error.command,
            error = %rndc_error.error,
            details = ?rndc_error.details,
            "RNDC command failed with structured error"
        );
        return Err(rndc_error.into());
    }

    // Fallback for unstructured errors
    if response_text.to_lowercase().contains("failed") {
        return Err(anyhow!("RNDC command returned error: {response_text}"));
    }

    Ok(response_text)
}
```

**Simplified `zone_exists` (now that errors are properly detected):**
```rust
pub async fn zone_exists(...) -> bool {
    self.zone_status(zone_name, server, key_data).await.is_ok()
}
```

**Benefits:**
1. ✅ **Structured Error Information** - Errors now include command name, error type, and details
2. ✅ **Better Debugging** - Logs show structured fields (command, error, details) for easier troubleshooting
3. ✅ **Type-Safe Error Handling** - Callers can match on specific error types (e.g., "not found" vs "already exists")
4. ✅ **All RNDC Commands Fixed** - Zone operations, reloads, transfers all properly detect failures
5. ✅ **Zone Provisioning Works** - Zones are created when they should be (no more silent skipping)
6. ✅ **Comprehensive Tests** - 8 unit tests cover various error formats and edge cases

**Example Error Output:**
```
rndc: 'zonestatus' failed: not found
no matching zone 'example.com' in any view
```
Parsed into:
```rust
RndcError {
    command: "zonestatus",
    error: "not found",
    details: Some("no matching zone 'example.com' in any view")
}
```

## [2025-12-02 00:16] - Add Interactive Zoom and Pan for Mermaid Diagrams

**Author:** Erick Bourgeois

### Added
- [docs/mermaid-init.js:20-120](docs/mermaid-init.js#L20-L120): Integrated zoom and pan functionality directly into Mermaid initialization to prevent re-rendering loops
- [docs/theme/custom.css:120-129](docs/theme/custom.css#L120-L129): Minimal CSS to enable overflow for Mermaid SVG diagrams

### Why
Complex architecture diagrams and flowcharts in the documentation (like the ones in [architecture.md](docs/src/concepts/architecture.md)) can be difficult to read due to their size and detail. Interactive zoom and pan functionality significantly improves user experience by:
- Allowing readers to zoom in on specific parts of large diagrams
- Enabling panning to explore different sections of complex flowcharts
- Providing easy reset functionality via double-click
- Making complex architecture more accessible

This enhancement makes technical documentation more accessible and easier to navigate, especially for new users learning about the system architecture.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Documentation enhancement only
- [ ] Config change only

### Features
**User Interactions:**
- **Scroll to Zoom**: Use mouse wheel to zoom in/out (0.5x to 5x scale range)
- **Click and Drag to Pan**: Move around large diagrams
- **Double-Click to Reset**: Return to original view
- **Visual Feedback**: Cursor changes to "grab" hand when hovering over diagrams

**Technical Details:**
- Zoom/pan integrated directly into `mermaid-init.js` to prevent infinite rendering loops
- Uses `svg.dataset.zoomEnabled` flag to prevent re-initialization
- Wraps SVG content in `<g>` element for transform operations
- Multiple initialization strategies (Mermaid callback, window load, MutationObserver)
- Console logging for troubleshooting
- Minimal CSS footprint - only sets overflow properties

**Implementation Notes:**
Based on the proven approach from virtrigaud project. Key difference from initial implementation:
- Zoom/pan code integrated into existing mermaid-init.js instead of separate file
- Prevents infinite loop by checking `svg.dataset.zoomEnabled` before initialization
- Simpler CSS that only handles overflow, not styling

## [2025-12-02 00:50] - Make Author Attribution Mandatory in Changelog

**Author:** Erick Bourgeois

### Changed
- [CLAUDE.md:219-224](CLAUDE.md#L219-L224): Made author attribution a **CRITICAL REQUIREMENT** for all changelog entries
- [CLAUDE.md:866-867](CLAUDE.md#L866-L867): Added author verification to PR/Commit checklist
- [CHANGELOG.md](CHANGELOG.md): Added `**Author:** Erick Bourgeois` to all existing changelog entries (6 entries total)

### Why
In a regulated banking environment, all code changes must be auditable and traceable to a specific person for accountability and compliance purposes. Author attribution in the changelog:
- Provides clear accountability for all changes
- Enables audit trails for regulatory compliance
- Helps track who requested or approved changes
- Supports incident investigation and root cause analysis
- Ensures proper attribution for code contributions

Without mandatory author attribution, it's impossible to determine who was responsible for specific changes, which violates compliance requirements.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Documentation policy change
- [x] All existing entries updated

### Details
**New Requirements**:
- Every changelog entry MUST include `**Author:** [Name]` line immediately after the title
- NO exceptions - this is a critical compliance requirement
- If author is unknown, use "Unknown" but investigate to identify proper author
- Added to PR/Commit checklist as mandatory verification step

**Format**:
```markdown
## [YYYY-MM-DD HH:MM] - Brief Title

**Author:** [Author Name]

### Changed
...
```

**Retroactive Updates**:
All 6 existing changelog entries have been updated with author attribution:
- ✅ [2025-12-02 00:45] - Consolidate All Constants into Single Module
- ✅ [2025-12-02 00:30] - Complete All DNS Record Types Implementation
- ✅ [2025-12-02 00:15] - Eliminate Magic Numbers from Codebase
- ✅ [2025-12-01 23:50] - Add Magic Numbers Policy to Code Quality Standards
- ✅ [2025-12-01 23:19] - Implement Dynamic DNS Record Updates (RFC 2136)
- ✅ [2025-12-01 22:29] - Fix DNSZone Creation with allow-new-zones and Correct Paths

## [2025-12-02 00:45] - Consolidate All Constants into Single Module

**Author:** Erick Bourgeois

### Changed
- [src/constants.rs:9-53](src/constants.rs#L9-L53): Merged all API constants from `api_constants.rs` into `constants.rs` under new "API Constants" section
- [src/bind9_resources.rs:9-14](src/bind9_resources.rs#L9-L14): Updated imports to use `constants` instead of `api_constants`
- [src/bind9_resources_tests.rs:8](src/bind9_resources_tests.rs#L8): Updated imports to use `constants` instead of `api_constants`
- [src/lib.rs:60-66](src/lib.rs#L60-L66): Removed `pub mod api_constants;` module declaration

### Removed
- [src/api_constants.rs](src/api_constants.rs): Deleted file - all constants moved to `constants.rs`

### Why
Having constants split across multiple files (`api_constants.rs` and `constants.rs`) violated the single source of truth principle and made it harder to find constants. This change:
- Consolidates ALL constants (API, DNS, Kubernetes, etc.) into a single `constants.rs` file
- Improves discoverability - developers only need to check one file for constants
- Follows the CLAUDE.md policy: "Use Global Constants for Repeated Strings"
- Eliminates confusion about where to add new constants

### Impact
- [ ] Breaking change (internal refactor only)
- [ ] Requires cluster rollout
- [x] Code organization improvement
- [x] All tests passing

### Details
**Organization**:
All constants are now grouped by category in `src/constants.rs`:
1. API Constants (CRD kinds, API group/version)
2. DNS Protocol Constants (ports, TTLs, timeouts)
3. Kubernetes Health Check Constants
4. Controller Error Handling Constants
5. Leader Election Constants
6. BIND9 Version Constants
7. Runtime Constants
8. Replica Count Constants

**Migration**:
- All imports of `crate::api_constants::*` changed to `crate::constants::*`
- No functional changes - purely organizational

**Code Quality**:
- ✅ `cargo fmt` passed
- ✅ `cargo clippy -- -D warnings` passed
- ✅ `cargo test` passed (217 tests, 8 ignored)

## [2025-12-02 00:30] - Complete All DNS Record Types Implementation

**Author:** Erick Bourgeois

### Added
- [src/bind9.rs:869-940](src/bind9.rs#L869-L940): Implemented `add_aaaa_record()` with RFC 2136 dynamic DNS update for IPv6 addresses
- [src/bind9.rs:942-1005](src/bind9.rs#L942-L1005): Implemented `add_mx_record()` with RFC 2136 dynamic DNS update for mail exchange records
- [src/bind9.rs:1007-1069](src/bind9.rs#L1007-L1069): Implemented `add_ns_record()` with RFC 2136 dynamic DNS update for nameserver delegation
- [src/bind9.rs:1071-1165](src/bind9.rs#L1071-L1165): Implemented `add_srv_record()` with RFC 2136 dynamic DNS update for service location records
- [src/bind9.rs:1167-1302](src/bind9.rs#L1167-L1302): Implemented `add_caa_record()` with RFC 2136 dynamic DNS update for certificate authority authorization
- [Cargo.toml:41](Cargo.toml#L41): Added `url` crate dependency for CAA record iodef URL parsing
- [src/bind9_tests.rs:753](src/bind9_tests.rs#L753): Added `#[ignore]` attribute to AAAA record test
- [src/bind9_tests.rs:826](src/bind9_tests.rs#L826): Added `#[ignore]` attribute to MX record test
- [src/bind9_tests.rs:851](src/bind9_tests.rs#L851): Added `#[ignore]` attribute to NS record test
- [src/bind9_tests.rs:875](src/bind9_tests.rs#L875): Added `#[ignore]` attribute to SRV record test
- [src/bind9_tests.rs:905](src/bind9_tests.rs#L905): Added `#[ignore]` attribute to CAA record test

### Changed
- [src/bind9.rs:646](src/bind9.rs#L646): Fixed TSIG signer creation to convert `TSIG_FUDGE_TIME_SECS` from `u64` to `u16`
- [src/constants.rs:32](src/constants.rs#L32): Fixed clippy warning by adding separator to `DEFAULT_SOA_EXPIRE_SECS` constant (604_800)

### Why
The user requested implementation of ALL DNS record types with actual dynamic DNS updates to BIND9 using RFC 2136 protocol. Previously, only A, CNAME, and TXT records were implemented. This change completes the implementation by adding:
- **AAAA**: IPv6 address records for dual-stack support
- **MX**: Mail exchange records with priority for email routing
- **NS**: Nameserver records for DNS delegation
- **SRV**: Service location records with priority, weight, and port
- **CAA**: Certificate authority authorization with support for issue, issuewild, and iodef tags

All record implementations use TSIG authentication for security and execute in `spawn_blocking` to handle synchronous hickory-client API.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Feature addition
- [x] All tests passing

### Details
**Technical Implementation**:
- All record types use hickory-client for DNS updates over UDP port 53
- TSIG authentication using bindy-operator key for all updates
- Proper type conversions: i32→u16 for SRV priority/weight/port, i32→u8 for CAA flags
- CAA record supports three tags: issue, issuewild, iodef
- All methods execute in `tokio::task::spawn_blocking` since hickory-client is synchronous
- Default TTL of 300 seconds from `DEFAULT_DNS_RECORD_TTL_SECS` constant

**Testing**:
- All placeholder tests updated to `#[ignore]` since they require real BIND9 server
- Tests can be run with `cargo test -- --ignored` when BIND9 server is available
- All non-ignored tests passing (217 tests)

**Code Quality**:
- ✅ `cargo fmt` passed
- ✅ `cargo clippy -- -D warnings` passed
- ✅ `cargo test` passed (217 tests, 8 ignored)
- All rustdoc comments updated with accurate error descriptions
- Proper error handling with context messages for all failure scenarios

## [2025-12-02 00:15] - Eliminate Magic Numbers from Codebase

**Author:** Erick Bourgeois

### Added
- [src/constants.rs](src/constants.rs): Created new global constants module with all numeric constants
  - DNS protocol constants (ports, TTLs, timeouts)
  - Kubernetes health check constants (probe delays, periods, thresholds)
  - Controller error handling constants
  - Leader election constants
  - BIND9 version constants
  - Runtime constants
  - Replica count constants

### Changed
- [src/bind9.rs](src/bind9.rs): Replaced all magic numbers with named constants from `constants` module
  - TTL values now use `DEFAULT_DNS_RECORD_TTL_SECS` and `DEFAULT_ZONE_TTL_SECS`
  - SOA record values use `DEFAULT_SOA_REFRESH_SECS`, `DEFAULT_SOA_RETRY_SECS`, etc.
  - Port numbers use `DNS_PORT` and `RNDC_PORT` constants
- [src/bind9_resources.rs](src/bind9_resources.rs): Updated all numeric literals to use named constants
  - Health check probes use `LIVENESS_*` and `READINESS_*` constants
  - Container ports use `DNS_PORT` and `RNDC_PORT`
- [src/main.rs](src/main.rs): Replaced runtime worker thread count with `TOKIO_WORKER_THREADS`
- [src/reconcilers/bind9cluster.rs](src/reconcilers/bind9cluster.rs): Updated error requeue duration to use `ERROR_REQUEUE_DURATION_SECS`
- [src/lib.rs](src/lib.rs): Added `pub mod constants;` export

### Why
Magic numbers (numeric literals other than 0 or 1) scattered throughout code reduce readability and maintainability. This change:
- Makes all numeric values self-documenting through descriptive constant names
- Allows values to be changed in a single location (`src/constants.rs`)
- Improves code readability by explaining the purpose of each number
- Enforces the "Use Global Constants for Repeated Strings" policy from CLAUDE.md
- Eliminates the need to search the codebase to understand what specific numbers mean

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Code quality improvement
- [x] All tests passing

### Details
**Constants Organization**:
- Grouped by category (DNS, Kubernetes, Controller, etc.)
- Each constant has a descriptive name explaining its purpose
- Rustdoc comments explain what each value represents
- Constants use proper numeric separators for readability (e.g., `604_800` instead of `604800`)

**Verification**:
```bash
# Before: Many magic numbers throughout codebase
# After: All numeric literals (except 0 and 1) are named constants
```

**Examples**:
- ✅ Before: `ttl.unwrap_or(300)`
- ✅ After: `ttl.unwrap_or(DEFAULT_DNS_RECORD_TTL_SECS)`

- ✅ Before: `initial_delay_seconds: Some(30)`
- ✅ After: `initial_delay_seconds: Some(LIVENESS_INITIAL_DELAY_SECS)`

## [2025-12-01 23:50] - Add Magic Numbers Policy to Code Quality Standards

**Author:** Erick Bourgeois

### Changed
- [CLAUDE.md:349-454](CLAUDE.md#L349-L454): Added "Magic Numbers Rule" to Rust Style Guidelines section

### Why
Magic numbers (numeric literals other than 0 or 1) scattered throughout code reduce readability and maintainability. Named constants make code self-documenting and allow values to be changed in a single location.

This policy enforces that:
- All numeric literals except `0` and `1` must be declared as named constants
- Constant names must explain the *purpose* of the value, not just restate it
- Constants should be grouped logically at module or crate level

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [ ] Config change only
- [x] Documentation only

### Details
**New Requirements**:
- No numeric literals other than `0` or `1` are allowed in code
- All numbers must be declared as named constants with descriptive names
- Special cases covered: unit conversions, array indexing, buffer sizes
- Verification command provided to find magic numbers in codebase

**Examples Added**:
- ✅ GOOD: `const DEFAULT_ZONE_TTL: u32 = 3600;`
- ❌ BAD: `ttl.unwrap_or(3600)` (no explanation of what 3600 means)

This aligns with existing code quality requirements for using global constants for repeated strings.

## [2025-12-01 23:19] - Implement Dynamic DNS Record Updates (RFC 2136)

**Author:** Erick Bourgeois

### Added
- [Cargo.toml:39-40](Cargo.toml#L39-L40): Added `hickory-client` and `hickory-proto` dependencies with `dnssec` feature for dynamic DNS updates
- [src/bind9.rs:619-650](src/bind9.rs#L619-L650): Implemented `create_tsig_signer()` helper method to convert RNDC key data to hickory TSIG signer
- [src/bind9.rs:652-741](src/bind9.rs#L652-L741): Implemented `add_a_record()` with actual RFC 2136 dynamic DNS update using hickory-client
- [src/bind9.rs:743-806](src/bind9.rs#L743-L806): Implemented `add_cname_record()` with RFC 2136 dynamic DNS update
- [src/bind9.rs:808-867](src/bind9.rs#L808-L867): Implemented `add_txt_record()` with RFC 2136 dynamic DNS update

###Changed
- [src/bind9.rs:45-55](src/bind9.rs#L45-L55): Added hickory-client imports for DNS client, TSIG authentication, and record types
- [src/bind9_tests.rs:727-750](src/bind9_tests.rs#L727-L750): Updated record tests to mark them as `#[ignore]` since they now require a real BIND9 server with TSIG authentication

### Why
The operator needs to dynamically update DNS records in BIND9 zones without reloading the entire zone file. The previous implementation only logged what would be done. This change implements actual RFC 2136 dynamic DNS updates using TSIG authentication for security.

**Use case**: When a `DNSRecord` custom resource is created/updated in Kubernetes, the operator should immediately update the DNS record in the running BIND9 server without disrupting other records or requiring a zone reload.

### Impact
- [x] Breaking change (placeholder methods now make actual DNS updates)
- [ ] Requires cluster rollout
- [x] Requires BIND9 configuration with `allow-update { key "bindy-operator"; };`
- [x] Feature enhancement

### Details
**Technical Implementation**:
- Uses hickory-client library for DNS protocol implementation
- TSIG (Transaction Signature) authentication using HMAC algorithms (MD5, SHA1, SHA224, SHA256, SHA384, SHA512)
- Updates sent over UDP to BIND9 server on port 53
- All methods execute in `tokio::task::spawn_blocking` since hickory-client is synchronous
- Response codes validated (NoError expected, errors returned with context)

**Security**:
- TSIG key authentication prevents unauthorized DNS updates
- TODO: Create separate key for zone updates (currently reuses bindy-operator RNDC key)

**Error Handling**:
- Connection failures: Returns error with server address context
- Invalid parameters: Returns error with parameter value context
- DNS update rejection: Returns error with response code
- Task panic: Returns error with context wrapper

**Testing**:
- Tests marked with `#[ignore]` attribute
- Tests require:
  - Running BIND9 server
  - TSIG key configured
  - Zone with `allow-update` directive
- Can be run with: `cargo test -- --ignored`

## [2025-12-01 22:29] - Fix DNSZone Creation with allow-new-zones and Correct Paths

**Author:** Erick Bourgeois

### Changed
- [templates/named.conf.options.tmpl:10](templates/named.conf.options.tmpl#L10): Added `allow-new-zones yes;` directive to BIND9 configuration
- [src/reconcilers/dnszone.rs:115](src/reconcilers/dnszone.rs#L115): Changed zone file path from `/var/lib/bind/` to `/var/cache/bind/`
- [src/reconcilers/dnszone.rs:153-156](src/reconcilers/dnszone.rs#L153-L156): Removed unnecessary `rndc reload` loop after `rndc addzone`
- [src/bind9.rs:524-543](src/bind9.rs#L524-L543): Added `allow-update { key "<update_key_name>"; }` to zone configuration in `add_zone()` method

### Why
BIND9 was refusing to create zones dynamically via `rndc addzone` because the `allow-new-zones yes;` directive was missing from named.conf. Without this directive, BIND9 rejects all `addzone` commands with "permission denied" errors.

Additionally:
- Zone files must be in `/var/cache/bind/` (writable directory) not `/var/lib/bind/` (read-only in container)
- The `rndc reload` after `addzone` is unnecessary and wrong - `addzone` automatically loads the zone
- Dynamic DNS updates require `allow-update` directive in zone configuration

### Impact
- [ ] Breaking change
- [x] Requires cluster rollout (ConfigMap must be updated)
- [x] Bug fix
- [x] Enables dynamic zone creation

### Details
**Root Cause**:
User identified: "the real fix is to add 'allow-new-zones yes;' to named.conf"

**BIND9 Behavior**:
- Without `allow-new-zones yes;`: `rndc addzone` fails with "permission denied"
- With `allow-new-zones yes;`: `rndc addzone` creates zone and loads it automatically
- Zone file path must be writable by named process

**Zone Configuration**:
```
addzone example.com '{ type primary; file "/var/cache/bind/example.com.zone"; allow-update { key "bindy-operator"; }; };'
```

**TODO**: Create separate TSIG key for zone updates (currently reuses bindy-operator RNDC key)

**Verification**:
```bash
# In BIND9 pod:
rndc zonestatus example.com  # Should show zone details
rndc showzone example.com    # Should show zone configuration
ls -la /var/cache/bind/      # Should show zone files
```
