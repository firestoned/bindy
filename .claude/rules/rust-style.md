# Rust Style Guide

## Core Principles

- Use `thiserror` for error types, not string errors
- Prefer `anyhow::Result` in binaries, typed errors in libraries
- Use `tracing` for logging, not `println!` or `log`
- Async functions should use `tokio`
- All k8s API calls must have timeout and retry logic
- **No magic numbers**: Any numeric literal other than `0` or `1` MUST be declared as a named constant
- **Use early returns/guard clauses**: Minimize nesting by handling edge cases early and returning

---

## Early Return / Guard Clause Pattern

**CRITICAL: Prefer early returns over nested if-else statements.**

The "early return" or "guard clause" coding style emphasizes minimizing nested if-else statements and promoting clearer, more linear code flow. This is achieved by handling error conditions or special cases at the beginning of a function and exiting early if those conditions are met.

### Key Principles

1. **Handle preconditions first**: Validate input parameters and other preconditions at the start of a function. If a condition is not met, return immediately (e.g., `return Err(...)`, `return None`, or `return Ok(())`).

   ```rust
   // ✅ GOOD - Early return for validation
   pub async fn reconcile_dnszone(
       client: Client,
       dnszone: DNSZone,
       zone_manager: &Bind9Manager,
   ) -> Result<()> {
       // Guard clause: Check if work is needed
       if !needs_reconciliation {
           debug!("Spec unchanged, skipping reconciliation");
           return Ok(());  // Early return - no work needed
       }

       // Main logic continues here (happy path)
       let secondary_ips = find_all_secondary_pod_ips(&client, &namespace, &cluster_ref).await?;
       add_dnszone(client, dnszone, zone_manager).await?;
       Ok(())
   }

   // ❌ BAD - Nested if-else
   pub async fn reconcile_dnszone(
       client: Client,
       dnszone: DNSZone,
       zone_manager: &Bind9Manager,
   ) -> Result<()> {
       if needs_reconciliation {
           let secondary_ips = find_all_secondary_pod_ips(&client, &namespace, &cluster_ref).await?;
           add_dnszone(client, dnszone, zone_manager).await?;
           Ok(())
       } else {
           debug!("Spec unchanged, skipping reconciliation");
           Ok(())
       }
   }
   ```

2. **Minimize else statements**: Instead of using if-else for mutually exclusive conditions, use early returns within if blocks.

   ```rust
   // ✅ GOOD - No else needed
   fn calculate_discount(price: f64, is_premium_member: bool) -> f64 {
       if is_premium_member {
           return price * 0.90;  // Apply 10% discount and return
       }
       // No 'else' needed; non-premium members are handled here
       price * 0.95  // Apply 5% discount
   }
   ```

3. **Use `?` for error propagation**: Rust's `?` operator is a form of early return for errors. Use it liberally to keep the happy path unindented.

   ```rust
   // ✅ GOOD - Early error returns with ?
   pub async fn add_dnszone(client: Client, dnszone: DNSZone) -> Result<()> {
       let namespace = dnszone.namespace().ok_or_else(|| anyhow!("No namespace"))?;
       let instances = find_instances(&client, &namespace).await?;

       if instances.is_empty() {
           return Err(anyhow!("No instances found"));
       }

       for instance in instances {
           add_zone_to_instance(&instance).await?;
       }

       Ok(())
   }
   ```

### Benefits

- **Reduced nesting**: Improves readability and reduces cognitive load
- **Clearer code flow**: The main logic is less cluttered by error handling
- **Easier to test**: Each condition can be tested in isolation
- **Fail-fast approach**: Catches invalid states or inputs early in the execution
- **More maintainable**: Changes to edge cases don't affect the main logic

### When to Use

- Input validation at function start
- Checking preconditions before expensive operations
- Handling special cases before the general case
- Error handling in async functions
- State validation in reconciliation loops

---

## Magic Numbers Rule

**CRITICAL: All numeric literals (except 0 and 1) MUST be named constants.**

A "magic number" is any numeric literal (other than `0` or `1`) that appears directly in code without explanation.

### Why

- **Readability**: Named constants make code self-documenting
- **Maintainability**: Change the value in one place, not scattered throughout
- **Semantic Meaning**: The constant name explains *why* the value matters
- **Type Safety**: Constants prevent accidental typos in numeric values

### Rules

- **`0` and `1` are allowed** - These are ubiquitous and self-explanatory (empty, none, first item, etc.)
- **All other numbers MUST be named constants** - No exceptions
- Use descriptive names that explain the *purpose*, not just the value

### Examples

