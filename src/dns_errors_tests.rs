// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for DNS error types.

#[cfg(test)]
mod tests {
    use crate::dns_errors::*;

    #[test]
    fn test_zone_not_found_error() {
        let error = ZoneError::ZoneNotFound {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Zone 'example.com' not found on endpoint 10.0.0.1:8080 (HTTP 404)"
        );
    }

    #[test]
    fn test_zone_creation_failed_error() {
        let error = ZoneError::ZoneCreationFailed {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
            reason: "Filesystem full".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Failed to create zone 'example.com' on endpoint 10.0.0.1:8080: Filesystem full"
        );
    }

    #[test]
    fn test_zone_already_exists_error() {
        let error = ZoneError::ZoneAlreadyExists {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Zone 'example.com' already exists on endpoint 10.0.0.1:8080"
        );
    }

    #[test]
    fn test_zone_deletion_failed_error() {
        let error = ZoneError::ZoneDeletionFailed {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
            reason: "Zone in use".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Failed to delete zone 'example.com' on endpoint 10.0.0.1:8080: Zone in use"
        );
    }

    #[test]
    fn test_invalid_zone_configuration_error() {
        let error = ZoneError::InvalidZoneConfiguration {
            zone: "example.com".to_string(),
            reason: "Invalid SOA serial number".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Invalid zone configuration for 'example.com': Invalid SOA serial number"
        );
    }

    #[test]
    fn test_record_not_found_error() {
        let error = RecordError::RecordNotFound {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "DNS record 'www' in zone 'example.com' not found on server 10.0.0.1:53 (no answer)"
        );
    }

    #[test]
    fn test_record_update_failed_error() {
        let error = RecordError::RecordUpdateFailed {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
            reason: "TSIG verification failed".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Failed to update record 'www.example.com' on server 10.0.0.1:53: TSIG verification failed"
        );
    }

    #[test]
    fn test_record_deletion_failed_error() {
        let error = RecordError::RecordDeletionFailed {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
            reason: "Record does not exist".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Failed to delete record 'www.example.com' on server 10.0.0.1:53: Record does not exist"
        );
    }

    #[test]
    fn test_invalid_record_data_error() {
        let error = RecordError::InvalidRecordData {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            reason: "Invalid IPv4 address format".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Invalid record data for 'www.example.com': Invalid IPv4 address format"
        );
    }

    #[test]
    fn test_bind9_instance_unavailable_error() {
        let error = InstanceError::Bind9InstanceUnavailable {
            endpoint: "10.0.0.1:8080".to_string(),
            status_code: 502,
        };

        assert_eq!(
            error.to_string(),
            "BIND9 instance at 10.0.0.1:8080 unavailable (HTTP 502)"
        );
    }

    #[test]
    fn test_http_connection_failed_error() {
        let error = InstanceError::HttpConnectionFailed {
            endpoint: "10.0.0.1:8080".to_string(),
            reason: "Connection refused".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "HTTP connection to 10.0.0.1:8080 failed: Connection refused"
        );
    }

    #[test]
    fn test_http_request_timeout_error() {
        let error = InstanceError::HttpRequestTimeout {
            endpoint: "10.0.0.1:8080".to_string(),
            timeout_ms: 5000,
        };

        assert_eq!(
            error.to_string(),
            "HTTP request to 10.0.0.1:8080 timed out after 5000ms"
        );
    }

    #[test]
    fn test_unexpected_http_response_error() {
        let error = InstanceError::UnexpectedHttpResponse {
            endpoint: "10.0.0.1:8080".to_string(),
            status_code: 418,
            reason: "I'm a teapot".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Unexpected HTTP response from 10.0.0.1:8080: 418 I'm a teapot"
        );
    }

    #[test]
    fn test_tsig_connection_error() {
        let error = TsigError::TsigConnectionError {
            server: "10.0.0.1:53".to_string(),
            reason: "Clock skew too large".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "TSIG authentication failed for server 10.0.0.1:53: Clock skew too large"
        );
    }

    #[test]
    fn test_tsig_key_not_found_error() {
        let error = TsigError::TsigKeyNotFound {
            secret_name: "my-instance-rndc-key".to_string(),
            namespace: "dns-system".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "TSIG key secret 'my-instance-rndc-key' not found in namespace 'dns-system'"
        );
    }

    #[test]
    fn test_invalid_tsig_key_data_error() {
        let error = TsigError::InvalidTsigKeyData {
            secret_name: "my-instance-rndc-key".to_string(),
            reason: "Missing algorithm field".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Invalid TSIG key data in secret 'my-instance-rndc-key': Missing algorithm field"
        );
    }

    #[test]
    fn test_tsig_verification_failed_error() {
        let error = TsigError::TsigVerificationFailed {
            server: "10.0.0.1:53".to_string(),
            key_name: "rndc-key".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "TSIG verification failed on server 10.0.0.1:53 for key 'rndc-key'"
        );
    }

