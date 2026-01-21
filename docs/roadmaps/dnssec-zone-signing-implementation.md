# DNSSEC Zone Signing Implementation Roadmap

**Status:** Planning
**Priority:** Medium
**Complexity:** High
**Impact:** Security enhancement - cryptographic validation of DNS records
**Target Completion:** TBD
**Author:** Erick Bourgeois
**Created:** 2026-01-02

---

## Executive Summary

Implement full DNSSEC zone signing capabilities in bindy to enable cryptographic authentication of DNS records. Currently, bindy only supports DNSSEC **validation** (verifying signatures from upstream servers). This roadmap details adding DNSSEC **signing** (creating signatures for zones served by bindy).

### Current State
- ✅ DNSSEC validation of upstream responses (`dnssec-validation yes/no`)
- ❌ DNSSEC zone signing (zones are served unsigned)
- ❌ DNSSEC key management
- ❌ DNSSEC policy configuration

### Target State
- ✅ Automatic DNSSEC key generation (KSK + ZSK)
- ✅ Automatic zone signing with configurable policies
- ✅ Key rotation with configurable intervals
- ✅ Support for multiple DNSSEC algorithms (ECDSAP256SHA256, RSASHA256, etc.)
- ✅ DS record generation for parent zone delegation
- ✅ Per-zone DNSSEC policy override capability

---

## Background

### Why DNSSEC Zone Signing?

**Security Benefits:**
- **Authentication**: Proves DNS responses came from authoritative source
- **Integrity**: Detects tampering with DNS data in transit
- **Non-existence Proof**: NSEC/NSEC3 records prove a domain doesn't exist
- **Cache Poisoning Protection**: Prevents DNS spoofing attacks

**Regulatory Compliance:**
- **NIST 800-53**: SC-20, SC-21, SC-23 (DNS integrity and authenticity)
- **Banking/Finance**: Required for critical infrastructure DNS in many jurisdictions
- **Zero-Trust Architecture**: DNSSEC is foundational for service mesh trust chains

### BIND9 DNSSEC Evolution

**Modern DNSSEC (BIND 9.16+):**
- Uses `dnssec-policy` for declarative key management
- Automatic key generation, rollover, and signing
- Simplified configuration vs. manual key management

**Legacy DNSSEC (BIND 9.15 and earlier):**
- Required manual key generation with `dnssec-keygen`
- Used `auto-dnssec maintain;` in zone configuration
- Complex key rollover procedures

**Bindy Target:** Use modern `dnssec-policy` approach (BIND 9.16+)

---

## Technical Architecture

### DNSSEC Components

```
┌─────────────────────────────────────────────────────────────┐
│                    Bind9Cluster CRD                         │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ spec.global.dnssec:                                    │ │
│  │   validation: true              # Existing             │ │
│  │   signing:                       # NEW                 │ │
│  │     enabled: true                                      │ │
│  │     policy: "default"            # Built-in policy     │ │
│  │     algorithm: "ECDSAP256SHA256" # ECDSA P-256         │ │
│  │     kskLifetime: 365d            # Key Signing Key     │ │
│  │     zskLifetime: 90d             # Zone Signing Key    │ │
│  │     nsec3: true                  # Use NSEC3 vs NSEC   │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              ConfigMap: named.conf.options                  │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ dnssec-validation yes;                                 │ │
│  │ dnssec-policy "default";  # Applied to all zones       │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              ConfigMap: named.conf (zones)                  │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ zone "example.com" {                                   │ │
│  │   type primary;                                        │ │
│  │   file "/var/cache/bind/db.example.com";              │ │
│  │   dnssec-policy "default";  # Inherit or override      │ │
│  │   inline-signing yes;        # Auto-generated          │ │
│  │ };                                                     │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│           PersistentVolume: DNSSEC Keys                     │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ /var/cache/bind/keys/                                  │ │
│  │   ├── Kexample.com.+013+12345.key     # ZSK public     │ │
│  │   ├── Kexample.com.+013+12345.private # ZSK private    │ │
│  │   ├── Kexample.com.+013+54321.key     # KSK public     │ │
│  │   └── Kexample.com.+013+54321.private # KSK private    │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              Status: DS Record for Parent                   │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ DNSZone.status.dnssec:                                 │ │
│  │   signed: true                                         │ │
│  │   dsRecords:                                           │ │
│  │     - "example.com. IN DS 54321 13 2 ABC123..."       │ │
│  │   keyTag: 54321                                        │ │
│  │   algorithm: ECDSAP256SHA256                           │ │
│  │   nextKeyRollover: "2026-04-02T00:00:00Z"             │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Key Management Strategy

**Self-Healing Architecture - Multiple Key Sources:**

bindy supports flexible DNSSEC key management with three options:

#### Option 1: User-Supplied Keys (Recommended for Production)
**Pattern:** User manages keys externally, bindy consumes from Kubernetes Secrets

**Benefits:**
- **External Secret Management**: Integrates with ExternalSecrets, Sealed Secrets, Vault
- **User-Controlled Rotation**: Customer controls when and how keys are rotated
- **GitOps Friendly**: Keys versioned and managed via GitOps workflows
- **Multi-Cluster**: Same keys can be shared across clusters
- **Compliance**: Meets requirements for external key management (HSM, KMS, etc.)

**Implementation:**
```yaml
spec:
  global:
    dnssec:
      signing:
        enabled: true
        keysFrom:
          secretRef:
            name: "dnssec-keys-example-com"  # User-provided secret
```

**Secret Format:**
```yaml
apiVersion: v1
kind: Secret
metadata:
  name: dnssec-keys-example-com
type: Opaque
data:
  Kexample.com.+013+12345.key: <base64-encoded-public-key>
  Kexample.com.+013+12345.private: <base64-encoded-private-key>
  Kexample.com.+013+54321.key: <base64-encoded-ksk-public>
  Kexample.com.+013+54321.private: <base64-encoded-ksk-private>
```

**ExternalSecrets Example:**
```yaml
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: dnssec-keys-sync
spec:
  refreshInterval: 1h
  secretStoreRef:
    name: vault-backend
    kind: SecretStore
  target:
    name: dnssec-keys-example-com
  data:
    - secretKey: Kexample.com.+013+12345.key
      remoteRef:
        key: dnssec/example.com/zsk/public
    - secretKey: Kexample.com.+013+12345.private
      remoteRef:
        key: dnssec/example.com/zsk/private
    # ... KSK keys ...
```

#### Option 2: Auto-Generated Keys (Self-Healing)
**Pattern:** BIND9 generates keys automatically, bindy extracts and stores in Secrets

**Benefits:**
- **Zero Configuration**: Works out of the box, no manual key generation
- **Self-Healing**: Keys auto-regenerated if lost/corrupted
- **Development Friendly**: Quick setup for dev/test environments
- **BIND9 Native**: Leverages BIND9's built-in `dnssec-policy` automation

**Implementation:**
```yaml
spec:
  global:
    dnssec:
      signing:
        enabled: true
        autoGenerate: true  # BIND9 generates keys
        exportToSecret: true  # Operator exports to Secret for backup
```

**How it Works:**
1. BIND9 generates keys in `/var/cache/bind/keys/` (emptyDir)
2. Operator watches for new keys via bindcar API
3. Operator exports keys to Secret `dnssec-keys-<zone>-generated`
4. On pod restart: Operator restores keys from Secret to emptyDir
5. **Self-Healing**: If Secret is deleted, BIND9 regenerates new keys

**Trade-offs:**
- ⚠️ Key loss triggers regeneration (breaks DNSSEC chain temporarily)
- ⚠️ DS records in parent zone must be updated after key regeneration
- ✅ Zone becomes functional again after DS update (self-healing complete)

#### Option 3: Persistent Storage (Legacy/Compatibility)
**Pattern:** Keys stored in PersistentVolume (traditional approach)

**Benefits:**
- **No Operator Dependency**: Keys persist independently of operator
- **Familiar Pattern**: Traditional BIND9 deployment model

**Drawbacks:**
- ❌ Not cloud-native (tight coupling to storage)
- ❌ PVC lifecycle management complexity
- ❌ Doesn't support multi-cluster key sharing
- ❌ Not GitOps friendly

**Implementation:**
```yaml
spec:
  storage:
    keys:
      accessModes:
        - ReadWriteOnce
      resources:
        requests:
          storage: 100Mi
