// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for SRV record operations.

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData, SRVRecordData};

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_srv_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let srv_data = SRVRecordData {
            priority: 10,
            weight: 60,
            port: 5060,
            target: "sip.example.com.".to_string(),
            ttl: Some(3600),
        };

        let result = manager
            .add_srv_record(
                "example.com",
                "_sip._tcp",
                &srv_data,
                "localhost:9530",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }
}
