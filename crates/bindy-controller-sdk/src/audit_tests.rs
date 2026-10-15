// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for the Secret audit trail (ADR-0024).

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::Layer;

    /// Every event on the audit target, as its fields.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

    struct Fields<'a>(&'a mut BTreeMap<String, String>);

    impl Visit for Fields<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.insert(
                field.name().to_string(),
                format!("{value:?}").trim_matches('"').to_string(),
            );
        }
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name().to_string(), value.to_string());
        }
    }

    impl<S: tracing::Subscriber> Layer<S> for Captured {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            if event.metadata().target() != AUDIT_TARGET {
                return;
            }
            let mut fields = BTreeMap::new();
            event.record(&mut Fields(&mut fields));
            self.0.lock().expect("lock").push(fields);
        }
    }

    fn capture<F: FnOnce()>(f: F) -> Vec<BTreeMap<String, String>> {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        tracing::subscriber::with_default(subscriber, f);
        let events = captured.0.lock().expect("lock").clone();
        events
    }

    fn audit() -> SecretAudit {
        SecretAudit {
            operation: SecretOperation::Replace,
            purpose: SecretPurpose::RndcKeyMirror,
            reason: AuditReason::MirrorSync,
            actor: Actor::Bind9Instance,
            namespace: "dns".to_string(),
            name: "primary-0-rndc-key".to_string(),
            owner: Some("Bind9Instance dns/primary-0".to_string()),
        }
    }

    fn api_error(code: u16, reason: &str) -> kube::Error {
        kube::Error::Api(
            kube::core::Status::failure(
                "secrets \"primary-0-rndc-key\" is forbidden: c2VjcmV0LWtleQ==",
                reason,
            )
            .with_code(code)
            .boxed(),
        )
    }

    #[test]
    fn a_success_emits_one_event_with_every_field() {
        let events = capture(|| audit().record::<()>(&Ok(())));
        assert_eq!(events.len(), 1, "{events:?}");
        let e = &events[0];
        for (k, v) in [
            ("kind", "audit"),
            ("resource", "Secret"),
            ("operation", "replace"),
            ("outcome", "success"),
            ("namespace", "dns"),
            ("name", "primary-0-rndc-key"),
            ("purpose", "rndc-key-mirror"),
            ("reason", "mirror-sync"),
            ("actor", "bind9instance"),
            ("owner", "Bind9Instance dns/primary-0"),
        ] {
            assert_eq!(e.get(k).map(String::as_str), Some(v), "field {k} in {e:?}");
        }
        assert!(!e.contains_key("error"), "{e:?}");
    }

    #[test]
    fn a_failure_records_the_api_reason_and_code_but_never_the_message() {
        let events = capture(|| audit().record::<()>(&Err(api_error(403, "Forbidden"))));
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.get("outcome").map(String::as_str), Some("failure"));
        assert_eq!(e.get("error").map(String::as_str), Some("403 Forbidden"));
        assert!(
            !e.values().any(|v| v.contains("c2VjcmV0")),
            "the API message must never be logged: {e:?}"
        );
    }

    #[test]
    fn a_non_api_failure_is_summarised_by_its_category() {
        let err = kube::Error::Service("connection refused to https://10.0.0.1".into());
        assert_eq!(kube_error_summary(&err), "service error");
    }

    #[tokio::test]
    async fn audited_returns_the_result_and_records_it() {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);

        let ok: Result<u8, kube::Error> = audited(audit(), async { Ok(7) }).await;
        assert_eq!(ok.expect("ok"), 7);
        let err: Result<u8, kube::Error> =
            audited(audit(), async { Err(api_error(404, "NotFound")) }).await;
        assert!(err.is_err());

        let events = captured.0.lock().expect("lock").clone();
        let outcomes: Vec<&str> = events
            .iter()
            .filter_map(|e| e.get("outcome").map(String::as_str))
            .collect();
        assert_eq!(outcomes, vec!["success", "failure"]);
    }

    #[test]
    fn every_event_is_counted_by_operation_purpose_and_outcome() {
        let counter = || {
            crate::metrics::SECRET_OPERATIONS_TOTAL
                .with_label_values(&["delete", "rndc-key", "failure"])
                .get()
        };
        let before = counter();
        let mut a = audit();
        a.operation = SecretOperation::Delete;
        a.purpose = SecretPurpose::RndcKey;
        capture(|| a.record::<()>(&Err(api_error(500, "InternalError"))));
        assert!((counter() - before - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_audit_target_is_added_unless_rust_log_names_it() {
        assert_eq!(audit_log_filter(None), "info,bindy::audit=info");
        assert_eq!(audit_log_filter(Some("")), "info,bindy::audit=info");
        assert_eq!(audit_log_filter(Some("warn")), "warn,bindy::audit=info");
        assert_eq!(
            audit_log_filter(Some("debug,kube=info")),
            "debug,kube=info,bindy::audit=info"
        );
        // An explicit choice, including turning it off, is kept as given.
        assert_eq!(
            audit_log_filter(Some("warn,bindy::audit=off")),
            "warn,bindy::audit=off"
        );
    }

    #[test]
    fn the_closed_sets_render_as_kebab_case() {
        assert_eq!(SecretOperation::Read.as_str(), "read");
        assert_eq!(SecretPurpose::ScoutKubeconfig.as_str(), "scout-kubeconfig");
        assert_eq!(
            SecretPurpose::ServiceAccountToken.as_str(),
            "serviceaccount-token"
        );
        assert_eq!(
            AuditReason::RecreateAlgorithm.as_str(),
            "recreate-algorithm"
        );
        assert_eq!(AuditReason::Revoke.as_str(), "revoke");
        assert_eq!(Actor::Bind9.as_str(), "bindy-bind9");
    }

    // -----------------------------------------------------------------
    // SecretApi: the only way to touch a Secret's data
    // -----------------------------------------------------------------

    use crate::test_api::{requests, scripted_client, status_body, HTTP_OK};
    use k8s_openapi::api::core::v1::Secret;

    const HTTP_NOT_FOUND: u16 = 404;
    const HTTP_CONFLICT: u16 = 409;

    fn secret_json(name: &str) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "v1", "kind": "Secret",
            "metadata": {"name": name, "namespace": "dns", "resourceVersion": "7"},
            "data": {"secret": "c2VjcmV0"},
        })
    }

    fn mirror_use() -> SecretUse {
        SecretUse::new(SecretPurpose::RndcKeyMirror, AuditReason::MirrorSync)
            .owned_by("Bind9Instance dns/primary-0")
    }

    /// Runs `f` with the audit events captured, on a current-thread runtime.
    fn capture_async<F, Fut>(f: F) -> Vec<BTreeMap<String, String>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(f());
        let events = captured.0.lock().expect("lock").clone();
        events
    }

    #[test]
    fn a_read_of_secret_data_is_audited() {
        let events = capture_async(|| async {
            let (client, _) = scripted_client(vec![(HTTP_OK, secret_json("bindy-src"))]);
            let api = SecretApi::namespaced(client, "dns", Actor::Bind9Instance);
            let s = api
                .get(
                    "bindy-src",
                    &SecretUse::new(SecretPurpose::RndcKeySource, AuditReason::MirrorSync),
                )
                .await
                .expect("get");
            assert_eq!(s.metadata.name.as_deref(), Some("bindy-src"));
        });
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].get("operation").map(String::as_str), Some("read"));
        assert_eq!(
            events[0].get("purpose").map(String::as_str),
            Some("rndc-key-source")
        );
        assert_eq!(events[0].get("name").map(String::as_str), Some("bindy-src"));
    }

    #[test]
    fn a_read_that_finds_nothing_accesses_no_key_and_records_nothing() {
        let events = capture_async(|| async {
            let (client, _) = scripted_client(vec![(
                HTTP_NOT_FOUND,
                status_body(HTTP_NOT_FOUND, "NotFound"),
            )]);
            let api = SecretApi::namespaced(client, "dns", Actor::Bind9Instance);
            assert!(api
                .get_opt("absent", &mirror_use())
                .await
                .expect("get_opt")
                .is_none());
        });
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_metadata_read_records_nothing() {
        let events = capture_async(|| async {
            let (client, seen) = scripted_client(vec![(
                HTTP_OK,
                serde_json::json!({
                    "apiVersion": "meta.k8s.io/v1", "kind": "PartialObjectMetadata",
                    "metadata": {"name": "primary-0-rndc-key", "namespace": "dns", "resourceVersion": "9"},
                }),
            )]);
            let api = SecretApi::namespaced(client, "dns", Actor::Bind9Instance);
            let meta = api
                .get_metadata_opt("primary-0-rndc-key")
                .await
                .expect("metadata")
                .expect("present");
            assert_eq!(meta.metadata.resource_version.as_deref(), Some("9"));
            assert_eq!(requests(&seen)[0].method, "GET");
        });
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_failed_write_is_audited_as_a_failure() {
        let events = capture_async(|| async {
            let (client, _) = scripted_client(vec![(
                HTTP_CONFLICT,
                status_body(HTTP_CONFLICT, "AlreadyExists"),
            )]);
            let api = SecretApi::namespaced(client, "dns", Actor::Bind9Instance);
            let secret: Secret =
                serde_json::from_value(secret_json("primary-0-rndc-key")).expect("secret");
            assert!(api.create(&secret, &mirror_use()).await.is_err());
        });
        assert_eq!(events.len(), 1, "{events:?}");
        let e = &events[0];
        assert_eq!(e.get("operation").map(String::as_str), Some("create"));
        assert_eq!(e.get("outcome").map(String::as_str), Some("failure"));
        assert_eq!(
            e.get("error").map(String::as_str),
            Some("409 AlreadyExists")
        );
        assert_eq!(
            e.get("owner").map(String::as_str),
            Some("Bind9Instance dns/primary-0")
        );
    }

    #[test]
    fn replace_and_delete_are_audited_with_their_operation() {
        let events = capture_async(|| async {
            let (client, _) = scripted_client(vec![
                (HTTP_OK, secret_json("primary-0-rndc-key")),
                (HTTP_OK, secret_json("primary-0-rndc-key")),
            ]);
            let api = SecretApi::namespaced(client, "dns", Actor::Bind9Instance);
            let secret: Secret =
                serde_json::from_value(secret_json("primary-0-rndc-key")).expect("secret");
            api.replace("primary-0-rndc-key", &secret, &mirror_use())
                .await
                .expect("replace");
            api.delete(
                "primary-0-rndc-key",
                &SecretUse::new(SecretPurpose::RndcKey, AuditReason::InstanceDeleted),
            )
            .await
            .expect("delete");
        });
        let ops: Vec<&str> = events
            .iter()
            .filter_map(|e| e.get("operation").map(String::as_str))
            .collect();
        assert_eq!(ops, vec!["replace", "delete"]);
    }

    /// ADR-0024 Decision 2: no other non-test code constructs an
    /// `Api<Secret>`, so no Secret operation can skip the audit wrapper.
    #[test]
    fn no_code_outside_the_audit_module_builds_a_secret_api() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates dir");
        // The one allowed exception: the instance controller's metadata
        // watch, which never receives Secret data.
        let allowed = ["bindy-controller-instance/src/watch.rs"];
        let mut offenders = Vec::new();
        let mut stack = vec![crates.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read dir") {
                let path = entry.expect("entry").path();
                let rel = path
                    .strip_prefix(crates)
                    .expect("under crates")
                    .to_string_lossy()
                    .to_string();
                if path.is_dir() {
                    if !rel.ends_with("target") && !rel.contains("/tests") {
                        stack.push(path);
                    }
                    continue;
                }
                let is_source = rel.ends_with(".rs") && !rel.ends_with("_tests.rs");
                if !is_source
                    || rel.ends_with("bindy-controller-sdk/src/audit.rs")
                    || allowed.contains(&rel.as_str())
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("read");
                if text.contains("Api<Secret>")
                    || text.contains("Api::<Secret>")
                    || text.contains("::<Secret>(")
                {
                    offenders.push(rel);
                }
            }
        }
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "Secret API built outside bindy_controller_sdk::audit::SecretApi: {offenders:?}"
        );
    }
}