```rust
// ✅ GOOD - Named constants
const DEFAULT_ZONE_TTL: u32 = 3600;
const MAX_RETRY_ATTEMPTS: u8 = 3;
const RECONCILE_INTERVAL_SECS: u64 = 300;
const DNS_PORT: u16 = 53;

fn build_zone(ttl: Option<u32>) -> Zone {
    Zone {
        ttl: ttl.unwrap_or(DEFAULT_ZONE_TTL),
        ..
    }
}

fn reconcile() -> Action {
    Action::requeue(Duration::from_secs(RECONCILE_INTERVAL_SECS))
}

// ❌ BAD - Magic numbers
fn build_zone(ttl: Option<u32>) -> Zone {
    Zone {
        ttl: ttl.unwrap_or(3600),  // What does 3600 mean? Why this value?
        ..
    }
}

fn reconcile() -> Action {
    Action::requeue(Duration::from_secs(300))  // Why 300?
}
```

### Special Cases

**Unit conversions**: Still need constants
```rust
// ✅ GOOD
const MILLISECONDS_PER_SECOND: u64 = 1000;
const SECONDS_PER_HOUR: u64 = 3600;

// ❌ BAD
Duration::from_millis(timeout_secs * 1000)
```

**Array sizes/indexing**: Use constants if size is meaningful
```rust
// ✅ GOOD
const MAX_DNS_LABELS: usize = 127;
let labels = vec![String::new(); MAX_DNS_LABELS];

// ✅ ACCEPTABLE - indexing with 0 or 1
let first = items[0];
let second = items[1];

// ❌ BAD - other index values
let third = items[2];  // Should be named if it has semantic meaning
```

**Buffer sizes**: Always use named constants
```rust
// ✅ GOOD
const READ_BUFFER_SIZE: usize = 8192;
let mut buf = vec![0u8; READ_BUFFER_SIZE];

// ❌ BAD
let mut buf = vec![0u8; 8192];
```

### Where to Define Constants

- **Module-level**: For constants used only within one file
- **Crate-level** (`src/constants.rs`): For constants used across modules
- Group related constants together with documentation

### Test Files Exception

Test files (`*_tests.rs`) may use literal values for test data when it improves readability and the values are only used once. However, if the same test value appears multiple times or represents a meaningful configuration value, it should still use the global constants.

### Verification

Before committing, manually scan code for numeric literals:
```bash
# Find numeric literals other than 0 and 1 in Rust files (excludes test files)
grep -Ern '\b[2-9][0-9]*\b' src/ --include="*.rs" --exclude="*_tests.rs" | grep -v '^[^:]*:[^:]*://.*$'
```

---

## Code Quality: Use Global Constants for Repeated Strings

When a string literal appears in multiple places across the codebase, it MUST be defined as a global constant and referenced consistently.

### Why

- **Single Source of Truth**: Changes only need to be made in one place
- **Consistency**: Prevents typos and inconsistencies across the codebase
- **Maintainability**: Easier to refactor and update values
- **Type Safety**: Compiler catches usage errors

### When to Create a Global Constant

- String appears 2+ times in the same file
- String appears in multiple files
- String represents a configuration value (paths, filenames, keys, etc.)
- String is part of an API contract or protocol

### Examples

```rust
// ✅ GOOD - Use constants
const BIND_NAMED_CONF_PATH: &str = "/etc/bind/named.conf";
const NAMED_CONF_FILENAME: &str = "named.conf";

fn build_configmap() {
    data.insert(NAMED_CONF_FILENAME.into(), named_conf);
}

// ❌ BAD - Hardcoded strings
fn build_configmap() {
    data.insert("named.conf".into(), named_conf);
}
```

### Where to Define Constants

- Module-level constants: At the top of the file for file-specific use
- Crate-level constants: In a dedicated module (e.g., `src/constants.rs`) for cross-module use
- Group related constants together with documentation

### Verification

Before committing, search for repeated string literals:
```bash
# Find potential duplicate strings in Rust files
grep -rn '"[^"]\{5,\}"' src/ | sort | uniq -d
```

---

## Dependency Management

Before adding a new dependency:
1. Check if existing deps solve the problem
2. Verify the crate is actively maintained (commits in last 6 months)
3. Prefer crates from well-known authors or the Rust ecosystem
4. Document why the dependency was added in `.claude/CHANGELOG.md`

---

## Code Comments

All public functions and types **must** have rustdoc comments:

```rust
/// Reconciles the BindZone custom resource.
///
/// # Arguments
/// * `zone` - The BindZone CR to reconcile
/// * `ctx` - Controller context with client and state
///
/// # Errors
/// Returns `ReconcileError` if DNS zone update fails or API is unreachable.
pub async fn reconcile(zone: Arc<BindZone>, ctx: Arc<Context>) -> Result<Action, ReconcileError> {
```

---

## Things to Never Do

- **Never** use `unwrap()` in production code - use `?` or explicit error handling
- **Never** hardcode namespaces - make them configurable
- **Never** use `sleep()` for synchronization - use proper k8s watch/informers
- **Never** ignore errors in finalizers - this blocks resource deletion
- **Never** store state outside of Kubernetes - operators must be stateless
