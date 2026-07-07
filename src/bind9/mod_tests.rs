// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `Bind9Manager`.

#[cfg(test)]
#[allow(unexpected_cfgs)]
mod tests {
    use crate::bind9::Bind9Manager;

    /// Install the ring TLS crypto provider for this test process.
    ///
    /// `Bind9Manager::new()` builds a `reqwest` client, and reqwest is compiled
    /// with `rustls-no-provider` (so bindy stays ring-only and never pulls in
    /// aws-lc-sys). reqwest therefore relies on the process-default
    /// `CryptoProvider`, which `main.rs` installs at startup but unit tests do
    /// not. Install it here; `install_default` returns `Err` once a provider is
    /// already set, so calling it from every test is safe and idempotent.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn test_bind9_manager_creation() {
        ensure_crypto_provider();
        let manager = Bind9Manager::new();
        // Verify manager can be created
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
        assert!(debug_output.contains("client"));
        assert!(debug_output.contains("token"));
    }

    #[test]
    fn test_bind9_manager_default() {
        ensure_crypto_provider();
        let manager = Bind9Manager::new();
        // Verify default implementation works
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
    }
}