    #[test]
    fn test_zone_transfer_failed_error() {
        let error = ZoneTransferError::TransferFailed {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
            secondary: "10.0.0.2".to_string(),
            reason: "Network unreachable".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Zone transfer for 'example.com' from 10.0.0.1 to 10.0.0.2 failed: Network unreachable"
        );
    }

    #[test]
    fn test_zone_transfer_refused_error() {
        let error = ZoneTransferError::TransferRefused {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "Zone transfer for 'example.com' refused by primary 10.0.0.1 (not in allow-transfer)"
        );
    }

    #[test]
    fn test_zone_transfer_timeout_error() {
        let error = ZoneTransferError::TransferTimeout {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
            timeout_secs: 300,
        };

        assert_eq!(
            error.to_string(),
            "Zone transfer for 'example.com' from 10.0.0.1 timed out after 300s"
        );
    }

    #[test]
    fn test_dns_error_from_zone_error() {
        let zone_error = ZoneError::ZoneNotFound {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
        };
        let dns_error: DnsError = zone_error.into();

        assert!(matches!(dns_error, DnsError::Zone(_)));
    }

    #[test]
    fn test_dns_error_from_record_error() {
        let record_error = RecordError::RecordNotFound {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
        };
        let dns_error: DnsError = record_error.into();

        assert!(matches!(dns_error, DnsError::Record(_)));
    }

    #[test]
    fn test_dns_error_from_instance_error() {
        let instance_error = InstanceError::Bind9InstanceUnavailable {
            endpoint: "10.0.0.1:8080".to_string(),
            status_code: 502,
        };
        let dns_error: DnsError = instance_error.into();

        assert!(matches!(dns_error, DnsError::Instance(_)));
    }

    #[test]
    fn test_dns_error_from_tsig_error() {
        let tsig_error = TsigError::TsigConnectionError {
            server: "10.0.0.1:53".to_string(),
            reason: "Clock skew".to_string(),
        };
        let dns_error: DnsError = tsig_error.into();

        assert!(matches!(dns_error, DnsError::Tsig(_)));
    }

    #[test]
    fn test_dns_error_from_zone_transfer_error() {
        let transfer_error = ZoneTransferError::TransferFailed {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
            secondary: "10.0.0.2".to_string(),
            reason: "Network error".to_string(),
        };
        let dns_error: DnsError = transfer_error.into();

        assert!(matches!(dns_error, DnsError::ZoneTransfer(_)));
    }

    #[test]
    fn test_is_transient_zone_errors() {
        // Zone not found is permanent
        let error = DnsError::Zone(ZoneError::ZoneNotFound {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
        });
        assert!(!error.is_transient());

        // Zone creation failure is transient (might be filesystem issue)
        let error = DnsError::Zone(ZoneError::ZoneCreationFailed {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
            reason: "Disk full".to_string(),
        });
        assert!(error.is_transient());

        // Zone already exists is permanent
        let error = DnsError::Zone(ZoneError::ZoneAlreadyExists {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
        });
        assert!(!error.is_transient());

        // Invalid configuration is permanent
        let error = DnsError::Zone(ZoneError::InvalidZoneConfiguration {
            zone: "example.com".to_string(),
            reason: "Bad SOA".to_string(),
        });
        assert!(!error.is_transient());
    }

    #[test]
    fn test_is_transient_record_errors() {
        // Record not found is permanent
        let error = DnsError::Record(RecordError::RecordNotFound {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
        });
        assert!(!error.is_transient());

        // Record update failure is transient (might be network issue)
        let error = DnsError::Record(RecordError::RecordUpdateFailed {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
            reason: "Timeout".to_string(),
        });
        assert!(error.is_transient());

        // Invalid record data is permanent
        let error = DnsError::Record(RecordError::InvalidRecordData {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            reason: "Bad IP".to_string(),
        });
        assert!(!error.is_transient());
    }

    #[test]
    fn test_is_transient_instance_errors() {
        // All instance errors are transient
        let error = DnsError::Instance(InstanceError::Bind9InstanceUnavailable {
            endpoint: "10.0.0.1:8080".to_string(),
            status_code: 502,
        });
        assert!(error.is_transient());

        let error = DnsError::Instance(InstanceError::HttpConnectionFailed {
            endpoint: "10.0.0.1:8080".to_string(),
            reason: "Refused".to_string(),
        });
        assert!(error.is_transient());

        let error = DnsError::Instance(InstanceError::HttpRequestTimeout {
            endpoint: "10.0.0.1:8080".to_string(),
            timeout_ms: 5000,
        });
        assert!(error.is_transient());
    }

