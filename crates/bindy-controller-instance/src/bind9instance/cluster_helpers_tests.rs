// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `cluster_helpers.rs`
//!
//! These tests document expected behavior for cluster integration helpers.
//! Full implementation requires Kubernetes API mocking infrastructure.

#[cfg(test)]
mod tests {
    use super::super::parent_or_none;

    fn api_error(code: u16) -> anyhow::Error {
        anyhow::Error::from(kube::Error::Api(
            kube::core::Status::failure("test error", "TestReason")
                .with_code(code)
                .boxed(),
        ))
    }

    #[test]
    fn test_parent_or_none_found_is_some() {
        assert_eq!(
            parent_or_none(Ok::<_, anyhow::Error>(7)).expect("ok"),
            Some(7)
        );
    }

    #[test]
    fn test_parent_or_none_not_found_is_none() {
        // Only a 404 means "this instance has no such parent".
        assert_eq!(
            parent_or_none::<i32>(Err(api_error(404))).expect("ok"),
            None
        );
    }

    #[test]
    fn test_parent_or_none_context_wrapped_not_found_is_none() {
        use anyhow::Context;
        let err = Err::<i32, _>(api_error(404))
            .context("get Bind9Cluster")
            .unwrap_err();
        assert_eq!(parent_or_none::<i32>(Err(err)).expect("ok"), None);
    }

    #[test]
    fn test_parent_or_none_transient_errors_propagate() {
        // A transient API error must fail the reconcile, never build the
        // Deployment without the cluster's image, version, RNDC or volumes.
        for code in [401, 403, 429, 500, 503] {
            assert!(
                parent_or_none::<i32>(Err(api_error(code))).is_err(),
                "HTTP {code} must propagate"
            );
        }
        assert!(parent_or_none::<i32>(Err(anyhow::anyhow!("retries exhausted"))).is_err());
    }
}
