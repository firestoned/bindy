# DNS Error Usage Guide

This guide explains how to use the structured DNS error types in Bindy.

## Overview

The `dns_errors` module provides structured error types for DNS operations performed via:
- **Bindcar HTTP API** - Zone management and configuration
- **Hickory DNS Client** - Dynamic DNS updates (nsupdate) and queries

All error types use `thiserror` for automatic `Display` and `Error` trait implementations.

## Error Types

### 1. ZoneError

Errors related to DNS zone operations via Bindcar HTTP API.

```rust
use bindy::dns_errors::{ZoneError, DnsError};

// Zone not found (HTTP 404)
let error = ZoneError::ZoneNotFound {
    zone: "example.com".to_string(),
    endpoint: "10.0.0.1:8080".to_string(),
};

// Zone creation failed
let error = ZoneError::ZoneCreationFailed {
    zone: "example.com".to_string(),
    endpoint: "10.0.0.1:8080".to_string(),
    reason: "Disk full".to_string(),
};

// Zone already exists (non-fatal, can be ignored)
let error = ZoneError::ZoneAlreadyExists {
    zone: "example.com".to_string(),
    endpoint: "10.0.0.1:8080".to_string(),
};

// Invalid zone configuration
let error = ZoneError::InvalidZoneConfiguration {
    zone: "example.com".to_string(),
    reason: "Invalid SOA serial number".to_string(),
};
```

### 2. RecordError

Errors related to DNS record operations via Hickory DNS client.

```rust
use bindy::dns_errors::RecordError;

// Record not found when querying DNS
let error = RecordError::RecordNotFound {
    name: "www".to_string(),
    zone: "example.com".to_string(),
    server: "10.0.0.1:53".to_string(),
};

// Dynamic update failed
let error = RecordError::RecordUpdateFailed {
    name: "www".to_string(),
    zone: "example.com".to_string(),
    server: "10.0.0.1:53".to_string(),
    reason: "TSIG verification failed".to_string(),
};

// Invalid record data (validation failed before sending)
let error = RecordError::InvalidRecordData {
    name: "www".to_string(),
    zone: "example.com".to_string(),
    reason: "Invalid IPv4 address format".to_string(),
};
```

### 3. InstanceError

Errors related to BIND9 instance availability and HTTP connectivity.

```rust
use bindy::dns_errors::InstanceError;

// BIND9 instance unavailable (HTTP 502 or 503)
let error = InstanceError::Bind9InstanceUnavailable {
    endpoint: "10.0.0.1:8080".to_string(),
    status_code: 502,
};

// HTTP connection failed
let error = InstanceError::HttpConnectionFailed {
    endpoint: "10.0.0.1:8080".to_string(),
    reason: "Connection refused".to_string(),
};

// HTTP request timeout
let error = InstanceError::HttpRequestTimeout {
    endpoint: "10.0.0.1:8080".to_string(),
    timeout_ms: 5000,
};
```

### 4. TsigError

Errors related to TSIG authentication for dynamic DNS updates.

```rust
use bindy::dns_errors::TsigError;

// TSIG authentication failed
let error = TsigError::TsigConnectionError {
    server: "10.0.0.1:53".to_string(),
    reason: "Clock skew too large".to_string(),
};

// TSIG key secret not found in Kubernetes
let error = TsigError::TsigKeyNotFound {
    secret_name: "my-instance-rndc-key".to_string(),
    namespace: "dns-system".to_string(),
};

// Invalid TSIG key data in secret
let error = TsigError::InvalidTsigKeyData {
    secret_name: "my-instance-rndc-key".to_string(),
    reason: "Missing algorithm field".to_string(),
};
```

### 5. ZoneTransferError

Errors related to zone transfer operations (AXFR/IXFR).

```rust
use bindy::dns_errors::ZoneTransferError;

// Zone transfer failed
let error = ZoneTransferError::TransferFailed {
    zone: "example.com".to_string(),
    primary: "10.0.0.1".to_string(),
    secondary: "10.0.0.2".to_string(),
    reason: "Network unreachable".to_string(),
};

// Zone transfer refused by primary (ACL issue)
let error = ZoneTransferError::TransferRefused {
    zone: "example.com".to_string(),
    primary: "10.0.0.1".to_string(),
};
```

## The DnsError Composite Type

`DnsError` wraps all specific error types and provides helper methods.

```rust
use bindy::dns_errors::{DnsError, ZoneError, RecordError};

// Automatic conversion from specific error types
let zone_error = ZoneError::ZoneNotFound {
    zone: "example.com".to_string(),
    endpoint: "10.0.0.1:8080".to_string(),
};
let dns_error: DnsError = zone_error.into();

// Pattern matching on DnsError
match dns_error {
    DnsError::Zone(ze) => println!("Zone error: {}", ze),
    DnsError::Record(re) => println!("Record error: {}", re),
    DnsError::Instance(ie) => println!("Instance error: {}", ie),
    DnsError::Tsig(te) => println!("TSIG error: {}", te),
    DnsError::ZoneTransfer(zte) => println!("Zone transfer error: {}", zte),
    DnsError::Generic(msg) => println!("Generic error: {}", msg),
}
```

## Helper Methods

### `is_transient()` - Retry Logic

Determine if an error should be retried or is permanent.

