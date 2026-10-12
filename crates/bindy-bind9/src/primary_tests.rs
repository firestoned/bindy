// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `primary.rs`

#[cfg(test)]
mod tests {
    use crate::crd::InstanceReference;

    /// Helper to create test instance references
    fn create_instance_ref(name: &str, namespace: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            last_reconciled_at: None,
        }
    }

    #[test]
    fn test_instance_reference_identity() {
        let ref1 = create_instance_ref("instance-1", "default");
        let ref2 = create_instance_ref("instance-1", "default");
        let ref3 = create_instance_ref("instance-2", "default");

        assert_eq!(ref1, ref2);
        assert_ne!(ref1, ref3);
    }
}

/// Primary-role lookups from the `Bind9Instance` reflector store (ADR-0015):
/// the record write path no longer GETs every instance to learn its role.
#[cfg(test)]
mod store_role_tests {
    use crate::crd::{Bind9Instance, InstanceReference, ServerRole};
    use crate::primary::{instance_role_in_store, try_filter_instances_by_role_cached};
    use kube::runtime::{reflector, watcher};
    use serde_json::json;

    fn instance(name: &str, role: &str) -> Bind9Instance {
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": {"name": name, "namespace": "dns"},
            "spec": {"clusterRef": "c", "role": role},
        }))
        .expect("valid Bind9Instance fixture")
    }

    fn reference(name: &str, namespace: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            last_reconciled_at: None,
        }
    }

    fn store(instances: Vec<Bind9Instance>) -> crate::context::MultiStore<Bind9Instance> {
        let (store, mut writer) = reflector::store();
        writer.apply_watcher_event(&watcher::Event::Init);
        for i in instances {
            writer.apply_watcher_event(&watcher::Event::InitApply(i));
        }
        writer.apply_watcher_event(&watcher::Event::InitDone);
        crate::context::MultiStore::new(vec![store])
    }

    #[test]
    fn primary_instance_in_store_is_primary() {
        let store = store(vec![instance("p0", "primary"), instance("s0", "secondary")]);

        assert_eq!(
            instance_role_in_store(&store, &reference("p0", "dns")),
            Some(ServerRole::Primary)
        );
    }

    #[test]
    fn secondary_instance_in_store_is_not_primary() {
        let store = store(vec![instance("p0", "primary"), instance("s0", "secondary")]);

        assert_eq!(
            instance_role_in_store(&store, &reference("s0", "dns")),
            Some(ServerRole::Secondary)
        );
    }

    #[test]
    fn instance_missing_from_store_is_unknown() {
        let store = store(vec![instance("p0", "primary")]);

        assert_eq!(
            instance_role_in_store(&store, &reference("p1", "dns")),
            None
        );
        assert_eq!(
            instance_role_in_store(&store, &reference("p0", "other")),
            None
        );
    }

    #[test]
    fn the_role_of_a_cached_instance_comes_from_the_store() {
        let store = store(vec![instance("p0", "primary"), instance("s0", "secondary")]);

        assert_eq!(
            instance_role_in_store(&store, &reference("s0", "dns")),
            Some(ServerRole::Secondary)
        );
        assert_eq!(
            instance_role_in_store(&store, &reference("x0", "dns")),
            None
        );
    }

    /// A client that cannot reach any API server: if the filter tried a GET
    /// for a cached instance, that instance would be skipped and the
    /// assertions below would fail (ADR-0016 decision 6).
    fn unreachable_client() -> kube::Client {
        let config =
            kube::Config::new("http://127.0.0.1:9".parse().expect("valid unreachable URL"));
        kube::Client::try_from(config).expect("client builds without connecting")
    }

    #[tokio::test]
    async fn cached_instances_are_filtered_by_role_without_an_api_call() {
        let store = store(vec![
            instance("p0", "primary"),
            instance("s0", "secondary"),
            instance("s1", "secondary"),
        ]);
        let refs = vec![
            reference("p0", "dns"),
            reference("s0", "dns"),
            reference("s1", "dns"),
        ];
        let client = unreachable_client();

        let secondaries =
            try_filter_instances_by_role_cached(&client, &store, &refs, &ServerRole::Secondary)
                .await
                .expect("cached instances need no API call");
        let primaries =
            try_filter_instances_by_role_cached(&client, &store, &refs, &ServerRole::Primary)
                .await
                .expect("cached instances need no API call");

        assert_eq!(
            secondaries,
            vec![reference("s0", "dns"), reference("s1", "dns")]
        );
        assert_eq!(primaries, vec![reference("p0", "dns")]);
    }

    /// The fallible form must not drop an instance it could not read: a
    /// transport failure is an error, not "not a primary".
    #[tokio::test]
    async fn try_filter_propagates_an_unreadable_uncached_instance() {
        let store = store(vec![instance("p0", "primary")]);
        let refs = vec![reference("p0", "dns"), reference("gone", "dns")];

        let result = try_filter_instances_by_role_cached(
            &unreachable_client(),
            &store,
            &refs,
            &ServerRole::Primary,
        )
        .await;

        assert!(
            result.is_err(),
            "an unreadable instance must fail the filter, got {result:?}"
        );
    }

    #[tokio::test]
    async fn filter_primary_instances_cached_propagates_an_unreadable_instance() {
        let store = store(vec![instance("p0", "primary")]);
        let refs = vec![reference("p0", "dns"), reference("gone", "dns")];

        let result =
            crate::primary::filter_primary_instances_cached(&unreachable_client(), &store, &refs)
                .await;

        assert!(
            result.is_err(),
            "a primary that cannot be read must not silently drop out: {result:?}"
        );
    }
}

