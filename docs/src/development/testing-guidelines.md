# Testing Guidelines

Guidelines for writing tests in Bindy.

## Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_name() {
        // Arrange
        let input = create_input();
        
        // Act
        let result = function_under_test(input);
        
        // Assert
        assert_eq!(result, expected);
    }
}
```

## Unit Tests

Test individual functions:

```rust
#[test]
fn test_build_configmap() {
    let instance = create_test_instance();
    let configmap = build_configmap(&instance);
    
    assert_eq!(configmap.metadata.name, Some("test".to_string()));
}
```

## Integration Tests

Test with Kubernetes:

```rust
#[tokio::test]
#[ignore]  // Requires cluster
async fn test_full_reconciliation() {
    let client = Client::try_default().await.unwrap();
    // Test logic
}
```

## Test Coverage

Aim for >80% coverage on new code.

## CI Tests

All tests run on:
- Pull requests
- Main branch commits
