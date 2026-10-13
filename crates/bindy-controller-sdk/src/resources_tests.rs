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

/// `controller_owner_reference`, `delete_if_present` and
/// `create_or_force_apply` against the scripted mock API server.
#[cfg(test)]
mod helper_tests {
    use super::super::{
        controller_owner_reference, create_or_force_apply, delete_if_present, force_apply_body,
    };
    use crate::kube_errors::is_not_found_anyhow;
    use crate::test_api::{requests, scripted_client, status_body, HTTP_OK};
    use bindy_api::constants::{HTTP_CONFLICT, HTTP_NOT_FOUND};
    use bindy_api::crd::Bind9Cluster;
    use k8s_openapi::api::core::v1::ConfigMap;
    use kube::Api;

    const NAMESPACE: &str = "dns";
    const NAME: &str = "prod";
    const UID: &str = "cluster-uid";
    const FIELD_MANAGER: &str = "bindy-test";
    const HTTP_CREATED: u16 = 201;
    const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;
    const APPLY_PATCH_CONTENT_TYPE: &str = "application/apply-patch+yaml";

    /// A `Bind9Cluster` as the API server would return it.
    fn cluster(metadata: serde_json::Value) -> Bind9Cluster {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Cluster",
            "metadata": metadata,
            "spec": { "version": "9.18" },
            "status": { "instanceCount": 3 },
        }))
        .expect("test Bind9Cluster deserializes")
    }

    fn named_cluster() -> Bind9Cluster {
        cluster(serde_json::json!({
            "name": NAME,
            "namespace": NAMESPACE,
            "uid": UID,
            "labels": { "app": "bindy" },
        }))
    }

    fn cluster_json() -> serde_json::Value {
        serde_json::to_value(named_cluster()).expect("serializes")
    }

    // ------------------------------------------------ controller_owner_reference

    #[test]
    fn owner_reference_is_a_blocking_controller_reference() {
        let owner = controller_owner_reference(&named_cluster()).expect("owner ref");
        assert_eq!(owner.api_version, "bindy.firestoned.io/v1beta1");
        assert_eq!(owner.kind, "Bind9Cluster");
        assert_eq!(owner.name, NAME);
        assert_eq!(owner.uid, UID);
        assert_eq!(owner.controller, Some(true));
        assert_eq!(owner.block_owner_deletion, Some(true));
    }

    #[test]
    fn owner_reference_requires_a_uid() {
        let no_uid = cluster(serde_json::json!({ "name": NAME, "namespace": NAMESPACE }));
        assert!(controller_owner_reference(&no_uid).is_err());
    }

    #[test]
    fn owner_reference_rejects_an_empty_uid() {
        let empty_uid = cluster(serde_json::json!({
            "name": NAME, "namespace": NAMESPACE, "uid": "",
        }));
        assert!(controller_owner_reference(&empty_uid).is_err());
    }

    #[test]
    fn owner_reference_requires_a_name() {
        let no_name = cluster(serde_json::json!({ "namespace": NAMESPACE, "uid": UID }));
        assert!(controller_owner_reference(&no_name).is_err());
    }

    // ---------------------------------------------------------- delete_if_present

    #[tokio::test]
    async fn delete_if_present_reports_a_deletion() {
        let (client, seen) = scripted_client(vec![(HTTP_OK, cluster_json())]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);

        let deleted = delete_if_present(&api, NAME)
            .await
            .expect("delete succeeds");

        assert!(deleted);
        let seen = requests(&seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "DELETE");
        assert!(
            seen[0].path.ends_with("/namespaces/dns/bind9clusters/prod"),
            "{}",
            seen[0].path
        );
    }

    #[tokio::test]
    async fn delete_if_present_treats_not_found_as_already_gone() {
        let (client, _seen) = scripted_client(vec![(
            HTTP_NOT_FOUND,
            status_body(HTTP_NOT_FOUND, "NotFound"),
        )]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);

        let deleted = delete_if_present(&api, NAME)
            .await
            .expect("404 is not an error");

        assert!(!deleted);
    }

    #[tokio::test]
    async fn delete_if_present_works_for_a_cluster_wide_api() {
        let (client, seen) = scripted_client(vec![(HTTP_OK, cluster_json())]);
        let api: Api<Bind9Cluster> = Api::all(client);

        assert!(delete_if_present(&api, NAME).await.expect("deleted"));
        assert!(!requests(&seen)[0].path.contains("/namespaces/"));
    }

    #[tokio::test]
    async fn delete_if_present_propagates_other_errors_naming_the_object() {
        let (client, _seen) = scripted_client(vec![(
            HTTP_INTERNAL_SERVER_ERROR,
            status_body(HTTP_INTERNAL_SERVER_ERROR, "InternalError"),
        )]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);

        let err = delete_if_present(&api, NAME).await.expect_err("500 fails");

        let text = format!("{err:#}");
        assert!(text.contains("Bind9Cluster"), "{text}");
        assert!(text.contains(NAME), "{text}");
        assert!(!is_not_found_anyhow(&err));
        assert!(
            err.chain()
                .any(|cause| cause.downcast_ref::<kube::Error>().is_some()),
            "the kube error stays in the chain"
        );
    }

    // ----------------------------------------------------------- force_apply_body

    #[test]
    fn force_apply_body_carries_identity_metadata_and_spec_but_no_status() {
        let mut obj = named_cluster();
        obj.metadata.owner_references = Some(vec![
            controller_owner_reference(&named_cluster()).expect("owner")
        ]);
        obj.metadata.annotations = Some([("index".to_string(), "0".to_string())].into());

        let body = force_apply_body(&obj).expect("body builds");

        assert_eq!(
            body,
            serde_json::json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "Bind9Cluster",
                "metadata": {
                    "name": NAME,
                    "namespace": NAMESPACE,
                    "labels": { "app": "bindy" },
                    "annotations": { "index": "0" },
                    "ownerReferences": serde_json::to_value(&obj.metadata.owner_references)
                        .expect("serializes"),
                },
                "spec": serde_json::to_value(&obj.spec).expect("serializes"),
            })
        );
    }

    #[test]
    fn force_apply_body_keeps_owner_references_null_and_omits_absent_maps() {
        let obj = cluster(serde_json::json!({ "name": NAME, "namespace": NAMESPACE }));

        let body = force_apply_body(&obj).expect("body builds");

        let metadata = body["metadata"].as_object().expect("metadata object");
        assert_eq!(
            metadata.get("ownerReferences"),
            Some(&serde_json::Value::Null)
        );
        assert!(!metadata.contains_key("labels"));
        assert!(!metadata.contains_key("annotations"));
        assert!(!body.as_object().expect("object").contains_key("status"));
    }

    #[test]
    fn force_apply_body_of_a_cluster_scoped_object_has_no_namespace() {
        let mut map = ConfigMap::default();
        map.metadata.name = Some(NAME.to_string());
        map.data = Some([("k".to_string(), "v".to_string())].into());

        let body = force_apply_body(&map).expect("body builds");

        assert!(body["metadata"].get("namespace").is_none());
        assert_eq!(body["data"], serde_json::json!({ "k": "v" }));
        assert_eq!(body["kind"], "ConfigMap");
        assert_eq!(body["apiVersion"], "v1");
    }

    #[test]
    fn force_apply_body_requires_a_name() {
        let obj = cluster(serde_json::json!({ "namespace": NAMESPACE }));
        assert!(force_apply_body(&obj).is_err());
    }

    // ------------------------------------------------------- create_or_force_apply

    #[tokio::test]
    async fn create_or_force_apply_creates_when_absent() {
        let (client, seen) = scripted_client(vec![(HTTP_CREATED, cluster_json())]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);

        let created = create_or_force_apply(&api, &named_cluster(), FIELD_MANAGER)
            .await
            .expect("create succeeds");

        assert_eq!(created.metadata.name.as_deref(), Some(NAME));
        let seen = requests(&seen);
        assert_eq!(seen.len(), 1, "no patch after a successful create");
        assert_eq!(seen[0].method, "POST");
        assert!(seen[0].path.ends_with("/namespaces/dns/bind9clusters"));
    }

    #[tokio::test]
    async fn create_or_force_apply_force_applies_on_conflict() {
        let (client, seen) = scripted_client(vec![
            (HTTP_CONFLICT, status_body(HTTP_CONFLICT, "AlreadyExists")),
            (HTTP_OK, cluster_json()),
        ]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);
        let obj = named_cluster();

        create_or_force_apply(&api, &obj, FIELD_MANAGER)
            .await
            .expect("apply succeeds");

        let seen = requests(&seen);
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].method, "PATCH");
        assert!(seen[1].path.ends_with("/namespaces/dns/bind9clusters/prod"));
        assert_eq!(seen[1].content_type, APPLY_PATCH_CONTENT_TYPE);
        assert!(
            seen[1].query.contains("fieldManager=bindy-test"),
            "{}",
            seen[1].query
        );
        assert!(seen[1].query.contains("force=true"), "{}", seen[1].query);
        assert_eq!(seen[1].body, force_apply_body(&obj).expect("body"));
    }

    #[tokio::test]
    async fn create_or_force_apply_propagates_a_create_failure_without_patching() {
        let (client, seen) = scripted_client(vec![(
            HTTP_INTERNAL_SERVER_ERROR,
            status_body(HTTP_INTERNAL_SERVER_ERROR, "InternalError"),
        )]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);

        let err = create_or_force_apply(&api, &named_cluster(), FIELD_MANAGER)
            .await
            .expect_err("500 fails");

        assert!(format!("{err:#}").contains(NAME));
        assert_eq!(requests(&seen).len(), 1, "a failed create is not patched");
    }

    #[tokio::test]
    async fn create_or_force_apply_propagates_an_apply_failure() {
        let (client, _seen) = scripted_client(vec![
            (HTTP_CONFLICT, status_body(HTTP_CONFLICT, "AlreadyExists")),
            (
                HTTP_INTERNAL_SERVER_ERROR,
                status_body(HTTP_INTERNAL_SERVER_ERROR, "InternalError"),
            ),
        ]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);

        let result = create_or_force_apply(&api, &named_cluster(), FIELD_MANAGER).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn create_or_force_apply_rejects_a_nameless_object_without_a_request() {
        let (client, seen) = scripted_client(vec![]);
        let api: Api<Bind9Cluster> = Api::namespaced(client, NAMESPACE);
        let obj = cluster(serde_json::json!({ "namespace": NAMESPACE }));

        assert!(create_or_force_apply(&api, &obj, FIELD_MANAGER)
            .await
            .is_err());
        assert!(requests(&seen).is_empty());
    }
}