```rust
use bindy::dns_errors::{DnsError, ZoneError, RecordError};

let error = DnsError::Zone(ZoneError::ZoneNotFound {
    zone: "example.com".to_string(),
    endpoint: "10.0.0.1:8080".to_string(),
});

if error.is_transient() {
    // Retry the operation
    println!("Error is transient, will retry");
} else {
    // Don't retry, log and return
    println!("Error is permanent, giving up");
}
```

**Transient errors** (should retry):
- Zone creation/deletion failures (might be filesystem issues)
- Record update/deletion failures (might be network issues)
- All instance errors (connectivity issues)
- TSIG connection errors (might be temporary clock skew)
- Zone transfer failures and timeouts

**Permanent errors** (should NOT retry):
- Zone not found
- Zone already exists
- Invalid zone/record configuration
- TSIG key not found or invalid
- TSIG verification failed
- Zone transfer refused (ACL configuration issue)

### `status_reason()` - Kubernetes Status Codes

Get a Kubernetes-standard status reason code for updating CRD status conditions.

```rust
use bindy::dns_errors::{DnsError, ZoneError};

let error = DnsError::Zone(ZoneError::ZoneNotFound {
    zone: "example.com".to_string(),
    endpoint: "10.0.0.1:8080".to_string(),
});

// Returns "ZoneNotFound" - suitable for Kubernetes status.conditions[].reason
let reason = error.status_reason();

// Use in status update
update_record_status(
    &client,
    &record,
    "Ready",
    "False",
    reason,  // "ZoneNotFound"
    &error.to_string(),  // Full error message
    current_generation,
)
.await?;
```

## Usage in Reconcilers

Example of using DNS errors in a reconciler:

```rust
use bindy::dns_errors::{DnsError, ZoneError, RecordError};
use anyhow::Result;

async fn reconcile_dns_record(record: ARecord) -> Result<()> {
    // Attempt to add record
    match add_record_to_zone(&record).await {
        Ok(()) => {
            update_status(&record, "Ready", "True", "RecordAvailable", "Record created").await?;
        }
        Err(e) => {
            let dns_error: DnsError = e.into();

            // Check if we should retry
            if dns_error.is_transient() {
                warn!("Transient error adding record: {}, will retry", dns_error);
                // Return error to trigger requeue
                return Err(dns_error.into());
            } else {
                warn!("Permanent error adding record: {}, giving up", dns_error);
                // Update status with failure, but don't requeue
                update_status(
                    &record,
                    "Ready",
                    "False",
                    dns_error.status_reason(),
                    &dns_error.to_string(),
                ).await?;
            }
        }
    }

    Ok(())
}
```

## Converting from HTTP Errors

When working with Bindcar HTTP API, convert HTTP status codes to appropriate DNS errors:

```rust
use bindy::dns_errors::{DnsError, ZoneError, InstanceError};

async fn create_zone_via_http(zone: &str, endpoint: &str) -> Result<(), DnsError> {
    match http_client.post(&format!("{}/zones/{}", endpoint, zone)).send().await {
        Ok(response) => {
            match response.status().as_u16() {
                200 | 201 => Ok(()),
                404 => Err(DnsError::Zone(ZoneError::ZoneNotFound {
                    zone: zone.to_string(),
                    endpoint: endpoint.to_string(),
                })),
                409 => Err(DnsError::Zone(ZoneError::ZoneAlreadyExists {
                    zone: zone.to_string(),
                    endpoint: endpoint.to_string(),
                })),
                502 | 503 => Err(DnsError::Instance(InstanceError::Bind9InstanceUnavailable {
                    endpoint: endpoint.to_string(),
                    status_code: response.status().as_u16(),
                })),
                code => Err(DnsError::Instance(InstanceError::UnexpectedHttpResponse {
                    endpoint: endpoint.to_string(),
                    status_code: code,
                    reason: response.text().await.unwrap_or_default(),
                })),
            }
        }
        Err(e) if e.is_timeout() => Err(DnsError::Instance(InstanceError::HttpRequestTimeout {
            endpoint: endpoint.to_string(),
            timeout_ms: 5000,
        })),
        Err(e) => Err(DnsError::Instance(InstanceError::HttpConnectionFailed {
            endpoint: endpoint.to_string(),
            reason: e.to_string(),
        })),
    }
}
```

## Best Practices

1. **Always provide context**: Include zone names, endpoints, and specific reasons in error messages
2. **Use `is_transient()` for retry logic**: Don't blindly retry permanent errors
3. **Use `status_reason()` for Kubernetes status**: Provides consistent status condition reasons
4. **Convert at API boundaries**: Convert HTTP/DNS errors to DnsError at the lowest level
5. **Log with full context**: Use the full error message for logging, status_reason() for metrics
6. **Test error paths**: Write tests for both success and all error scenarios

## Migration from anyhow::Error

The `DnsError` type implements `From<anyhow::Error>` for backward compatibility:

```rust
use bindy::dns_errors::DnsError;

// Old code using anyhow
fn old_function() -> anyhow::Result<()> {
    Err(anyhow::anyhow!("Something went wrong"))
}

// Can be converted to DnsError
let result = old_function();
if let Err(e) = result {
    let dns_error: DnsError = e.into();
    // Now dns_error is DnsError::Generic("Something went wrong")
}
```

However, for new code, prefer using the specific error types directly for better structure and context.
