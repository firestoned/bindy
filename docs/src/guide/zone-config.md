# Zone Configuration

Advanced zone configuration options.

## Default TTL

Set the default TTL for all records in the zone:

```yaml
spec:
  ttl: 3600  # 1 hour
```

## SOA Record Details

```yaml
spec:
  soaRecord:
    primaryNS: ns1.example.com.    # Primary nameserver FQDN (must end with .)
    adminEmail: admin@example.com  # Admin email (@ replaced with . in zone file)
    serial: 2024010101             # Serial number (YYYYMMDDnn format recommended)
    refresh: 3600                  # How often secondaries check for updates (seconds)
    retry: 600                     # How long to wait before retry after failed refresh
    expire: 604800                 # When to stop answering if no refresh (1 week)
    negativeTTL: 86400             # TTL for negative responses (NXDOMAIN)
```

## Secondary Zone Configuration

For secondary zones, specify primary servers:

```yaml
spec:
  type: secondary
  secondaryConfig:
    primaryServers:
      - "10.0.1.10"
      - "10.0.1.11"
```