/// Classification of a failed instance GET (bug B9): only 404 means "the
/// instance is gone, skip it"; every other failure must propagate so a
/// primary never silently drops out of a write's target set.
#[cfg(test)]
mod instance_get_error_tests {
    use crate::crd::{Bind9Instance, InstanceReference, ServerRole};
    use crate::primary::{filter_primary_instances, role_from_get_result};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;
    const HTTP_TOO_MANY_REQUESTS: u16 = 429;
    const INSTANCE_PATH_PREFIX: &str =
        "/apis/bindy.firestoned.io/v1beta1/namespaces/dns/bind9instances";

    fn reference(name: &str) -> InstanceReference {
        InstanceReference {
            api_version: "bindy.firestoned.io/v1beta1".to_string(),
            kind: "Bind9Instance".to_string(),
            name: name.to_string(),
            namespace: "dns".to_string(),
            last_reconciled_at: None,
        }
    }

    fn instance_json(name: &str, role: &str) -> serde_json::Value {
        json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": {"name": name, "namespace": "dns"},
            "spec": {"clusterRef": "c", "role": role},
        })
    }

    fn api_error(code: u16) -> kube::Error {
        kube::Error::Api(
            kube::core::Status::failure("test error", "TestReason")
                .with_code(code)
                .boxed(),
        )
    }

    fn status_json(code: u16, reason: &str) -> serde_json::Value {
        json!({
            "apiVersion": "v1",
            "kind": "Status",
            "metadata": {},
            "status": "Failure",
            "message": "test",
            "reason": reason,
            "code": code,
        })
    }

    fn client_for(server: &MockServer) -> kube::Client {
        let config = kube::Config::new(server.uri().parse().expect("mock server uri"));
        kube::Client::try_from(config).expect("client from mock config")
    }

    #[test]
    fn a_readable_instance_yields_its_role() {
        let instance: Bind9Instance =
            serde_json::from_value(instance_json("p0", "primary")).expect("valid fixture");
        let role = role_from_get_result(Ok(instance), &reference("p0")).expect("no error");
        assert_eq!(role, Some(ServerRole::Primary));
    }

    #[test]
    fn a_not_found_instance_is_skipped() {
        let role = role_from_get_result(
            Err(api_error(bindy_api::constants::HTTP_NOT_FOUND)),
            &reference("gone"),
        )
        .expect("404 is not an error");
        assert_eq!(role, None);
    }

    #[test]
    fn a_server_error_propagates() {
        for code in [HTTP_INTERNAL_SERVER_ERROR, HTTP_TOO_MANY_REQUESTS] {
            let result = role_from_get_result(Err(api_error(code)), &reference("p0"));
            assert!(result.is_err(), "HTTP {code} must propagate");
        }
    }

    #[tokio::test]
    async fn filter_primary_instances_skips_a_deleted_instance() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("{INSTANCE_PATH_PREFIX}/p0")))
            .respond_with(ResponseTemplate::new(200).set_body_json(instance_json("p0", "primary")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{INSTANCE_PATH_PREFIX}/gone")))
            .respond_with(
                ResponseTemplate::new(bindy_api::constants::HTTP_NOT_FOUND).set_body_json(
                    status_json(bindy_api::constants::HTTP_NOT_FOUND, "NotFound"),
                ),
            )
            .mount(&server)
            .await;

        let primaries =
            filter_primary_instances(&client_for(&server), &[reference("p0"), reference("gone")])
                .await
                .expect("a deleted instance is skipped, not an error");

        assert_eq!(primaries, vec![reference("p0")]);
    }

    #[tokio::test]
    async fn filter_primary_instances_propagates_a_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("{INSTANCE_PATH_PREFIX}/p0")))
            .respond_with(ResponseTemplate::new(200).set_body_json(instance_json("p0", "primary")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{INSTANCE_PATH_PREFIX}/p1")))
            .respond_with(
                ResponseTemplate::new(HTTP_INTERNAL_SERVER_ERROR)
                    .set_body_json(status_json(HTTP_INTERNAL_SERVER_ERROR, "InternalError")),
            )
            .mount(&server)
            .await;

        let result =
            filter_primary_instances(&client_for(&server), &[reference("p0"), reference("p1")])
                .await;

        assert!(
            result.is_err(),
            "a primary that returned 500 must not silently drop out: {result:?}"
        );
    }
}
