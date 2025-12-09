// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for TXT record operations.

#[cfg(test)]
mod tests {
    use crate::bind9::{Bind9Manager, RndcKeyData};

    #[tokio::test]
    #[ignore = "Requires running BIND9 server with TSIG key configured for dynamic DNS updates"]
    async fn test_add_txt_record_placeholder() {
        let manager = Bind9Manager::new();
        let key_data = RndcKeyData {
            name: "test".to_string(),
            algorithm: crate::crd::RndcAlgorithm::HmacSha256,
            secret: "dGVzdA==".to_string(),
        };

        let texts = vec!["v=spf1 mx ~all".to_string()];
        let result = manager
            .add_txt_record(
                "example.com",
                "@",
                &texts,
                Some(3600),
                "127.0.0.1:53",
                &key_data,
            )
            .await;

        let _ = result;
    }
}
