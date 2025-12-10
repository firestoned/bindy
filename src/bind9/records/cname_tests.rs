// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for CNAME record operations.

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData};

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_cname_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let result = manager
            .add_cname_record(
                "example.com",
                "blog",
                "www.example.com.",
                Some(300),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        let _ = result;
    }
}
