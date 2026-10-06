// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `watch.rs`.
//!
//! The shard is driven by a channel standing in for the API server's watch,
//! so every test controls exactly which watcher events arrive and when.

#[cfg(test)]
mod tests {
    use super::super::{label_selected_config, MultiStore, RestartBackoff, WatchSet, WatchShard};
    use crate::namespace_scope::NamespaceScope;
    use futures::{Stream, StreamExt};
    use k8s_openapi::api::core::v1::ConfigMap;
    use kube::api::ObjectMeta;
    use kube::runtime::watcher;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;

    type Event = Result<watcher::Event<ConfigMap>, watcher::Error>;

    const WAIT: Duration = Duration::from_secs(2);
    const POLL: Duration = Duration::from_millis(5);
    const FAST: RestartBackoff = RestartBackoff {
        initial: Duration::from_millis(1),
        max: Duration::from_millis(2),
    };

    fn cm(name: &str) -> ConfigMap {
        ConfigMap {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("ns".to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// A stream factory whose first stream is fed by the returned sender and
    /// whose later streams (after a restart) never yield.
    fn channel_source() -> (
        mpsc::UnboundedSender<Event>,
        impl Fn() -> std::pin::Pin<Box<dyn Stream<Item = Event> + Send>> + Send + Sync + 'static,
    ) {
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        let rx = Arc::new(Mutex::new(Some(rx)));
        let factory = move || -> std::pin::Pin<Box<dyn Stream<Item = Event> + Send>> {
            match rx.lock().unwrap().take() {
                Some(rx) => Box::pin(futures::stream::unfold(rx, |mut rx| async move {
                    rx.recv().await.map(|ev| (ev, rx))
                })),
                None => Box::pin(futures::stream::pending()),
            }
        };
        (tx, factory)
    }

    async fn wait_until(mut cond: impl FnMut() -> bool) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while !cond() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "condition not met within {WAIT:?}"
            );
            tokio::time::sleep(POLL).await;
        }
    }

    async fn next_names(
        stream: &mut (impl Stream<Item = Result<ConfigMap, watcher::Error>> + Unpin),
        n: usize,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for _ in 0..n {
            let item = tokio::time::timeout(WAIT, stream.next())
                .await
                .expect("subscriber timed out")
                .expect("subscriber ended")
                .expect("subscriber yielded an error");
            out.push(item.metadata.name.unwrap_or_default());
        }
        out
    }

    fn names(store: &kube::runtime::reflector::Store<ConfigMap>) -> Vec<String> {
        let mut v: Vec<String> = store
            .state()
            .iter()
            .map(|o| o.metadata.name.clone().unwrap_or_default())
            .collect();
        v.sort();
        v
    }

