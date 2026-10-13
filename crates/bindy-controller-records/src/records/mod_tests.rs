// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `records/mod.rs`: the `DNSZone` store lookup with an API
//! fallback (ADR-0016 decision 5).

#[cfg(test)]
mod tests {
    use super::super::cached_or_fetched;
    use futures::executor::block_on;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone, Debug, PartialEq)]
    struct Zone(&'static str);

    #[test]
    fn a_cached_object_is_returned_without_calling_the_api() {
        let calls = AtomicUsize::new(0);

        let got = block_on(cached_or_fetched(
            Some(Arc::new(Zone("from-store"))),
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(Zone("from-api")))
            },
        ))
        .expect("lookup succeeds");

        assert_eq!(got, Some(Zone("from-store")));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the store hit must not GET"
        );
    }

    #[test]
    fn an_object_the_store_does_not_hold_falls_back_to_the_api() {
        let calls = AtomicUsize::new(0);

        let got = block_on(cached_or_fetched(None, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(Zone("from-api")))
        }))
        .expect("lookup succeeds");

        assert_eq!(got, Some(Zone("from-api")));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_object_missing_from_the_store_and_the_api_is_none() {
        let got = block_on(cached_or_fetched::<Zone, _, _>(None, || async { Ok(None) }))
            .expect("a 404 is not an error");

        assert_eq!(got, None);
    }

    #[test]
    fn an_api_error_on_the_fallback_is_returned() {
        let got = block_on(cached_or_fetched::<Zone, _, _>(None, || async {
            Err(anyhow::anyhow!("timeout"))
        }));

        assert!(got.unwrap_err().to_string().contains("timeout"));
    }
}

/// Unit tests for the deletion decisions in `delete_record`: an API failure
/// must keep the finalizer (retry), and only a definite "nothing is
/// published" lets the finalizer go (bug B2, ADR-0015).
#[cfg(test)]
mod delete_step_tests {
    use super::super::{instances_step, primaries_step, zone_lookup_step, DeleteStep};

    #[test]
    fn a_found_zone_proceeds() {
        let step = zone_lookup_step(Ok(Some("zone")));
        assert!(matches!(step, DeleteStep::Proceed("zone")));
    }

    #[test]
    fn a_zone_that_does_not_exist_allows_deletion() {
        let step = zone_lookup_step::<&str>(Ok(None));
        assert!(matches!(step, DeleteStep::AllowDeletion));
    }

    #[test]
    fn a_failed_zone_lookup_retries() {
        let step = zone_lookup_step::<&str>(Err(anyhow::anyhow!("apiserver timeout")));
        let DeleteStep::Retry(err) = step else {
            panic!("an API failure on the zone lookup must retry, not drop the finalizer");
        };
        assert!(err.to_string().contains("apiserver timeout"));
    }

    #[test]
    fn selected_instances_proceed() {
        let step = instances_step(Ok(vec!["primary-0"]));
        assert!(matches!(step, DeleteStep::Proceed(ref v) if v == &vec!["primary-0"]));
    }

    #[test]
    fn a_zone_that_selects_no_instances_allows_deletion() {
        let step = instances_step::<&str>(Err(anyhow::anyhow!("no instances matching")));
        assert!(matches!(step, DeleteStep::AllowDeletion));
    }

    #[test]
    fn an_empty_instance_list_allows_deletion() {
        let step = instances_step::<&str>(Ok(Vec::new()));
        assert!(matches!(step, DeleteStep::AllowDeletion));
    }

    #[test]
    fn found_primaries_proceed() {
        let step = primaries_step(Ok(vec!["primary-0", "primary-1"]));
        assert!(matches!(step, DeleteStep::Proceed(ref v) if v.len() == 2));
    }

    #[test]
    fn no_primaries_allows_deletion() {
        let step = primaries_step::<&str>(Ok(Vec::new()));
        assert!(matches!(step, DeleteStep::AllowDeletion));
    }

    #[test]
    fn a_failed_primary_role_lookup_retries() {
        let step = primaries_step::<&str>(Err(anyhow::anyhow!("GET bind9instance: 503")));
        let DeleteStep::Retry(err) = step else {
            panic!("a failed role GET must retry, not drop the finalizer");
        };
        assert!(err.to_string().contains("503"));
    }
}

/// The rejection-cooldown key: one helper, used by the reconcile that notes
/// a rejected write and the finalizer that clears it (ADR-0020). Both built
/// it by hand before, one from the short type name plus "Record", the other
/// from the kind.
#[cfg(test)]
mod rejected_write_key_tests {
    use super::super::rejected_write_key;
    use crate::crd::{ARecord, PTRRecord};
    use serde_json::json;

    fn record<K: serde::de::DeserializeOwned>(kind: &str, spec: serde_json::Value) -> K {
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": kind,
            "metadata": {"name": "web", "namespace": "team"},
            "spec": spec,
        }))
        .expect("valid fixture")
    }

    #[test]
    fn key_is_kind_namespace_and_name() {
        let a: ARecord = record(
            "ARecord",
            json!({"name": "www", "ipv4Addresses": ["192.0.2.1"]}),
        );
        assert_eq!(rejected_write_key(&a), "ARecord/team/web");

        let ptr: PTRRecord = record(
            "PTRRecord",
            json!({"name": "10", "target": "www.example.com."}),
        );
        assert_eq!(rejected_write_key(&ptr), "PTRRecord/team/web");
    }
}
