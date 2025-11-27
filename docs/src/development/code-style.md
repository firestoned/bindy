# Code Style

Code style guidelines for Bindy.

## Rust Style

Follow official Rust style guide:

```bash
# Format code
cargo fmt

# Check for issues
cargo clippy
```

## Naming Conventions

- `snake_case` for functions, variables
- `PascalCase` for types, traits
- `SCREAMING_SNAKE_CASE` for constants

## Documentation

Document public APIs:

```rust
/// Reconciles a Bind9Instance resource.
///
/// Creates or updates Kubernetes resources for BIND9.
///
/// # Arguments
///
/// * `instance` - The Bind9Instance to reconcile
///
/// # Returns
///
/// Ok(()) on success, Err on failure
pub async fn reconcile(instance: Bind9Instance) -> Result<()> {
    // Implementation
}
```

## Error Handling

Use `anyhow::Result` for errors:

```rust
use anyhow::{Context, Result};

fn do_thing() -> Result<()> {
    some_operation()
        .context("Failed to do thing")?;
    Ok(())
}
```

## Testing

Write tests for all public functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_function() {
        assert_eq!(function(), expected);
    }
}
```
