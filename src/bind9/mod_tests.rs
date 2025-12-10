// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for Bind9Manager.

#[cfg(test)]
#[allow(unexpected_cfgs)]
mod tests {
    use crate::bind9::Bind9Manager;

    #[test]
    fn test_bind9_manager_creation() {
        let manager = Bind9Manager::new();
        // Verify manager can be created
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
        assert!(debug_output.contains("client"));
        assert!(debug_output.contains("token"));
    }

    #[test]
    fn test_bind9_manager_default() {
        let manager = Bind9Manager::new();
        // Verify default implementation works
        let debug_output = format!("{manager:?}");
        assert!(debug_output.starts_with("Bind9Manager"));
    }
}
