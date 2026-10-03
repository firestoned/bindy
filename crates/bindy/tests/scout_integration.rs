// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Integration tests for Scout's stale-cluster ARecord cleanup selectors.
//!
//! The unit tests in `src/scout_tests.rs` assert the *text* of the selectors
//! Scout builds, against a `wiremock` server that echoes whatever it is given.
//! They cannot assert the thing that actually matters: that a real Kubernetes
//! API server, evaluating that text, returns exactly the records Scout intends
//! to delete and no others.
//!
//! That gap is what issue #474 lived in — the selector was well-formed and
//! matched more records than intended, so a cross-cluster live ARecord was
//! deleted on every reconcile. These tests close it by planting records with
//! the labels Scout writes, then asking a live API server to evaluate the
//! selectors and checking what comes back.
//!
//! Run with: cargo test --test scout_integration -- --ignored

use bindy::crd::{ARecord, ARecordSpec};
use bindy::scout::{
    arecord_label_selector, stale_arecord_label_selector, stale_httproute_arecord_label_selector,
    stale_tcproute_arecord_label_selector, stale_tlsroute_arecord_label_selector,
};
use futures::FutureExt;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, DeleteParams, ListParams, Patch, PatchParams, PostParams};
use kube::client::Client;
use kube::ResourceExt;
use std::collections::BTreeMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};

const TEST_NAMESPACE: &str = "scout-selector-it";

/// How long `cleanup` waits for deletes to actually complete. The operator
/// running in the e2e cluster holds a finalizer on every ARecord it has
/// reconciled, so a record is gone only after the operator's Cleanup pass —
/// `delete_collection` alone merely marks it.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);
/// After this long, assume a finalizer is wedged (the operator retries a
/// NotSelected record on a backoff) and strip finalizers directly. A
/// terminating object cannot gain new finalizers, so this cannot race the
/// operator into re-adding one.
const CLEANUP_STRIP_FINALIZERS_AFTER: Duration = Duration::from_secs(20);
/// Poll cadence while waiting for the namespace to empty.
const CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(500);
const SOURCE_NAMESPACE: &str = "team-checkout";
const SOURCE_NAME: &str = "web-frontend";
const ZONE_ALPHA: &str = "zone-alpha.example.internal";
const ZONE_BETA: &str = "zone-beta.example.internal";

/// Label keys Scout stamps onto every ARecord it creates. Duplicated as
/// literals rather than imported so a rename in `scout.rs` fails this test
/// loudly instead of silently re-labelling both sides in lockstep.
const LABEL_MANAGED_BY: &str = "bindy.firestoned.io/managed-by";
const LABEL_SOURCE_CLUSTER: &str = "bindy.firestoned.io/source-cluster";
const LABEL_SOURCE_NAMESPACE: &str = "bindy.firestoned.io/source-namespace";
const LABEL_SOURCE_NAME: &str = "bindy.firestoned.io/source-name";
const LABEL_ZONE: &str = "bindy.firestoned.io/zone";

async fn client_or_skip() -> Option<Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    match Client::try_default().await {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("Skipping: no Kubernetes cluster available ({e})");
            None
        }
    }
}