    #[tokio::test]
    async fn store_follows_init_apply_and_delete() {
        let (tx, factory) = channel_source();
        let shard = WatchShard::spawn("ConfigMap", None, |_: &ConfigMap| true, factory, FAST);

        tx.send(Ok(watcher::Event::Init)).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("a")))).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("b")))).unwrap();
        tx.send(Ok(watcher::Event::InitDone)).unwrap();
        tx.send(Ok(watcher::Event::Apply(cm("c")))).unwrap();
        tx.send(Ok(watcher::Event::Delete(cm("a")))).unwrap();

        let store = shard.store();
        wait_until(|| names(&store) == ["b", "c"]).await;
    }

    #[tokio::test]
    async fn every_subscriber_receives_applies_and_deletes() {
        let (tx, factory) = channel_source();
        let shard = WatchShard::spawn("ConfigMap", None, |_: &ConfigMap| true, factory, FAST);
        let mut first = Box::pin(shard.subscribe());
        let mut second = Box::pin(shard.subscribe());

        tx.send(Ok(watcher::Event::Init)).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("a")))).unwrap();
        tx.send(Ok(watcher::Event::InitDone)).unwrap();
        tx.send(Ok(watcher::Event::Apply(cm("b")))).unwrap();
        tx.send(Ok(watcher::Event::Delete(cm("a")))).unwrap();

        // kube's shared stores drop deletes; this fan-out must not.
        assert_eq!(next_names(&mut first, 3).await, ["a", "b", "a"]);
        assert_eq!(next_names(&mut second, 3).await, ["a", "b", "a"]);
    }

    #[tokio::test]
    async fn late_subscriber_gets_the_store_then_live_events() {
        let (tx, factory) = channel_source();
        let shard = WatchShard::spawn("ConfigMap", None, |_: &ConfigMap| true, factory, FAST);

        tx.send(Ok(watcher::Event::Init)).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("a")))).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("b")))).unwrap();
        tx.send(Ok(watcher::Event::InitDone)).unwrap();
        let store = shard.store();
        wait_until(|| names(&store) == ["a", "b"]).await;

        // A controller that starts after the cache is warm (leader election).
        let mut late = Box::pin(shard.subscribe());
        let mut replayed = next_names(&mut late, 2).await;
        replayed.sort();
        assert_eq!(replayed, ["a", "b"]);

        tx.send(Ok(watcher::Event::Apply(cm("c")))).unwrap();
        assert_eq!(next_names(&mut late, 1).await, ["c"]);
    }

    /// Long enough for the shard task to process the events already sent.
    const SETTLE: Duration = Duration::from_millis(100);

    #[tokio::test]
    async fn subscriber_joining_mid_initial_list_gets_the_objects_listed_before_it() {
        let (tx, factory) = channel_source();
        let shard = WatchShard::spawn("ConfigMap", None, |_: &ConfigMap| true, factory, FAST);

        // The initial list has started: `a` is broadcast to nobody, and the
        // reflector store keeps it buffered (invisible) until InitDone.
        tx.send(Ok(watcher::Event::Init)).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("a")))).unwrap();
        tokio::time::sleep(SETTLE).await;

        // A controller subscribes now (Scout at startup, a controller after
        // leader election): `a` is neither live nor in the store yet.
        let mut sub = Box::pin(shard.subscribe());

        tx.send(Ok(watcher::Event::InitDone)).unwrap();
        tx.send(Ok(watcher::Event::Apply(cm("b")))).unwrap();

        let mut seen = next_names(&mut sub, 2).await;
        seen.sort();
        assert_eq!(
            seen,
            ["a", "b"],
            "an object listed before subscribing was lost"
        );
    }

    #[tokio::test]
    async fn subscriber_joining_mid_relist_keeps_the_old_and_new_objects() {
        let (tx, factory) = channel_source();
        let shard = WatchShard::spawn("ConfigMap", None, |_: &ConfigMap| true, factory, FAST);

        tx.send(Ok(watcher::Event::Init)).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("a")))).unwrap();
        tx.send(Ok(watcher::Event::InitDone)).unwrap();
        let store = shard.store();
        wait_until(|| names(&store) == ["a"]).await;

        // A re-list (the watcher's desync recovery) is half way through.
        tx.send(Ok(watcher::Event::Init)).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("b")))).unwrap();
        tokio::time::sleep(SETTLE).await;

        let mut sub = Box::pin(shard.subscribe());
        tx.send(Ok(watcher::Event::InitApply(cm("a")))).unwrap();
        tx.send(Ok(watcher::Event::InitDone)).unwrap();

        // `a` comes from the store snapshot (and again live); `b` only from
        // the re-list in progress. Neither may be missing.
        let mut seen = next_names(&mut sub, 3).await;
        seen.sort();
        seen.dedup();
        assert_eq!(seen, ["a", "b"]);
    }

    #[tokio::test]
    async fn predicate_filters_the_store_and_the_broadcast() {
        let (tx, factory) = channel_source();
        let keep = |o: &ConfigMap| {
            o.metadata
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("keep"))
        };
        let shard = WatchShard::spawn("ConfigMap", None, keep, factory, FAST);
        let mut sub = Box::pin(shard.subscribe());

        tx.send(Ok(watcher::Event::Init)).unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("drop-1"))))
            .unwrap();
        tx.send(Ok(watcher::Event::InitApply(cm("keep-1"))))
            .unwrap();
        tx.send(Ok(watcher::Event::InitDone)).unwrap();
        tx.send(Ok(watcher::Event::Apply(cm("drop-2")))).unwrap();
        tx.send(Ok(watcher::Event::Apply(cm("keep-2")))).unwrap();

        assert_eq!(next_names(&mut sub, 2).await, ["keep-1", "keep-2"]);
        let store = shard.store();
        wait_until(|| names(&store) == ["keep-1", "keep-2"]).await;
    }

    #[tokio::test]
    async fn watch_errors_do_not_stop_the_shard() {
        let (tx, factory) = channel_source();
        let shard = WatchShard::spawn("ConfigMap", None, |_: &ConfigMap| true, factory, FAST);
        let mut sub = Box::pin(shard.subscribe());

        tx.send(Err(watcher::Error::NoResourceVersion)).unwrap();
        tx.send(Ok(watcher::Event::Apply(cm("after-error"))))
            .unwrap();

        assert_eq!(next_names(&mut sub, 1).await, ["after-error"]);
    }

    #[tokio::test]
    async fn an_ended_stream_is_restarted_and_counted() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in_factory = calls.clone();
        // Two streams end at once; the third parks.
        let factory = move || -> std::pin::Pin<Box<dyn Stream<Item = Event> + Send>> {
            if calls_in_factory.fetch_add(1, Ordering::SeqCst) < 2 {
                Box::pin(futures::stream::empty())
            } else {
                Box::pin(futures::stream::pending())
            }
        };
        let shard = WatchShard::spawn(
            "ConfigMap",
            Some("restart-test".to_string()),
            |_: &ConfigMap| true,
            factory,
            FAST,
        );

        // Two restarts, and the third stream (the one that parks) created.
        wait_until(|| shard.restarts() == 2 && calls.load(Ordering::SeqCst) == 3).await;
        tokio::time::sleep(FAST.max * 10).await;
        assert_eq!(shard.restarts(), 2, "a parked stream must not be restarted");
    }

    #[test]
    fn restart_backoff_doubles_and_caps() {
        let backoff = RestartBackoff {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(5),
        };
        assert_eq!(backoff.delay(0), Duration::from_secs(1));
        assert_eq!(backoff.delay(1), Duration::from_secs(2));
        assert_eq!(backoff.delay(2), Duration::from_secs(4));
        assert_eq!(backoff.delay(3), Duration::from_secs(5));
        assert_eq!(backoff.delay(40), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn subscribe_all_merges_every_namespace_shard() {
        // A zone in namespace "a" can be served by an instance in "b": a
        // controller for "a" must see "b"'s events too.
        let (tx_a, factory_a) = channel_source();
        let (tx_b, factory_b) = channel_source();
        let shard_a = WatchShard::spawn(
            "ConfigMap",
            Some("a".to_string()),
            |_: &ConfigMap| true,
            factory_a,
            FAST,
        );
        let shard_b = WatchShard::spawn(
            "ConfigMap",
            Some("b".to_string()),
            |_: &ConfigMap| true,
            factory_b,
            FAST,
        );
        let scope = NamespaceScope::Namespaces(vec!["a".to_string(), "b".to_string()]);
        let mut ws = WatchSet::new(unreachable_client(), scope);
        let _ = ws.insert::<ConfigMap>(
            "ConfigMap",
            vec![
                (Some("a".to_string()), shard_a),
                (Some("b".to_string()), shard_b),
            ],
        );

        let mut all = Box::pin(ws.subscribe_all::<ConfigMap>());
        tx_a.send(Ok(watcher::Event::Apply(cm("from-a")))).unwrap();
        tx_b.send(Ok(watcher::Event::Apply(cm("from-b")))).unwrap();

        let mut got = next_names(&mut all, 2).await;
        got.sort();
        assert_eq!(got, ["from-a", "from-b"]);
    }

    #[test]
    fn label_selected_config_filters_on_the_server() {
        let config = label_selected_config("app.kubernetes.io/part-of=bindy");
        assert_eq!(
            config.label_selector.as_deref(),
            Some("app.kubernetes.io/part-of=bindy")
        );
    }

    fn unreachable_client() -> kube::Client {
        // Never dialled successfully: these tests only exercise routing.
        let config = kube::Config::new("http://127.0.0.1:1".parse().unwrap());
        kube::Client::try_from(config).unwrap()
    }

    #[tokio::test]
    async fn watch_set_shards_a_namespaced_kind_per_target() {
        let scope = NamespaceScope::Namespaces(vec!["a".to_string(), "b".to_string()]);
        let mut ws = WatchSet::new(unreachable_client(), scope);

        let view: MultiStore<ConfigMap> = ws.register::<ConfigMap>("ConfigMap");

        assert_eq!(view.shard_count(), 2);
        let _store_a = ws.store::<ConfigMap>(Some("a"));
        let _sub_b = ws.subscribe::<ConfigMap>(Some("b"));
    }

    #[tokio::test]
    async fn watch_set_cluster_wide_scope_has_one_shard() {
        let mut ws = WatchSet::new(unreachable_client(), NamespaceScope::All);
        let view: MultiStore<ConfigMap> = ws.register::<ConfigMap>("ConfigMap");
        assert_eq!(view.shard_count(), 1);
        let _store = ws.store::<ConfigMap>(None);
    }

    #[tokio::test]
    #[should_panic(expected = "not registered")]
    async fn watch_set_panics_on_an_unregistered_kind() {
        let ws = WatchSet::new(unreachable_client(), NamespaceScope::All);
        let _ = ws.store::<ConfigMap>(None);
    }

    #[tokio::test]
    #[should_panic(expected = "no shard")]
    async fn watch_set_panics_on_an_unwatched_namespace() {
        let scope = NamespaceScope::Namespaces(vec!["a".to_string()]);
        let mut ws = WatchSet::new(unreachable_client(), scope);
        let _ = ws.register::<ConfigMap>("ConfigMap");
        let _ = ws.store::<ConfigMap>(Some("elsewhere"));
    }

    // ------------------------------------------------------------------
    // primary_predicate: the self-trigger filter (ADR-0009 §4, amended)
    // ------------------------------------------------------------------

    fn versioned_config_map(
        generation: i64,
        resource_version: &str,
        finalizers: &[&str],
        labels: &[(&str, &str)],
        annotations: &[(&str, &str)],
    ) -> ConfigMap {
        let map = |kv: &[(&str, &str)]| {
            (!kv.is_empty()).then(|| {
                kv.iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect()
            })
        };
        ConfigMap {
            metadata: ObjectMeta {
                name: Some("zone".to_string()),
                namespace: Some("a".to_string()),
                uid: Some("uid-1".to_string()),
                generation: Some(generation),
                resource_version: Some(resource_version.to_string()),
                finalizers: (!finalizers.is_empty())
                    .then(|| finalizers.iter().map(ToString::to_string).collect()),
                labels: map(labels),
                annotations: map(annotations),
                ..ObjectMeta::default()
            },
            ..ConfigMap::default()
        }
    }

    #[tokio::test]
    async fn primary_predicate_drops_status_only_writes_and_passes_everything_else() {
        use kube::runtime::WatchStreamExt;

        let events = vec![
            // first sight
            versioned_config_map(1, "100", &[], &[], &[]),
            // a status write: only the resourceVersion moved
            versioned_config_map(1, "101", &[], &[], &[]),
            // spec change
            versioned_config_map(2, "102", &[], &[], &[]),
            // finalizer added by kube's finalizer() helper
            versioned_config_map(2, "103", &["f"], &[], &[]),
            // label change
            versioned_config_map(2, "104", &["f"], &[("k", "v")], &[]),
            // annotation change
            versioned_config_map(2, "105", &["f"], &[("k", "v")], &[("a", "b")]),
            // another status write
            versioned_config_map(2, "106", &["f"], &[("k", "v")], &[("a", "b")]),
        ];
        let stream = futures::stream::iter(events.into_iter().map(Ok::<ConfigMap, watcher::Error>));

        let passed: Vec<String> = stream
            .predicate_filter(super::super::primary_predicate(), Default::default())
            .map(|cm| cm.unwrap().metadata.resource_version.unwrap())
            .collect()
            .await;

        assert_eq!(passed, vec!["100", "102", "103", "104", "105"]);
    }

    // ------------------------------------------------------------------
    // diagnose_watch_error: an actionable message per watch failure
    // ------------------------------------------------------------------

    fn api_error(code: u16, message: &str) -> kube::Error {
        kube::Error::Api(
            kube::core::Status::failure(message, "Reason")
                .with_code(code)
                .boxed(),
        )
    }

    #[test]
    fn a_forbidden_list_points_at_rbac() {
        let error = watcher::Error::InitialListFailed(api_error(403, "dnszones is forbidden"));
        let diagnosis = super::super::diagnose_watch_error(&error);
        assert!(diagnosis.starts_with("initial list failed"), "{diagnosis}");
        assert!(diagnosis.contains("check RBAC"), "{diagnosis}");
        assert!(diagnosis.contains("dnszones is forbidden"), "{diagnosis}");
    }

    #[test]
    fn an_unauthorized_watch_points_at_credentials() {
        let error = watcher::Error::WatchStartFailed(api_error(401, "token expired"));
        let diagnosis = super::super::diagnose_watch_error(&error);
        assert!(diagnosis.starts_with("watch start failed"), "{diagnosis}");
        assert!(diagnosis.contains("check credentials"), "{diagnosis}");
    }

    #[test]
    fn an_error_event_in_the_stream_carries_its_status() {
        let status = kube::core::Status::failure("too old resource version", "Expired")
            .with_code(410)
            .boxed();
        let diagnosis = super::super::diagnose_watch_error(&watcher::Error::WatchError(status));
        assert!(
            diagnosis.contains("too old resource version"),
            "{diagnosis}"
        );
        assert!(diagnosis.contains("HTTP 410"), "{diagnosis}");
    }

    #[test]
    fn a_kind_without_resource_versions_says_so() {
        let diagnosis = super::super::diagnose_watch_error(&watcher::Error::NoResourceVersion);
        assert!(diagnosis.contains("does not support watch"), "{diagnosis}");
    }

    // ------------------------------------------------------------------
    // changed_only: pass an object only when the part a mapper reads changes
    // ------------------------------------------------------------------

    fn labelled(name: &str, rv: &str, tier: &str) -> ConfigMap {
        ConfigMap {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("a".to_string()),
                resource_version: Some(rv.to_string()),
                labels: Some([("tier".to_string(), tier.to_string())].into()),
                ..ObjectMeta::default()
            },
            ..ConfigMap::default()
        }
    }

    fn tier_key(cm: &ConfigMap) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        cm.metadata
            .labels
            .as_ref()
            .and_then(|l| l.get("tier"))
            .hash(&mut h);
        h.finish()
    }

    async fn run_changed_only(events: Vec<ConfigMap>, cached: &[&str]) -> Vec<String> {
        let cached: Vec<String> = cached.iter().map(ToString::to_string).collect();
        let stream = futures::stream::iter(events.into_iter().map(Ok::<_, watcher::Error>));
        super::super::changed_only(stream, tier_key, move |cm: &ConfigMap| {
            cached.contains(&cm.metadata.name.clone().unwrap_or_default())
        })
        .map(|cm| {
            let cm = cm.unwrap();
            format!(
                "{}@{}",
                cm.metadata.name.unwrap(),
                cm.metadata.resource_version.unwrap()
            )
        })
        .collect()
        .await
    }

    #[tokio::test]
    async fn changed_only_passes_first_sight_and_key_changes_and_drops_the_rest() {
        let passed = run_changed_only(
            vec![
                labelled("x", "1", "edge"), // first sight
                labelled("x", "2", "edge"), // a timestamp write: same key
                labelled("x", "3", "core"), // the key changed
                labelled("y", "4", "edge"), // another object's first sight
                labelled("x", "5", "core"), // same key again
            ],
            &["x", "y"],
        )
        .await;
        assert_eq!(passed, vec!["x@1", "x@3", "y@4"]);
    }

    #[tokio::test]
    async fn changed_only_always_passes_a_deleted_object_and_forgets_it() {
        // "x" is no longer in the cache: a Delete event (the store is updated
        // before the broadcast). It must reach the mapper even though its key
        // is unchanged, and a recreated "x" is first sight again.
        let passed = run_changed_only(
            vec![
                labelled("x", "1", "edge"),
                labelled("x", "2", "edge"),
                labelled("x", "3", "edge"),
            ],
            &[],
        )
        .await;
        assert_eq!(passed, vec!["x@1", "x@2", "x@3"]);
    }

    #[tokio::test]
    async fn changed_only_passes_watch_errors_through() {
        let stream =
            futures::stream::iter(vec![Err::<ConfigMap, _>(watcher::Error::NoResourceVersion)]);
        let out: Vec<_> = super::super::changed_only(stream, tier_key, |_: &ConfigMap| true)
            .collect()
            .await;
        assert_eq!(out.len(), 1);
        assert!(out[0].is_err());
    }
}
