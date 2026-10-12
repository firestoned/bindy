// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `resources.rs`

#[cfg(test)]
mod tests {
    use super::super::create_or_apply;
    use k8s_openapi::api::core::v1::ServiceAccount;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::Client;
    use std::sync::{Arc, Mutex};

    const TEST_NAMESPACE: &str = "test-namespace";
    const TEST_NAME: &str = "test-resource";
    const FIELD_MANAGER: &str = "test-controller";

    /// Requests the mock API server saw, as (method, path).
    type Seen = Arc<Mutex<Vec<(String, String)>>>;

    fn service_account(name: Option<&str>) -> ServiceAccount {
        ServiceAccount {
            metadata: ObjectMeta {
                name: name.map(str::to_string),
                namespace: Some(TEST_NAMESPACE.to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// A client whose API server answers GET with 200 (the object exists) or
    /// 404 (it does not), answers every write with the object, and records
    /// what it was asked.
    fn recording_client(exists: bool) -> (Client, Seen) {
        let seen: Seen = Arc::default();
        let log = seen.clone();
        let object = serde_json::to_vec(&service_account(Some(TEST_NAME))).expect("serializes");
        let not_found = serde_json::to_vec(&serde_json::json!({
            "kind": "Status",
            "apiVersion": "v1",
            "status": "Failure",
            "reason": "NotFound",
            "code": http::StatusCode::NOT_FOUND.as_u16(),
        }))
        .expect("serializes");
        let service = tower::service_fn(move |req: http::Request<kube::client::Body>| {
            let log = log.clone();
            let is_get = req.method() == http::Method::GET;
            let (status, body) = if is_get && !exists {
                (http::StatusCode::NOT_FOUND, not_found.clone())
            } else {
                (http::StatusCode::OK, object.clone())
            };
            async move {
                log.lock()
                    .expect("lock")
                    .push((req.method().to_string(), req.uri().path().to_string()));
                Ok::<_, std::convert::Infallible>(
                    http::Response::builder()
                        .status(status)
                        .header("content-type", "application/json")
                        .body(kube::client::Body::from(body))
                        .expect("response builds"),
                )
            }
        });
        (Client::new(service, TEST_NAMESPACE), seen)
    }

    fn methods(seen: &Seen) -> Vec<String> {
        seen.lock()
            .expect("lock")
            .iter()
            .map(|(method, _)| method.clone())
            .collect()
    }

    #[tokio::test]
    async fn create_or_apply_rejects_a_resource_without_a_name() {
        let (client, seen) = recording_client(false);

        let result = create_or_apply(
            &client,
            TEST_NAMESPACE,
            &service_account(None),
            FIELD_MANAGER,
        )
        .await;

        assert!(result.is_err(), "a nameless resource must be rejected");
        assert!(methods(&seen).is_empty(), "no request may be sent");
    }

    #[tokio::test]
    async fn create_or_apply_creates_when_missing() {
        let (client, seen) = recording_client(false);

        create_or_apply(
            &client,
            TEST_NAMESPACE,
            &service_account(Some(TEST_NAME)),
            FIELD_MANAGER,
        )
        .await
        .expect("create succeeds");

        assert_eq!(methods(&seen), vec!["GET", "POST"]);
    }

    #[tokio::test]
    async fn create_or_apply_applies_when_present() {
        let (client, seen) = recording_client(true);

        create_or_apply(
            &client,
            TEST_NAMESPACE,
            &service_account(Some(TEST_NAME)),
            FIELD_MANAGER,
        )
        .await
        .expect("apply succeeds");

        assert_eq!(methods(&seen), vec!["GET", "PATCH"]);
        let paths: Vec<String> = seen
            .lock()
            .expect("lock")
            .iter()
            .map(|(_, path)| path.clone())
            .collect();
        assert!(paths.iter().all(|p| p.ends_with(TEST_NAME)), "{paths:?}");
    }
}
