# Label Selectors

Label selectors determine which Bind9Instances will host a zone.

## Match Labels

Simple equality-based selection:

```yaml
instanceSelector:
  matchLabels:
    dns-role: primary
    environment: production
```

## Match Expressions

Advanced selection with operators:

```yaml
instanceSelector:
  matchExpressions:
    - key: dns-role
      operator: In
      values: [primary, secondary]
    - key: region
      operator: Exists
```

### Operators

- `In` - Label value must be in the list
- `NotIn` - Label value must not be in the list  
- `Exists` - Label key must exist
- `DoesNotExist` - Label key must not exist
