// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for CAA record operations.

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData};

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_caa_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_caa_record(
                "example.com",
                "@",
                0,
                "issue",
                "letsencrypt.org",
                Some(3600),
                "localhost:9530",
                &key_data,
            )
            .await;

        assert!(result.is_ok());
    }
}
