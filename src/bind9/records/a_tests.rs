// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for A and AAAA record operations.

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData};

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_a_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        // This test now requires a real BIND9 server with TSIG authentication
        let result = manager
            .add_a_record(
                "example.com",
                "www",
                "192.0.2.1",
                Some(300),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        // Will fail without real server - test is ignored by default
        let _ = result;
    }

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_aaaa_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_aaaa_record(
                "example.com",
                "www",
                "2001:db8::1",
                Some(300),
                "localhost:9530",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }
}