    #[test]
    fn test_is_transient_tsig_errors() {
        // TSIG connection error is transient (might be temporary)
        let error = DnsError::Tsig(TsigError::TsigConnectionError {
            server: "10.0.0.1:53".to_string(),
            reason: "Clock skew".to_string(),
        });
        assert!(error.is_transient());

        // TSIG key not found is permanent
        let error = DnsError::Tsig(TsigError::TsigKeyNotFound {
            secret_name: "key".to_string(),
            namespace: "ns".to_string(),
        });
        assert!(!error.is_transient());

        // Invalid key data is permanent
        let error = DnsError::Tsig(TsigError::InvalidTsigKeyData {
            secret_name: "key".to_string(),
            reason: "Bad data".to_string(),
        });
        assert!(!error.is_transient());

        // Verification failure is permanent
        let error = DnsError::Tsig(TsigError::TsigVerificationFailed {
            server: "10.0.0.1:53".to_string(),
            key_name: "key".to_string(),
        });
        assert!(!error.is_transient());
    }

    #[test]
    fn test_is_transient_zone_transfer_errors() {
        // Transfer failed is transient
        let error = DnsError::ZoneTransfer(ZoneTransferError::TransferFailed {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
            secondary: "10.0.0.2".to_string(),
            reason: "Network".to_string(),
        });
        assert!(error.is_transient());

        // Transfer refused is permanent (ACL issue)
        let error = DnsError::ZoneTransfer(ZoneTransferError::TransferRefused {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
        });
        assert!(!error.is_transient());

        // Transfer timeout is transient
        let error = DnsError::ZoneTransfer(ZoneTransferError::TransferTimeout {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
            timeout_secs: 300,
        });
        assert!(error.is_transient());
    }

    #[test]
    fn test_status_reason_zone_errors() {
        let error = DnsError::Zone(ZoneError::ZoneNotFound {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
        });
        assert_eq!(error.status_reason(), "ZoneNotFound");

        let error = DnsError::Zone(ZoneError::ZoneCreationFailed {
            zone: "example.com".to_string(),
            endpoint: "10.0.0.1:8080".to_string(),
            reason: "Error".to_string(),
        });
        assert_eq!(error.status_reason(), "ZoneCreationFailed");
    }

    #[test]
    fn test_status_reason_record_errors() {
        let error = DnsError::Record(RecordError::RecordNotFound {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
        });
        assert_eq!(error.status_reason(), "RecordNotFound");

        let error = DnsError::Record(RecordError::RecordUpdateFailed {
            name: "www".to_string(),
            zone: "example.com".to_string(),
            server: "10.0.0.1:53".to_string(),
            reason: "Error".to_string(),
        });
        assert_eq!(error.status_reason(), "RecordUpdateFailed");
    }

    #[test]
    fn test_status_reason_instance_errors() {
        let error = DnsError::Instance(InstanceError::Bind9InstanceUnavailable {
            endpoint: "10.0.0.1:8080".to_string(),
            status_code: 502,
        });
        assert_eq!(error.status_reason(), "Bind9InstanceUnavailable");

        let error = DnsError::Instance(InstanceError::HttpConnectionFailed {
            endpoint: "10.0.0.1:8080".to_string(),
            reason: "Refused".to_string(),
        });
        assert_eq!(error.status_reason(), "HttpConnectionFailed");
    }

    #[test]
    fn test_status_reason_tsig_errors() {
        let error = DnsError::Tsig(TsigError::TsigConnectionError {
            server: "10.0.0.1:53".to_string(),
            reason: "Error".to_string(),
        });
        assert_eq!(error.status_reason(), "TsigConnectionError");

        let error = DnsError::Tsig(TsigError::TsigKeyNotFound {
            secret_name: "key".to_string(),
            namespace: "ns".to_string(),
        });
        assert_eq!(error.status_reason(), "TsigKeyNotFound");
    }

    #[test]
    fn test_status_reason_zone_transfer_errors() {
        let error = DnsError::ZoneTransfer(ZoneTransferError::TransferFailed {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
            secondary: "10.0.0.2".to_string(),
            reason: "Error".to_string(),
        });
        assert_eq!(error.status_reason(), "ZoneTransferFailed");

        let error = DnsError::ZoneTransfer(ZoneTransferError::TransferRefused {
            zone: "example.com".to_string(),
            primary: "10.0.0.1".to_string(),
        });
        assert_eq!(error.status_reason(), "ZoneTransferRefused");
    }

    #[test]
    fn test_generic_dns_error() {
        let error = DnsError::Generic("Something went wrong".to_string());
        assert_eq!(
            error.to_string(),
            "DNS operation failed: Something went wrong"
        );
        assert_eq!(error.status_reason(), "DnsOperationFailed");
        assert!(error.is_transient()); // Generic errors are assumed transient
    }

    #[test]
    fn test_dns_error_from_anyhow_error() {
        let anyhow_error = anyhow::anyhow!("Test error");
        let dns_error: DnsError = anyhow_error.into();

        assert!(matches!(dns_error, DnsError::Generic(_)));
        assert_eq!(dns_error.to_string(), "DNS operation failed: Test error");
    }
}
