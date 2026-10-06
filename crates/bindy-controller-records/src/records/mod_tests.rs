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