async fn ensure_namespace(client: &Client) {
    let api: Api<Namespace> = Api::all(client.clone());
    let ns = Namespace {
        metadata: ObjectMeta {
            name: Some(TEST_NAMESPACE.to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    // AlreadyExists is fine — suites re-run against a warm cluster.
    let _ = api.create(&PostParams::default(), &ns).await;
}

/// Creates an ARecord carrying exactly the labels Scout writes.
async fn plant_arecord(client: &Client, name: &str, source_cluster: &str, zone: &str) {
    let api: Api<ARecord> = Api::namespaced(client.clone(), TEST_NAMESPACE);
    let mut labels = BTreeMap::new();
    labels.insert(LABEL_MANAGED_BY.to_string(), "scout".to_string());
    labels.insert(LABEL_SOURCE_CLUSTER.to_string(), source_cluster.to_string());
    labels.insert(
        LABEL_SOURCE_NAMESPACE.to_string(),
        SOURCE_NAMESPACE.to_string(),
    );
    labels.insert(LABEL_SOURCE_NAME.to_string(), SOURCE_NAME.to_string());
    labels.insert(LABEL_ZONE.to_string(), zone.to_string());

    let record = ARecord {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(TEST_NAMESPACE.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        // The zone lives in the label, not the spec — that is precisely why
        // the stale-cleanup selector has to match on the label.
        spec: ARecordSpec {
            name: "app".to_string(),
            ipv4_addresses: vec!["192.0.2.1".to_string()],
            ttl: None,
        },
        status: None,
    };

    let _ = api.delete(name, &DeleteParams::default()).await;
    api.create(&PostParams::default(), &record)
        .await
        .unwrap_or_else(|e| panic!("failed to plant ARecord {name}: {e}"));
}

async fn names_matching(client: &Client, selector: &str) -> Vec<String> {
    let api: Api<ARecord> = Api::namespaced(client.clone(), TEST_NAMESPACE);
    let mut names: Vec<String> = api
        .list(&ListParams::default().labels(selector))
        .await
        .expect("list with selector")
        .items
        .iter()
        .map(ResourceExt::name_any)
        .collect();
    names.sort();
    names
}

/// Deletes every ARecord in the test namespace and waits until they are
/// actually gone. The wait is the point: the operator's finalizer makes
/// deletion asynchronous, and a test that lists the namespace while the
/// previous test's records are still terminating sees them in its selector
/// results — exactly how `stale-oldsouth-alpha` leaked into the same-zone
/// canary test in CI.
async fn cleanup(client: &Client) {
    let api: Api<ARecord> = Api::namespaced(client.clone(), TEST_NAMESPACE);
    let _ = api
        .delete_collection(&DeleteParams::default(), &ListParams::default())
        .await;

    let started = Instant::now();
    let mut stripped = false;
    loop {
        let remaining = api
            .list(&ListParams::default())
            .await
            .expect("list ARecords during cleanup")
            .items;
        if remaining.is_empty() {
            return;
        }
        let names: Vec<String> = remaining.iter().map(ResourceExt::name_any).collect();
        assert!(
            started.elapsed() < CLEANUP_TIMEOUT,
            "cleanup: ARecords still present after {CLEANUP_TIMEOUT:?} \
             (finalizers wedged even after being stripped?): {names:?}"
        );
        if !stripped && started.elapsed() >= CLEANUP_STRIP_FINALIZERS_AFTER {
            strip_finalizers(&api, &remaining).await;
            stripped = true;
        }
        tokio::time::sleep(CLEANUP_POLL_INTERVAL).await;
    }
}

/// Escape hatch for records whose finalizer the operator cannot complete
/// promptly (a planted record matches no DNSZone, so its Cleanup pass can
/// end in a retry loop). Mirrors `force_clear_zone_finalizers` in
/// `tests/lib/dns_fixtures.sh`.
async fn strip_finalizers(api: &Api<ARecord>, records: &[ARecord]) {
    let patch = serde_json::json!({"metadata": {"finalizers": null}});
    for record in records {
        let _ = api
            .patch(
                &record.name_any(),
                &PatchParams::default(),
                &Patch::Merge(&patch),
            )
            .await;
    }
}

/// Runs one test body against a namespace that is verifiably empty before it
/// starts, and is emptied again afterwards EVEN IF THE BODY PANICS. Without
/// the panic guard, one failing assertion leaks its planted records for the
/// rest of the e2e run — the operator retries them forever and every later
/// suite's log dump fills with its warnings.
async fn with_clean_slate<Fut>(client: &Client, body: impl FnOnce(Client) -> Fut)
where
    Fut: Future<Output = ()>,
{
    ensure_namespace(client).await;
    cleanup(client).await;
    let result = AssertUnwindSafe(body(client.clone())).catch_unwind().await;
    cleanup(client).await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

/// The #474 regression, evaluated by a real API server rather than asserted as
/// a string: south's stale selector must match north's record when they share
/// a zone, and must NOT match it when they publish into different zones.
#[tokio::test]
#[ignore = "requires a Kubernetes cluster"]
async fn stale_selector_matches_only_same_zone_records() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    with_clean_slate(&client, |client| async move {
        // south's own live record — must never match its own stale selector.
        plant_arecord(&client, "own-south-alpha", "south", ZONE_ALPHA).await;
        // A genuine rename: the same physical Scout under its previous name,
        // publishing into the same zone. This is what cleanup should catch.
        plant_arecord(&client, "stale-oldsouth-alpha", "old-south", ZONE_ALPHA).await;
        // An unrelated cluster publishing into a DIFFERENT zone. Before #474
        // was fixed this was deleted on every reconcile.
        plant_arecord(&client, "live-north-beta", "north", ZONE_BETA).await;

        let selector =
            stale_arecord_label_selector("south", SOURCE_NAMESPACE, SOURCE_NAME, ZONE_ALPHA);
        let matched = names_matching(&client, &selector).await;

        assert_eq!(
            matched,
            vec!["stale-oldsouth-alpha".to_string()],
            "stale selector must match only the same-zone record from a prior \
             cluster name; matched {matched:?}"
        );
        assert!(
            !matched.iter().any(|n| n == "live-north-beta"),
            "a different cluster's record in a different zone must survive (#474)"
        );
        assert!(
            !matched.iter().any(|n| n == "own-south-alpha"),
            "the calling cluster's own record must never be selected as stale"
        );
    })
    .await;
}

/// The documented residual limitation, pinned so a future change that closes
/// it has to update this test deliberately rather than by accident: clusters
/// sharing a zone still match each other.
#[tokio::test]
#[ignore = "requires a Kubernetes cluster"]
async fn stale_selector_still_matches_a_different_cluster_in_the_same_zone() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    with_clean_slate(&client, |client| async move {
        plant_arecord(&client, "live-north-alpha", "north", ZONE_ALPHA).await;

        let selector =
            stale_arecord_label_selector("south", SOURCE_NAMESPACE, SOURCE_NAME, ZONE_ALPHA);
        let matched = names_matching(&client, &selector).await;

        assert_eq!(
            matched,
            vec!["live-north-alpha".to_string()],
            "KNOWN LIMITATION (documented in docs/src/guide/scout.md): clusters \
             sharing a zone still match each other's stale selector. If this now \
             fails, the instance-UID marker landed — update the docs and this test."
        );
    })
    .await;
}

/// The own-records selector must be exactly complementary to the stale one:
/// what cleanup deletes as "mine" and what it deletes as "stale" must never
/// overlap, or a delete path would race itself.
#[tokio::test]
#[ignore = "requires a Kubernetes cluster"]
async fn own_and_stale_selectors_are_disjoint() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    with_clean_slate(&client, |client| async move {
        plant_arecord(&client, "own-south-alpha", "south", ZONE_ALPHA).await;
        plant_arecord(&client, "stale-oldsouth-alpha", "old-south", ZONE_ALPHA).await;

        let own = names_matching(
            &client,
            &arecord_label_selector("south", SOURCE_NAMESPACE, SOURCE_NAME),
        )
        .await;
        let stale = names_matching(
            &client,
            &stale_arecord_label_selector("south", SOURCE_NAMESPACE, SOURCE_NAME, ZONE_ALPHA),
        )
        .await;

        assert_eq!(own, vec!["own-south-alpha".to_string()]);
        assert_eq!(stale, vec!["stale-oldsouth-alpha".to_string()]);
        assert!(
            own.iter().all(|n| !stale.contains(n)),
            "own and stale selectors must not both claim a record: own={own:?} stale={stale:?}"
        );
    })
    .await;
}

/// All four resource kinds share one selector builder, so a live API server
/// must evaluate them identically for identical inputs.
#[tokio::test]
#[ignore = "requires a Kubernetes cluster"]
async fn every_resource_kind_selector_agrees_on_a_live_api_server() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    with_clean_slate(&client, |client| async move {
        plant_arecord(&client, "stale-oldsouth-alpha", "old-south", ZONE_ALPHA).await;
        plant_arecord(&client, "live-north-beta", "north", ZONE_BETA).await;

        let expected = vec!["stale-oldsouth-alpha".to_string()];
        for selector in [
            stale_arecord_label_selector("south", SOURCE_NAMESPACE, SOURCE_NAME, ZONE_ALPHA),
            stale_httproute_arecord_label_selector(
                "south",
                SOURCE_NAMESPACE,
                SOURCE_NAME,
                ZONE_ALPHA,
            ),
            stale_tlsroute_arecord_label_selector(
                "south",
                SOURCE_NAMESPACE,
                SOURCE_NAME,
                ZONE_ALPHA,
            ),
            stale_tcproute_arecord_label_selector(
                "south",
                SOURCE_NAMESPACE,
                SOURCE_NAME,
                ZONE_ALPHA,
            ),
        ] {
            assert_eq!(
                names_matching(&client, &selector).await,
                expected,
                "selector {selector:?} disagreed with the others"
            );
        }
    })
    .await;
}

/// A zone that is a legal DNS name but too long for a label value must be
/// rejected by the API server, not silently matched. This is the 400 that
/// `resolve_usable_zone` now prevents from ever being issued.
#[tokio::test]
#[ignore = "requires a Kubernetes cluster"]
async fn an_overlong_zone_is_rejected_by_the_api_server() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    ensure_namespace(&client).await;

    // 68 characters: every DNS label is <= 63, so the DNSZone CRD's zoneName
    // pattern accepts it, but it exceeds the 63-char label-value limit.
    let overlong = "payments-gateway.team-checkout.production.eu-west-1.example.internal";
    assert!(overlong.len() > 63);

    let selector = stale_arecord_label_selector("south", SOURCE_NAMESPACE, SOURCE_NAME, overlong);
    let api: Api<ARecord> = Api::namespaced(client.clone(), TEST_NAMESPACE);
    let result = api.list(&ListParams::default().labels(&selector)).await;

    assert!(
        result.is_err(),
        "an over-long zone must make the API server reject the selector — \
         this is the request Scout now refuses to send"
    );
}