```

**Recommendation:** Use Option 1 (User-Supplied) for production, Option 2 (Auto-Generated) for dev/test.

**Key Types:**
- **KSK (Key Signing Key)**: Signs the DNSKEY RRset, published in parent zone as DS record
- **ZSK (Zone Signing Key)**: Signs all other records in the zone

**Key Rotation:**
- **User-Supplied**: User rotates keys externally, updates Secret
- **Auto-Generated**: BIND9 handles rotation based on `dnssec-policy`, operator updates Secret
- **Persistent Storage**: BIND9 handles rotation, keys persist in PVC

**Self-Healing on Key Loss:**
1. Pod restarts, keys missing from emptyDir
2. Operator checks for Secret with keys (`dnssec-keys-<zone>`)
3. **If Secret exists**: Restore keys from Secret to emptyDir
4. **If Secret missing**: BIND9 regenerates keys, operator exports new keys to Secret
5. Operator updates DNSZone status with new DS records
6. User receives notification to update DS records in parent zone (via status condition)

---

## Implementation Phases

### Phase 1: CRD Schema Extensions (Week 1)

**Goal:** Add DNSSEC signing configuration to CRDs

**Tasks:**

1. **Extend `DNSSECConfig` struct** in `src/crd.rs`:
   ```rust
   pub struct DNSSECConfig {
       /// Enable DNSSEC validation (existing)
       #[serde(default)]
       pub validation: Option<bool>,

       /// Enable DNSSEC zone signing (NEW)
       #[serde(default)]
       pub signing: Option<DNSSECSigningConfig>,
   }

   pub struct DNSSECSigningConfig {
       /// Enable DNSSEC signing for zones
       #[serde(default)]
       pub enabled: bool,

       /// DNSSEC policy name (default, custom, or built-in)
       #[serde(default)]
       pub policy: Option<String>,

       /// DNSSEC algorithm (ECDSAP256SHA256, ECDSAP384SHA384, RSASHA256, etc.)
       #[serde(default)]
       pub algorithm: Option<String>,

       /// Key Signing Key (KSK) lifetime (e.g., "365d", "1y")
       #[serde(default)]
       pub ksk_lifetime: Option<String>,

       /// Zone Signing Key (ZSK) lifetime (e.g., "90d", "3m")
       #[serde(default)]
       pub zsk_lifetime: Option<String>,

       /// Use NSEC3 instead of NSEC for authenticated denial of existence
       #[serde(default)]
       pub nsec3: Option<bool>,

       /// NSEC3 salt (hex string, auto-generated if not specified)
       #[serde(default)]
       pub nsec3_salt: Option<String>,

       /// NSEC3 iterations (default: 0 for performance)
       #[serde(default)]
       pub nsec3_iterations: Option<u32>,
   }
   ```

2. **Add per-zone DNSSEC override** to `DNSZoneSpec`:
   ```rust
   pub struct DNSZoneSpec {
       // ... existing fields ...

       /// Override cluster DNSSEC policy for this zone
       #[serde(default)]
       pub dnssec_policy: Option<String>,
   }
   ```

3. **Add DNSSEC status fields** to `DNSZoneStatus`:
   ```rust
   pub struct DNSZoneStatus {
       // ... existing fields ...

       /// DNSSEC signing status
       #[serde(default)]
       pub dnssec: Option<DNSSECStatus>,
   }

   pub struct DNSSECStatus {
       /// Zone is signed with DNSSEC
       pub signed: bool,

       /// DS records for parent zone delegation
       pub ds_records: Vec<String>,

       /// KSK key tag
       pub key_tag: Option<u32>,

       /// Algorithm name
       pub algorithm: Option<String>,

       /// Next scheduled key rollover timestamp
       pub next_key_rollover: Option<String>,

       /// Last key rollover timestamp
       pub last_key_rollover: Option<String>,
   }
   ```

4. **Regenerate CRDs**:
   ```bash
   cargo run --bin crdgen
   cargo run --bin crddoc > docs/src/reference/api.md
   ```

5. **Update examples** in `/examples/`:
   - Add `dnssec-signing-enabled.yaml` example
   - Update `complete-setup.yaml` with DNSSEC signing config

6. **Run validation**:
   ```bash
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test
   kubectl apply --dry-run=client -f deploy/crds/
   ./scripts/validate-examples.sh
   ```

**Deliverables:**
- [ ] Updated CRD schema with DNSSEC signing fields
- [ ] Regenerated CRD YAML files
- [ ] Updated API documentation
- [ ] Example manifests with DNSSEC signing
- [ ] All tests passing

---

### Phase 2: DNSSEC Policy Configuration (Week 2)

**Goal:** Generate `dnssec-policy` declarations in named.conf

**Tasks:**

1. **Create DNSSEC policy template** - Add `templates/dnssec-policy.tmpl`:
   ```bind
   dnssec-policy "{{POLICY_NAME}}" {
       // Key Signing Key (KSK)
       keys {
           ksk lifetime {{KSK_LIFETIME}} algorithm {{ALGORITHM}};
           zsk lifetime {{ZSK_LIFETIME}} algorithm {{ALGORITHM}};
       };

       // Authenticated denial of existence
       {{NSEC_TYPE}};  // "nsec3" or commented out for NSEC

       // DNSSEC signature validity
       signatures-refresh 5d;
       signatures-validity 30d;
       signatures-validity-dnskey 30d;

       // Zone propagation delay (time for zone updates to reach all servers)
       zone-propagation-delay 300;  // 5 minutes

       // Parent propagation delay (time for DS updates in parent zone)
       parent-propagation-delay 3600;  // 1 hour

       // Maximum zone TTL (affects key rollover timing)
       max-zone-ttl 86400;  // 24 hours

       // Parent DS registry (for automated DS updates - future enhancement)
       // parent-ds-sync automatic;
   };
   ```

2. **Extend `build_named_conf_options()`** in `src/bind9_resources.rs`:
   ```rust
   // Add DNSSEC policy generation
   fn generate_dnssec_policies(
       global_config: &Bind9Config,
       instance_config: Option<&Bind9Config>,
   ) -> String {
       let mut policies = String::new();

       // Check for DNSSEC signing configuration
       let signing_config = instance_config
           .and_then(|c| c.dnssec.as_ref())
           .and_then(|d| d.signing.as_ref())
           .or_else(|| {
               global_config
                   .dnssec
                   .as_ref()
                   .and_then(|d| d.signing.as_ref())
           });

       if let Some(signing) = signing_config {
           if signing.enabled {
               let policy_name = signing.policy.as_deref().unwrap_or("default");
               let algorithm = signing.algorithm.as_deref().unwrap_or("ECDSAP256SHA256");
               let ksk_lifetime = signing.ksk_lifetime.as_deref().unwrap_or("365d");
               let zsk_lifetime = signing.zsk_lifetime.as_deref().unwrap_or("90d");
               let nsec_type = if signing.nsec3.unwrap_or(false) {
                   let iterations = signing.nsec3_iterations.unwrap_or(0);
                   format!("nsec3param iterations {iterations} optout no")
               } else {
                   "// Using NSEC (default)".to_string()
               };

               policies = DNSSEC_POLICY_TEMPLATE
                   .replace("{{POLICY_NAME}}", policy_name)
                   .replace("{{ALGORITHM}}", algorithm)
                   .replace("{{KSK_LIFETIME}}", ksk_lifetime)
                   .replace("{{ZSK_LIFETIME}}", zsk_lifetime)
                   .replace("{{NSEC_TYPE}}", &nsec_type);
           }
       }

       policies
   }
   ```

3. **Update `named.conf` template** to include policies:
   ```diff
   // BIND9 Configuration - Auto-generated by bindy

   include "/etc/bind/named.conf.options";
   +
   +// DNSSEC Policies
   +{{DNSSEC_POLICIES}}
   +
   {{ZONES_INCLUDE}}
   ```

4. **Add DNSSEC policy to ConfigMap builder**:
   ```rust
   let dnssec_policies = generate_dnssec_policies(&global_config, instance_config);
   let named_conf = NAMED_CONF_TEMPLATE
       .replace("{{ZONES_INCLUDE}}", &zones_include)
       .replace("{{DNSSEC_POLICIES}}", &dnssec_policies)
       .replace("{{RNDC_KEY_INCLUDES}}", &rndc_includes)
       .replace("{{RNDC_KEY_NAMES}}", &rndc_key_names);
   ```

5. **Add unit tests** in `src/bind9_resources_tests.rs`:
   ```rust
   #[test]
   fn test_dnssec_policy_generation() {
       let config = Bind9Config {
           dnssec: Some(DNSSECConfig {
               validation: Some(true),
               signing: Some(DNSSECSigningConfig {
                   enabled: true,
                   policy: Some("default".to_string()),
                   algorithm: Some("ECDSAP256SHA256".to_string()),
                   ksk_lifetime: Some("365d".to_string()),
                   zsk_lifetime: Some("90d".to_string()),
                   nsec3: Some(true),
                   nsec3_salt: None,
                   nsec3_iterations: Some(0),
               }),
           }),
           // ... other fields ...
       };

       let policies = generate_dnssec_policies(&config, None);
       assert!(policies.contains("dnssec-policy \"default\""));
       assert!(policies.contains("algorithm ECDSAP256SHA256"));
       assert!(policies.contains("ksk lifetime 365d"));
       assert!(policies.contains("zsk lifetime 90d"));
       assert!(policies.contains("nsec3param"));
   }
   ```

**Deliverables:**
- [ ] DNSSEC policy template created
- [ ] Policy generation logic implemented
- [ ] ConfigMap builder updated
- [ ] Unit tests for policy generation
- [ ] All tests passing

---

### Phase 3: Key Source Configuration (Week 3)

**Goal:** Support multiple DNSSEC key sources (user-supplied Secrets, auto-generated, persistent storage)

**Tasks:**

1. **Extend `DNSSECSigningConfig` in `src/crd.rs`**:
   ```rust
   pub struct DNSSECSigningConfig {
       // ... existing fields (enabled, policy, algorithm, etc.) ...

       /// Key source configuration
       #[serde(default)]
       pub keys_from: Option<DNSSECKeySource>,

       /// Auto-generate keys if no keysFrom specified
       #[serde(default)]
       pub auto_generate: Option<bool>,

       /// Export auto-generated keys to Secret for backup/restore
       #[serde(default)]
       pub export_to_secret: Option<bool>,
   }

   /// DNSSEC key source configuration
   pub struct DNSSECKeySource {
       /// Secret containing DNSSEC keys
       #[serde(default)]
       pub secret_ref: Option<SecretReference>,

       /// Persistent volume for keys (legacy/compatibility)
       #[serde(default)]
       pub persistent_volume: Option<PersistentVolumeClaimSpec>,
   }

   pub struct SecretReference {
       /// Secret name containing DNSSEC keys
       pub name: String,

       /// Optional namespace (defaults to same namespace as cluster)
       #[serde(default)]
       pub namespace: Option<String>,
   }
   ```

2. **Extend `build_statefulset()`** in `src/bind9_resources.rs` for flexible key sources:
   ```rust
   // Add DNSSEC keys volume based on key source configuration
   if is_dnssec_signing_enabled(&global_config, instance_config) {
       let signing_config = get_dnssec_signing_config(&global_config, instance_config);

       match &signing_config.keys_from {
           // Option 1: User-supplied keys from Secret
           Some(DNSSECKeySource { secret_ref: Some(secret), .. }) => {
               volumes.push(Volume {
                   name: "dnssec-keys".to_string(),
                   secret: Some(SecretVolumeSource {
                       secret_name: Some(secret.name.clone()),
                       default_mode: Some(0o600),  // Secure permissions
                       ..Default::default()
                   }),
                   ..Default::default()
               });

               volume_mounts.push(VolumeMount {
                   name: "dnssec-keys".to_string(),
                   mount_path: "/var/cache/bind/keys".to_string(),
                   read_only: false,  // BIND9 may update .state files
                   ..Default::default()
               });

               info!("Mounting user-supplied DNSSEC keys from Secret: {}", secret.name);
           }

           // Option 2: Auto-generated keys (emptyDir + Secret backup)
           None if signing_config.auto_generate.unwrap_or(true) => {
               // Use emptyDir for BIND9 to generate keys
               volumes.push(Volume {
                   name: "dnssec-keys".to_string(),
                   empty_dir: Some(EmptyDirVolumeSource::default()),
                   ..Default::default()
               });

               volume_mounts.push(VolumeMount {
                   name: "dnssec-keys".to_string(),
                   mount_path: "/var/cache/bind/keys".to_string(),
                   ..Default::default()
               });

               info!("DNSSEC keys will be auto-generated by BIND9 in emptyDir");

               // Operator will export keys to Secret if exportToSecret is true
               if signing_config.export_to_secret.unwrap_or(true) {
                   info!("Auto-generated keys will be exported to Secret for backup/restore");
               }
           }

           // Option 3: Persistent storage (legacy/compatibility)
           Some(DNSSECKeySource { persistent_volume: Some(pvc), .. }) => {
               volume_claim_templates.push(PersistentVolumeClaim {
                   metadata: ObjectMeta {
                       name: Some("dnssec-keys".to_string()),
                       ..Default::default()
                   },
                   spec: Some(pvc.clone()),
                   status: None,
               });

               volume_mounts.push(VolumeMount {
                   name: "dnssec-keys".to_string(),
                   mount_path: "/var/cache/bind/keys".to_string(),
                   ..Default::default()
               });

               info!("Mounting DNSSEC keys from PersistentVolume");
           }

           // Default: Auto-generate with emptyDir
           _ => {
               volumes.push(Volume {
                   name: "dnssec-keys".to_string(),
                   empty_dir: Some(EmptyDirVolumeSource::default()),
                   ..Default::default()
               });

               volume_mounts.push(VolumeMount {
                   name: "dnssec-keys".to_string(),
                   mount_path: "/var/cache/bind/keys".to_string(),
                   ..Default::default()
               });

               info!("DNSSEC keys will be auto-generated (default behavior)");
           }
       }
   }
   ```

3. **Add key export/restore logic** in Bind9Instance reconciler (`src/reconcilers/bind9instance.rs`):
   ```rust
   /// Export DNSSEC keys from BIND9 pod to Kubernetes Secret for backup/restore.
   async fn export_dnssec_keys_to_secret(
       client: &Client,
       http_client: &Arc<HttpClient>,
       token: &Arc<String>,
       instance_name: &str,
       namespace: &str,
       pod_name: &str,
       zone_name: &str,
   ) -> Result<()> {
       // List key files via bindcar API or exec into pod
       let keys = list_dnssec_keys(http_client, token, pod_name).await?;

       if keys.is_empty() {
           debug!("No DNSSEC keys found to export for {zone_name}");
           return Ok(());
       }

       // Read key files content
       let mut secret_data = BTreeMap::new();
       for key_file in keys {
           let content = read_key_file(http_client, token, pod_name, &key_file).await?;
           secret_data.insert(key_file, content);
       }

       // Create or update Secret
       let secret_name = format!("dnssec-keys-{zone_name}-generated");
       let secret = Secret {
           metadata: ObjectMeta {
               name: Some(secret_name.clone()),
               namespace: Some(namespace.to_string()),
               labels: Some(BTreeMap::from([
                   ("app.kubernetes.io/name".to_string(), "bindy".to_string()),
                   ("app.kubernetes.io/component".to_string(), "dnssec-keys".to_string()),
                   ("bindy.firestoned.io/zone".to_string(), zone_name.to_string()),
                   ("bindy.firestoned.io/instance".to_string(), instance_name.to_string()),
               ])),
               ..Default::default()
           },
           type_: Some("Opaque".to_string()),
           data: Some(secret_data),
           ..Default::default()
       };

       let secrets_api: Api<Secret> = Api::namespaced(client.clone(), namespace);
       secrets_api
           .patch(
               &secret_name,
               &PatchParams::apply("bindy-operator"),
               &Patch::Apply(secret),
           )
           .await
           .context("Failed to export DNSSEC keys to Secret")?;

       info!("Exported DNSSEC keys for {zone_name} to Secret {secret_name}");
       Ok(())
   }

   /// Restore DNSSEC keys from Kubernetes Secret to BIND9 pod on startup.
   async fn restore_dnssec_keys_from_secret(
       client: &Client,
       http_client: &Arc<HttpClient>,
       token: &Arc<String>,
       namespace: &str,
       pod_name: &str,
       zone_name: &str,
   ) -> Result<bool> {
       let secret_name = format!("dnssec-keys-{zone_name}-generated");
       let secrets_api: Api<Secret> = Api::namespaced(client.clone(), namespace);

       // Check if Secret exists
       let secret = match secrets_api.get(&secret_name).await {
           Ok(s) => s,
           Err(e) if e.to_string().contains("NotFound") => {
               debug!("No DNSSEC key Secret found for {zone_name}, keys will be auto-generated");
               return Ok(false);
           }
           Err(e) => return Err(e).context("Failed to get DNSSEC key Secret"),
       };

       // Restore key files to pod
       if let Some(data) = secret.data {
           for (filename, content) in data {
               write_key_file(http_client, token, pod_name, &filename, &content).await?;
           }
           info!("Restored {} DNSSEC keys for {zone_name} from Secret", data.len());
           Ok(true)
       } else {
           Ok(false)
       }
   }
   ```

4. **Update example manifests** with key source configurations:
   ```yaml
   # examples/dnssec-user-supplied-keys.yaml (RECOMMENDED for production)
   apiVersion: bindy.firestoned.io/v1beta1
   kind: Bind9Cluster
   metadata:
     name: dnssec-cluster
     namespace: dns-system
   spec:
     global:
       dnssec:
         validation: true
         signing:
           enabled: true
           policy: "default"
           algorithm: "ECDSAP256SHA256"
           kskLifetime: "365d"
           zskLifetime: "90d"
           nsec3: true
           nsec3Iterations: 0
           keysFrom:
             secretRef:
               name: "dnssec-keys-example-com"  # User manages via ExternalSecrets, Vault, etc.

     primaries:
       replicas: 3
   ---
   # examples/dnssec-auto-generated-keys.yaml (Self-healing for dev/test)
   apiVersion: bindy.firestoned.io/v1beta1
   kind: Bind9Cluster
   metadata:
     name: dnssec-cluster-auto
     namespace: dns-system
   spec:
     global:
       dnssec:
         validation: true
         signing:
           enabled: true
           autoGenerate: true         # BIND9 generates keys
           exportToSecret: true        # Operator exports to Secret for restore
           policy: "default"
           algorithm: "ECDSAP256SHA256"

     primaries:
       replicas: 3
   ---
   # examples/dnssec-external-secrets.yaml (Production with Vault)
   apiVersion: external-secrets.io/v1beta1
   kind: ExternalSecret
   metadata:
     name: dnssec-keys-sync
     namespace: dns-system
   spec:
     refreshInterval: 1h
     secretStoreRef:
       name: vault-backend
       kind: SecretStore
     target:
       name: dnssec-keys-example-com
     data:
       - secretKey: Kexample.com.+013+12345.key
         remoteRef:
           key: dnssec/example.com/zsk/public
       - secretKey: Kexample.com.+013+12345.private
         remoteRef:
           key: dnssec/example.com/zsk/private
       - secretKey: Kexample.com.+013+54321.key
         remoteRef:
           key: dnssec/example.com/ksk/public
       - secretKey: Kexample.com.+013+54321.private
         remoteRef:
           key: dnssec/example.com/ksk/private
   ```

5. **Add status conditions** for key source info:
   ```rust
   // Inform user about DNSSEC key source
   match &signing_config.keys_from {
       Some(DNSSECKeySource { secret_ref: Some(secret), .. }) => {
           conditions.push(Condition {
               type_: "DNSSECKeySource".to_string(),
               status: "True".to_string(),
               reason: "UserSupplied".to_string(),
               message: format!("DNSSEC keys loaded from user-supplied Secret: {}", secret.name),
               last_transition_time: Some(Time(Utc::now())),
               observed_generation: None,
           });
       }
       None if signing_config.auto_generate.unwrap_or(true) => {
           conditions.push(Condition {
               type_: "DNSSECKeySource".to_string(),
               status: "True".to_string(),
               reason: "AutoGenerated".to_string(),
               message: "DNSSEC keys auto-generated by BIND9. Self-healing enabled - keys will be regenerated if lost.".to_string(),
               last_transition_time: Some(Time(Utc::now())),
               observed_generation: None,
           });
       }
       _ => {}
   }
   ```

**Deliverables:**
- [ ] DNSSECKeySource CRD types added
- [ ] StatefulSet builder supports all three key sources
- [ ] Key export/restore logic implemented
- [ ] Example manifests for all key source patterns
- [ ] Status conditions for key source info
- [ ] All tests passing

---

### Phase 4: Zone Configuration for Signing (Week 4)

**Goal:** Apply DNSSEC policy to zones via bindcar API

**Tasks:**

1. **Extend bindcar `ZoneConfig`** - Update dependency to bindcar v0.6.0+ (requires bindcar enhancement):
   ```rust
   // In bindcar library (external dependency)
   pub struct ZoneConfig {
       // ... existing fields ...

       /// DNSSEC policy name to apply to this zone
       pub dnssec_policy: Option<String>,

       /// Enable inline signing (required for DNSSEC)
       pub inline_signing: Option<bool>,
   }
   ```

2. **Update `add_primary_zone()`** in `src/bind9/zone_ops.rs`:
   ```rust
   pub async fn add_primary_zone(
       client: &Arc<HttpClient>,
       token: &Arc<String>,
       zone_name: &str,
       server: &str,
       key_data: &RndcKeyData,
       soa_record: &crate::crd::SOARecord,
       name_server_ips: Option<&HashMap<String, String>>,
       secondary_ips: Option<&[String]>,
       dnssec_policy: Option<&str>,  // NEW parameter
   ) -> Result<bool> {
       // ... existing code ...

       let zone_config = ZoneConfig {
           // ... existing fields ...
           dnssec_policy: dnssec_policy.map(String::from),
           inline_signing: if dnssec_policy.is_some() {
               Some(true)  // Required for DNSSEC
           } else {
               None
           },
       };

       // ... rest of function ...
   }
   ```

3. **Update DNSZone reconciler** in `src/reconcilers/dnszone.rs`:
   ```rust
   // Determine DNSSEC policy for this zone
   let dnssec_policy = dnszone
       .spec
       .dnssec_policy
       .as_ref()
       .or_else(|| {
           // Fall back to cluster global policy if configured
           cluster_config
               .and_then(|c| c.dnssec.as_ref())
               .and_then(|d| d.signing.as_ref())
               .filter(|s| s.enabled)
               .and_then(|s| s.policy.as_ref())
       });

   // Add zone to primary with DNSSEC policy
   add_primary_zone(
       &http_client,
       &token,
       &zone_name,
       &primary_api,
       &key_data,
       &soa_record,
       Some(&name_server_ips),
       Some(&secondary_ips),
       dnssec_policy.map(String::as_str),  // NEW
   )
   .await?;
   ```

4. **Add zone signing verification**:
   ```rust
   // After adding zone, verify it's signed if DNSSEC policy was applied
   if dnssec_policy.is_some() {
       // Query zone for DNSKEY records to confirm signing
       match verify_zone_signed(&http_client, &token, &zone_name, &primary_api).await {
           Ok(true) => {
               info!("Zone {zone_name} is signed with DNSSEC");
           }
           Ok(false) => {
               warn!("Zone {zone_name} has DNSSEC policy but is not yet signed (keys may be generating)");
           }
           Err(e) => {
               warn!("Failed to verify DNSSEC signing for {zone_name}: {e}");
           }
       }
   }
   ```

5. **Add helper function** for DNSSEC verification:
   ```rust
   /// Verify that a zone is signed with DNSSEC by querying for DNSKEY records.
   async fn verify_zone_signed(
       client: &Arc<HttpClient>,
       token: &Arc<String>,
       zone_name: &str,
       server: &str,
   ) -> Result<bool> {
       use hickory_client::client::{Client, SyncClient};
       use hickory_client::rr::{DNSClass, Name, RecordType};
       use hickory_client::udp::UdpClientConnection;
       use std::str::FromStr;

       // Parse server address (e.g., "bind9-primary-api:8080" -> "bind9-primary:5353")
       let dns_server = server
           .split(':')
           .next()
           .unwrap_or(server)
           .replace("-api", "");

       let dns_addr = format!("{dns_server}:{DNS_CONTAINER_PORT}");
       let conn = UdpClientConnection::new(dns_addr.parse()?)?;
       let client = SyncClient::new(conn);

       let name = Name::from_str(zone_name)?;
       let response = client.query(&name, DNSClass::IN, RecordType::DNSKEY)?;

       // If we got DNSKEY records, the zone is signed
       Ok(!response.answers().is_empty())
   }
   ```

**Deliverables:**
- [ ] bindcar dependency updated (or PR submitted to bindcar)
- [ ] Zone creation includes DNSSEC policy
- [ ] Reconciler applies DNSSEC policy to zones
- [ ] DNSSEC verification helper function
- [ ] All tests passing

---

### Phase 5: DS Record Status Reporting (Week 5)

**Goal:** Extract DS records from signed zones and publish in status

**Tasks:**

1. **Add DS record extraction** - Query BIND9 for DS records:
   ```rust
   /// Extract DS records from a signed zone.
   ///
   /// DS records are derived from the KSK DNSKEY record and must be published
   /// in the parent zone to complete the DNSSEC chain of trust.
   async fn extract_ds_records(
       client: &Arc<HttpClient>,
       token: &Arc<String>,
       zone_name: &str,
       server: &str,
   ) -> Result<Vec<String>> {
       use hickory_client::client::{Client, SyncClient};
       use hickory_client::rr::{DNSClass, Name, RecordType};
       use hickory_client::udp::UdpClientConnection;
       use std::str::FromStr;

       let dns_server = server
           .split(':')
           .next()
           .unwrap_or(server)
           .replace("-api", "");

       let dns_addr = format!("{dns_server}:{DNS_CONTAINER_PORT}");
       let conn = UdpClientConnection::new(dns_addr.parse()?)?;
       let dns_client = SyncClient::new(conn);

       let name = Name::from_str(zone_name)?;

       // Query for DNSKEY records
       let dnskey_response = dns_client.query(&name, DNSClass::IN, RecordType::DNSKEY)?;

       // Filter for KSK (Key Signing Key) - flags=257 (bit 0 set = SEP flag)
       let ksk_records: Vec<_> = dnskey_response
           .answers()
           .iter()
           .filter_map(|record| {
               if let Some(dnskey) = record.data()?.as_dnssec()?.as_dnskey() {
                   if dnskey.zone_key() && dnskey.secure_entry_point() {
                       // This is a KSK
                       Some(dnskey.clone())
                   } else {
                       None
                   }
               } else {
                   None
               }
           })
           .collect();

       if ksk_records.is_empty() {
           anyhow::bail!("No KSK DNSKEY records found for zone {zone_name}");
       }

       // Generate DS records from KSK using SHA-256 digest
       let mut ds_records = Vec::new();
       for ksk in ksk_records {
           let ds = ksk.to_ds(
               &name,
               hickory_proto::rr::dnssec::DigestType::SHA256,
           )?;

           // Format as DNS record: "example.com. IN DS 12345 13 2 ABC123..."
           ds_records.push(format!(
               "{zone_name}. IN DS {} {} {} {}",
               ds.key_tag(),
               ds.algorithm().to_u8(),
               ds.digest_type().to_u8(),
               hex::encode(ds.digest())
           ));
       }

       Ok(ds_records)
   }
   ```

2. **Update DNSZone status** after signing:
   ```rust
   // In DNSZone reconciler after zone creation/update
   if let Some(policy) = dnssec_policy {
       match extract_ds_records(&http_client, &token, &zone_name, &primary_api).await {
           Ok(ds_records) => {
               // Update status with DNSSEC info
               let dnssec_status = DNSSECStatus {
                   signed: true,
                   ds_records: ds_records.clone(),
                   key_tag: Some(parse_key_tag_from_ds(&ds_records[0])?),
                   algorithm: Some(policy.to_string()),
                   next_key_rollover: None,  // TODO: Calculate from policy
                   last_key_rollover: None,
               };

               // Patch DNSZone status
               let status = DNSZoneStatus {
                   dnssec: Some(dnssec_status),
                   // ... other status fields ...
               };

               patch_dnszone_status(client, &dnszone_name, &namespace, status).await?;

               info!(
                   "Published DS records for {zone_name}: {}",
                   ds_records.join(", ")
               );
           }
           Err(e) => {
               warn!("Failed to extract DS records for {zone_name}: {e}");
           }
       }
   }
   ```

3. **Add status patch helper**:
   ```rust
   /// Patch DNSZone status with DNSSEC information.
   async fn patch_dnszone_status(
       client: &Client,
       name: &str,
       namespace: &str,
       status: DNSZoneStatus,
   ) -> Result<()> {
       let api: Api<DNSZone> = Api::namespaced(client.clone(), namespace);

       let patch = serde_json::json!({
           "status": status
       });

       api.patch_status(
           name,
           &PatchParams::apply("bindy-operator"),
           &Patch::Merge(patch),
       )
       .await
       .context("Failed to patch DNSZone status")?;

       Ok(())
   }
   ```

4. **Add DS record display to kubectl output**:
   ```rust
   // In CRD definition - add print column for DS records
   #[kube(
       kind = "DNSZone",
       // ... other attributes ...
       printcolumn = r#"{"name":"DNSSEC","type":"string","jsonPath":".status.dnssec.signed"}"#,
       printcolumn = r#"{"name":"DS Records","type":"string","jsonPath":".status.dnssec.dsRecords[0]","priority":1}"#,
   )]
   ```

5. **Update documentation** - Add DS record usage guide:
   ```markdown
   # Using DS Records for Parent Zone Delegation

   When DNSSEC signing is enabled, bindy generates DS (Delegation Signer) records
   that must be published in the parent zone to complete the DNSSEC chain of trust.

   ## Retrieving DS Records

   View DS records in the DNSZone status:

   ```bash
   kubectl get dnszone example-com -n dns-system -o jsonpath='{.status.dnssec.dsRecords[*]}'
   ```

   Example output:
   ```
   example.com. IN DS 12345 13 2 ABC123...
   ```

   ## Publishing DS Records

   ### For Public Domains
   1. Log into your domain registrar's DNS management console
   2. Navigate to DNSSEC settings
   3. Add the DS record with the key tag, algorithm, digest type, and digest

   ### For Internal Domains
   Add the DS record to the parent zone (e.g., for `app.example.com`, add to `example.com` zone).
   ```

**Deliverables:**
- [ ] DS record extraction implemented
- [ ] Status patching with DNSSEC info
- [ ] kubectl print column for DS records
- [ ] Documentation for DS record usage
- [ ] All tests passing

---

### Phase 6: Integration Testing & Validation (Week 6)

**Goal:** Comprehensive testing of DNSSEC signing functionality

**Tasks:**

1. **Create integration test** - `tests/dnssec_integration.rs`:
   ```rust
   #[tokio::test]
   async fn test_dnssec_zone_signing_e2e() {
       // 1. Create Bind9Cluster with DNSSEC signing enabled
       let cluster = Bind9Cluster {
           metadata: ObjectMeta {
               name: Some("dnssec-test".to_string()),
               namespace: Some("test".to_string()),
               ..Default::default()
           },
           spec: Bind9ClusterSpec {
               global: Bind9Config {
                   dnssec: Some(DNSSECConfig {
                       validation: Some(true),
                       signing: Some(DNSSECSigningConfig {
                           enabled: true,
                           policy: Some("default".to_string()),
                           algorithm: Some("ECDSAP256SHA256".to_string()),
                           ksk_lifetime: Some("365d".to_string()),
                           zsk_lifetime: Some("90d".to_string()),
                           nsec3: Some(true),
                           nsec3_iterations: Some(0),
                           nsec3_salt: None,
                       }),
                   }),
                   // ... other config ...
               },
               // ... other spec fields ...
           },
           status: None,
       };

       apply_resource(&cluster).await?;

       // 2. Create DNSZone
       let zone = DNSZone {
           metadata: ObjectMeta {
               name: Some("test-zone".to_string()),
               namespace: Some("test".to_string()),
               ..Default::default()
           },
           spec: DNSZoneSpec {
               zone_name: "test.example.com".to_string(),
               cluster_ref: "dnssec-test".to_string(),
               // ... other fields ...
           },
           status: None,
       };

       apply_resource(&zone).await?;

       // 3. Wait for zone to be signed
       wait_for_condition(&zone, "Ready", "True", Duration::from_secs(120)).await?;

       // 4. Verify DNSSEC records exist
       let updated_zone = get_resource::<DNSZone>("test", "test-zone").await?;
       let dnssec_status = updated_zone
           .status
           .and_then(|s| s.dnssec)
           .expect("DNSSEC status should be present");

       assert!(dnssec_status.signed, "Zone should be signed");
       assert!(!dnssec_status.ds_records.is_empty(), "DS records should be present");
       assert!(dnssec_status.key_tag.is_some(), "Key tag should be present");

       // 5. Query DNSKEY records via DNS
       let dnskeys = query_dns_records("test.example.com", RecordType::DNSKEY).await?;
       assert!(!dnskeys.is_empty(), "DNSKEY records should be present");

       // 6. Verify RRSIG records exist (signed records)
       let rrsigs = query_dns_records("test.example.com", RecordType::RRSIG).await?;
       assert!(!rrsigs.is_empty(), "RRSIG records should be present");

       // 7. Validate DNSSEC chain with delv (DNSSEC lookup and validation)
       let delv_output = Command::new("delv")
           .args(&["@127.0.0.1", "test.example.com", "SOA"])
           .output()
           .await?;

       let output_str = String::from_utf8_lossy(&delv_output.stdout);
       assert!(
           output_str.contains("fully validated"),
           "DNSSEC validation should succeed"
       );

       // Cleanup
       delete_resource::<DNSZone>("test", "test-zone").await?;
       delete_resource::<Bind9Cluster>("test", "dnssec-test").await?;
   }
   ```

2. **Add DNSSEC validation test**:
   ```rust
   #[tokio::test]
   async fn test_dnssec_validation_with_dig() {
       // Use dig +dnssec to verify DNSSEC signatures
       let dig_output = Command::new("dig")
           .args(&[
               "@127.0.0.1",
               "test.example.com",
               "SOA",
               "+dnssec",
               "+multi",
           ])
           .output()
           .await?;

       let output_str = String::from_utf8_lossy(&dig_output.stdout);

       // Should see DNSKEY and RRSIG records
       assert!(output_str.contains("DNSKEY"), "DNSKEY record should be present");
       assert!(output_str.contains("RRSIG"), "RRSIG record should be present");
       assert!(output_str.contains("ad;"), "AD (Authentic Data) flag should be set");
   }
   ```

3. **Add key rotation test**:
   ```rust
   #[tokio::test]
   async fn test_dnssec_key_rotation() {
       // 1. Create zone with short key lifetime (for testing)
       let cluster = create_cluster_with_short_key_lifetime().await?;
       let zone = create_zone(&cluster).await?;

       // 2. Extract initial key tag
       let initial_status = get_zone_status(&zone).await?;
       let initial_key_tag = initial_status.dnssec.unwrap().key_tag.unwrap();

       // 3. Wait for key rollover (simulate time passing or manually trigger)
       // NOTE: This may require mocking or extending test duration
       tokio::time::sleep(Duration::from_secs(180)).await;

       // 4. Verify new key tag
       let updated_status = get_zone_status(&zone).await?;
       let new_key_tag = updated_status.dnssec.unwrap().key_tag.unwrap();

       assert_ne!(
           initial_key_tag, new_key_tag,
           "Key tag should change after rotation"
       );
   }
   ```

4. **Manual validation checklist**:
   ```markdown
   # DNSSEC Manual Testing Checklist

   ## Prerequisites
   - [ ] Kind cluster running
   - [ ] bindy deployed with DNSSEC signing enabled
   - [ ] DNS tools installed: dig, delv, drill

   ## Test Cases

   ### 1. Zone Signing
   ```bash
   # Create cluster with DNSSEC
   kubectl apply -f examples/dnssec-signing-enabled.yaml

   # Verify DNSKEY records
   dig @<pod-ip> example.com DNSKEY +short

   # Should see two keys: KSK (257) and ZSK (256)
   # Example output:
   # 256 3 13 <zsk-key-data>
   # 257 3 13 <ksk-key-data>
   ```

   ### 2. Record Signatures
   ```bash
   # Verify A record has RRSIG
   dig @<pod-ip> www.example.com A +dnssec

   # Should see both A record and RRSIG in ANSWER section
   ```

   ### 3. NSEC/NSEC3 Records
   ```bash
   # Query for non-existent record
   dig @<pod-ip> nonexistent.example.com A +dnssec

   # Should see NSEC3 record proving non-existence
   ```

   ### 4. DS Record Accuracy
   ```bash
   # Get DS from status
   kubectl get dnszone example-com -o jsonpath='{.status.dnssec.dsRecords[0]}'

   # Manually generate DS from DNSKEY
   dig @<pod-ip> example.com DNSKEY | dnssec-dsfromkey -2 -f - example.com

   # Compare - should match
   ```

   ### 5. Validation with delv
   ```bash
   # Validate DNSSEC chain
   delv @<pod-ip> example.com SOA +root=/etc/bind/bind.keys

   # Should output: "fully validated"
   ```

   ### 6. Key Persistence
   ```bash
   # Check keys exist in PVC
   kubectl exec -it bind9-primary-0 -- ls -la /var/cache/bind/keys/

   # Restart pod
   kubectl delete pod bind9-primary-0

   # Wait for pod restart, verify keys still exist
   kubectl exec -it bind9-primary-0 -- ls -la /var/cache/bind/keys/

   # Keys should persist
   ```

   ### 7. Zone Transfer with DNSSEC
   ```bash
   # Query primary
   dig @<primary-ip> example.com DNSKEY +short

   # Query secondary
   dig @<secondary-ip> example.com DNSKEY +short

   # Keys should match (transferred)
   ```
   ```

5. **Add CI/CD workflow** - `.github/workflows/dnssec-tests.yml`:
   ```yaml
   name: DNSSEC Integration Tests

   on:
     pull_request:
       paths:
         - 'src/**'
         - 'tests/**'
         - 'examples/*dnssec*'
     push:
       branches: [main]

   jobs:
     dnssec-test:
       runs-on: ubuntu-latest
       steps:
         - uses: actions/checkout@v4

         - name: Install DNS tools
           run: |
             sudo apt-get update
             sudo apt-get install -y dnsutils ldnsutils bind9-dnsutils

         - name: Setup Kind
           uses: helm/kind-action@v1

         - name: Build and deploy bindy
           run: |
             make docker-build
             kind load docker-image bindy:latest
             kubectl apply -f deploy/crds/
             kubectl apply -f deploy/

         - name: Run DNSSEC integration tests
           run: cargo test --test dnssec_integration -- --nocapture

         - name: Collect logs on failure
           if: failure()
           run: |
             kubectl logs -n dns-system -l app=bind9 --tail=100
             kubectl get all -n dns-system
   ```

**Deliverables:**
- [ ] Integration tests for DNSSEC signing
- [ ] DNSSEC validation tests
- [ ] Key rotation tests
- [ ] Manual testing checklist
- [ ] CI/CD workflow for DNSSEC tests
- [ ] All tests passing

---

### Phase 7: Documentation & Examples (Week 7)

**Goal:** Complete user-facing documentation for DNSSEC feature

**Tasks:**

1. **Create comprehensive guide** - `docs/src/guides/dnssec-setup.md`:
   ```markdown
   # DNSSEC Setup and Configuration

   This guide covers setting up DNSSEC zone signing in bindy.

   ## Prerequisites
   - Persistent storage for DNSSEC keys (recommended)
   - BIND9 version 9.16+ (for modern dnssec-policy support)
   - Access to parent zone for DS record publishing (for public domains)

   ## Quick Start
   [... detailed setup instructions ...]

   ## Configuration Options
   [... all DNSSEC config parameters explained ...]

   ## DS Record Management
   [... how to publish DS records in parent zones ...]

   ## Key Rotation
   [... explanation of automatic key rotation ...]

   ## Troubleshooting
   [... common issues and solutions ...]
   ```

2. **Add architecture documentation** - `docs/src/architecture/dnssec.md`:
   ```markdown
   # DNSSEC Architecture

   ## Overview
   [... architecture diagrams ...]

   ## Key Management
   [... key generation and storage ...]

   ## Signing Process
   [... how zones are signed ...]

   ## Zone Transfer with DNSSEC
   [... how DNSSEC works with primaries/secondaries ...]
   ```

3. **Update feature matrix** in `README.md`:
   ```markdown
   ## Features

   - [x] **DNSSEC Validation** - Validate signatures from upstream servers
   - [x] **DNSSEC Zone Signing** - Automatic signing of authoritative zones
   - [x] **Automatic Key Management** - KSK/ZSK generation and rotation
   - [x] **DS Record Publishing** - Export DS records for parent delegation
   - [x] **NSEC3 Support** - Authenticated denial of existence with privacy
   ```

4. **Create example manifests**:
   - `examples/dnssec-signing-enabled.yaml` - Basic DNSSEC setup
   - `examples/dnssec-with-nsec3.yaml` - DNSSEC with NSEC3
   - `examples/dnssec-custom-policy.yaml` - Custom key lifetimes
   - `examples/dnssec-per-zone-override.yaml` - Zone-specific policies

5. **Add API reference documentation**:
   ```bash
   # Regenerate API docs with new DNSSEC fields
   cargo run --bin crddoc > docs/src/reference/api.md
   ```

6. **Update `CHANGELOG.md`**:
   ```markdown
   ## [2026-02-15] - DNSSEC Zone Signing

   **Author:** Erick Bourgeois

   ### Added
   - DNSSEC zone signing with automatic key management
   - Support for ECDSAP256SHA256, ECDSAP384SHA384, and RSASHA256 algorithms
   - Automatic DS record generation and status publishing
   - NSEC3 support for authenticated denial of existence
   - Per-zone DNSSEC policy overrides
   - Persistent storage configuration for DNSSEC keys
   - Comprehensive DNSSEC documentation and examples

   ### Changed
   - Extended DNSSECConfig CRD with signing configuration
   - Updated Bind9InstanceSpec and Bind9ClusterSpec with DNSSEC signing options
   - Enhanced DNSZoneStatus with DNSSEC information (signed, dsRecords, keyTag)

   ### Why
   Enable cryptographic authentication of DNS records to protect against
   cache poisoning and man-in-the-middle attacks. Meets regulatory
   requirements for DNS security in banking and critical infrastructure.

   ### Impact
   - [ ] Non-breaking change (DNSSEC signing is opt-in)
   - [ ] Requires persistent storage for production use
   - [ ] DS records must be published in parent zone to complete chain of trust
   - [ ] Documentation updated
   ```

**Deliverables:**
- [ ] DNSSEC setup guide
- [ ] Architecture documentation
- [ ] Updated README feature matrix
- [ ] Example manifests for all DNSSEC scenarios
- [ ] Regenerated API documentation
- [ ] Updated CHANGELOG.md
- [ ] Documentation builds successfully: `make docs`

---

## Dependencies & Prerequisites

### External Dependencies

1. **BIND9 Version**: 9.16 or later (for `dnssec-policy` support)
   - Verify image: `internetsystemsconsortium/bind9:9.18` or later

2. **bindcar Version**: v0.6.0+ (requires enhancement for `dnssec_policy` field)
   - **Action Required**: Submit PR to bindcar to add DNSSEC fields to `ZoneConfig`
   - Alternative: Fork bindcar and add fields locally

3. **Storage Class**: Persistent storage for DNSSEC keys
   - Recommended: PersistentVolume with ReadWriteOnce access
   - Minimum size: 100Mi (keys are small)

4. **DNS Tools** (for testing):
   - dig (BIND9 utils)
   - delv (DNSSEC validation tool)
   - drill (ldns utils)

### Internal Dependencies

1. **CRD Version**: Must bump to v1beta2 for schema changes
   - Migration guide required for existing users

2. **Operator Compatibility**: DNSSEC logic only applied if feature enabled
   - Backwards compatible: existing clusters without DNSSEC continue working

3. **Status Subresource**: DNSZone status must support DNSSEC fields
   - Already available in current CRD

---

## Risks & Mitigations

### Risk 1: Key Loss (MITIGATED - Self-Healing Architecture)

**Description:** DNSSEC keys could be lost due to pod restarts, storage failures, or accidental deletion.

**Impact:** Medium (reduced from High) - Temporary DNSSEC validation failures until keys are restored/regenerated

**Mitigation:**
- **✅ Self-Healing Architecture (Primary)**: Auto-generated keys are exported to Secrets and automatically restored on pod restart
- **✅ User-Controlled Keys (Production)**: Users manage keys externally via ExternalSecrets, Vault, sealed-secrets, GitOps
- **✅ Multi-Source Support**: Three key source options (user-supplied, auto-generated with backup, persistent storage)
- **✅ Automatic Recovery**: If Secret is deleted, BIND9 auto-generates new keys, operator exports to new Secret
- **📝 Status Notifications**: DNSZone status updated with new DS records after key regeneration
- **📚 Documentation**: Clear guidance on DS record update procedures for parent zones

**Production Recommendation:** Use user-supplied keys via ExternalSecrets for maximum control and compliance

### Risk 2: DS Record Propagation Delay

**Description:** Parent zone may take time to publish DS records, creating window where DNSSEC validation fails.

**Impact:** Medium - Temporary validation failures until DS published

**Mitigation:**
- Document expected propagation delays (registrar-dependent)
- Add status field for DS publication timestamp
- Provide troubleshooting guide for validation failures
- Support pre-publishing DS records before enabling signing

### Risk 3: Key Rollover Complexity

**Description:** Automatic key rollover requires careful timing to avoid validation failures.

**Impact:** Medium - Misconfigured rollover can break DNSSEC validation

**Mitigation:**
- Use BIND9's proven `dnssec-policy` automation
- Set conservative default lifetimes (365d KSK, 90d ZSK)
- Monitor key rollover events in operator logs
- Add status fields for next/last rollover timestamps

### Risk 4: Performance Impact

**Description:** DNSSEC signing adds CPU overhead for cryptographic operations.

**Impact:** Low-Medium - Depends on zone size and update frequency

**Mitigation:**
- Use ECDSA algorithms (faster than RSA)
- Set NSEC3 iterations to 0 (recommended by RFC 9276)
- Document resource requirements for DNSSEC-enabled clusters
- Provide performance tuning guide

### Risk 5: Bindcar Dependency

**Description:** Requires bindcar enhancement to support DNSSEC configuration.

**Impact:** Medium - Blocks implementation if bindcar doesn't accept changes

**Mitigation:**
- Submit PR to bindcar early in Phase 1
- Alternative: Fork bindcar and maintain DNSSEC patches
- Future: Move zone management logic into bindy operator (eliminate bindcar dependency)

---

## Success Criteria

### Functional Requirements

- [ ] Zones can be signed with DNSSEC using declarative configuration
- [ ] Multiple DNSSEC algorithms supported (ECDSAP256SHA256, ECDSAP384SHA384, RSASHA256)
- [ ] Automatic key generation and rotation
- [ ] DS records extracted and published in DNSZone status
- [ ] Per-zone DNSSEC policy overrides
- [ ] NSEC3 support for authenticated denial of existence
- [ ] Keys persist across pod restarts (with persistent storage)

### Non-Functional Requirements

- [ ] DNSSEC signing adds <10% CPU overhead vs. unsigned zones
- [ ] Key rollover completes without validation failures
- [ ] Integration tests verify end-to-end DNSSEC functionality
- [ ] Documentation covers all DNSSEC scenarios
- [ ] Backwards compatible with existing non-DNSSEC clusters

### Quality Gates

- [ ] All unit tests passing
- [ ] All integration tests passing
- [ ] DNSSEC validation succeeds with `delv` and `dig +dnssec`
- [ ] No clippy warnings
- [ ] Documentation builds successfully: `make docs`
- [ ] Examples validate: `./scripts/validate-examples.sh`

---

## Testing Strategy

### Unit Tests
- DNSSEC policy generation
- ConfigMap building with DNSSEC options
- Zone configuration with signing enabled
- DS record parsing and formatting
- Storage configuration validation

### Integration Tests
- End-to-end zone signing
- Key persistence across pod restarts
- DS record extraction
- Zone transfer with DNSSEC
- NSEC3 record generation

### Manual Tests
- DNSSEC validation with delv
- Parent DS record publishing (test domain)
- Key rollover observation
- Performance benchmarking with DNSSEC

### Security Tests
- Key file permissions (should be 0600, owned by bind)
- Key directory permissions (should be 0700)
- RNDC authentication still works with DNSSEC
- No key material in logs or status

---

## Open Questions

1. **Q: Should we support custom DNSSEC policies or only built-in?**
   - **A:** Start with built-in "default" policy, add custom policy CRD in future enhancement

2. **Q: Should we automatically publish DS records to parent zones?**
   - **A:** No for initial implementation. Requires parent zone credentials and varies by provider. Document manual process.

3. **Q: How to handle key backup and recovery?**
   - **A:** Out of scope for Phase 1. Document manual backup procedures. Future enhancement: Automatic backup to S3/external storage.

4. **Q: Should we support algorithm agility (multiple algorithms per zone)?**
   - **A:** No for initial implementation. Use single algorithm per zone. BIND9 `dnssec-policy` supports this natively for future enhancement.

5. **Q: How to handle DNSSEC for secondary-only zones?**
   - **A:** Secondaries transfer signed zones from primaries. No special handling needed - DNSSEC config applies to primaries only.

6. **Q: Should we validate DS records before publishing to status?**
   - **A:** Yes - compute DS from DNSKEY and verify it matches before publishing to status. Prevents incorrect DS publication.

---

## Future Enhancements

### Phase 8: Advanced Features (Future)
- **Automated DS Publishing**: Integrate with domain registrars (Route53, Cloudflare, etc.) for automatic DS updates
- **Custom DNSSEC Policies**: Allow users to define custom policies via CRD
- **Key Backup Automation**: Automatic backup of DNSSEC keys to S3/external storage
- **Key Recovery**: Restore keys from backup on pod restart
- **DNSSEC Monitoring**: Prometheus metrics for key expiration, rollover events, signing failures
- **Multi-Algorithm Support**: Sign zones with multiple algorithms simultaneously
- **NSEC3 Parameter Tuning**: Per-zone NSEC3 salt and iteration configuration
- **Inline Signing Verification**: Automated testing of DNSSEC signatures before publishing
- **Parent Zone Integration**: Automatic DS record publishing to parent zones (for internal domains)

### Phase 9: Compliance & Auditing (Future)
- **Audit Logging**: Log all DNSSEC key operations (generation, rollover, deletion)
- **Compliance Reports**: Generate NIST 800-53 compliance reports for DNSSEC
- **Key Ceremony Support**: Document key generation ceremony for high-security environments
- **HSM Integration**: Support for hardware security modules for key storage (PKCS#11)

---

## References

### RFCs
- [RFC 4033](https://www.rfc-editor.org/rfc/rfc4033.html) - DNS Security Introduction and Requirements
- [RFC 4034](https://www.rfc-editor.org/rfc/rfc4034.html) - Resource Records for the DNS Security Extensions
- [RFC 4035](https://www.rfc-editor.org/rfc/rfc4035.html) - Protocol Modifications for the DNS Security Extensions
- [RFC 5155](https://www.rfc-editor.org/rfc/rfc5155.html) - DNS Security (DNSSEC) Hashed Authenticated Denial of Existence
- [RFC 6781](https://www.rfc-editor.org/rfc/rfc6781.html) - DNSSEC Operational Practices, Version 2
- [RFC 8624](https://www.rfc-editor.org/rfc/rfc8624.html) - Algorithm Implementation Requirements and Usage Guidance for DNSSEC
- [RFC 9276](https://www.rfc-editor.org/rfc/rfc9276.html) - Guidance for NSEC3 Parameter Settings

### BIND9 Documentation
- [BIND9 DNSSEC Guide](https://bind9.readthedocs.io/en/latest/dnssec-guide.html)
- [dnssec-policy Statement](https://bind9.readthedocs.io/en/latest/reference.html#namedconf-statement-dnssec-policy)
- [BIND9 ARM Chapter 4: DNSSEC](https://bind9.readthedocs.io/en/latest/chapter4.html)

### External Tools
- [DNSViz](https://dnsviz.net/) - DNSSEC visualization tool
- [Zonemaster](https://zonemaster.net/) - DNS zone quality checker
- [DNSSEC Analyzer](https://dnssec-analyzer.verisignlabs.com/) - Verisign DNSSEC debugger

---

## Timeline Summary

| Phase | Duration | Deliverables |
|-------|----------|-------------|
| Phase 1: CRD Schema | Week 1 | Updated CRDs, examples, tests |
| Phase 2: Policy Config | Week 2 | DNSSEC policy generation in named.conf |
| Phase 3: Storage | Week 3 | Persistent volumes for keys |
| Phase 4: Zone Config | Week 4 | Zone signing via bindcar API |
| Phase 5: DS Records | Week 5 | DS record extraction and status |
| Phase 6: Testing | Week 6 | Integration tests, validation |
| Phase 7: Documentation | Week 7 | Guides, examples, API docs |

**Total Estimated Duration:** 7 weeks (35 business days)

---

## Stakeholder Sign-Off

- [ ] **Product Owner**: Feature scope approved
- [ ] **Platform Engineering**: Architecture approved
- [ ] **Security Team**: Key management approach approved
- [ ] **Compliance**: Regulatory requirements met
- [ ] **Documentation Team**: Documentation plan approved

---

## Appendix: Example Configurations

### Example 1: Basic DNSSEC Signing

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dnssec-cluster
  namespace: dns-system
spec:
  global:
    dnssec:
      validation: true
      signing:
        enabled: true
        policy: "default"
        algorithm: "ECDSAP256SHA256"
        kskLifetime: "365d"
        zskLifetime: "90d"

  storage:
    keys:
      accessModes:
        - ReadWriteOnce
      resources:
        requests:
          storage: 100Mi

  primaries:
    replicas: 3
```

### Example 2: DNSSEC with NSEC3

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: dnssec-nsec3-cluster
  namespace: dns-system
spec:
  global:
    dnssec:
      validation: true
      signing:
        enabled: true
        policy: "default"
        algorithm: "ECDSAP256SHA256"
        kskLifetime: "365d"
        zskLifetime: "90d"
        nsec3: true
        nsec3Iterations: 0  # RFC 9276 recommendation

  storage:
    keys:
      accessModes:
        - ReadWriteOnce
      resources:
        requests:
          storage: 100Mi

  primaries:
    replicas: 3
```

### Example 3: Per-Zone DNSSEC Override

```yaml
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: special-zone
  namespace: dns-system
spec:
  zoneName: "special.example.com"
  clusterRef: "dnssec-cluster"
  dnssecPolicy: "custom-high-security"  # Override cluster default
  soa:
    primaryNs: "ns1.special.example.com."
    adminEmail: "admin.special.example.com."
    serial: 2026010201
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
```

---

**End of Roadmap**
