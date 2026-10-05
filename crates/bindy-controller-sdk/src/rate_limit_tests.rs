// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::metrics::{KUBE_API_RATE_LIMIT_HITS_TOTAL, KUBE_API_REQUESTS_TOTAL};
    use bindy_api::constants::{KUBE_CLIENT_BURST, KUBE_CLIENT_QPS};
    use http::{Request, Response, StatusCode};
    use std::convert::Infallible;
    use std::time::Duration;
    use tower::{Layer, ServiceExt};

    // ------------------------------------------------------------------
    // RateLimitConfig::from_values
    // ------------------------------------------------------------------

    #[test]
    fn test_from_values_defaults_when_unset() {
        let config = RateLimitConfig::from_values(None, None);
        assert!((config.qps - KUBE_CLIENT_QPS).abs() < f32::EPSILON);
        assert_eq!(config.burst, KUBE_CLIENT_BURST);
    }

    #[test]
    fn test_from_values_parses_overrides() {
        let config = RateLimitConfig::from_values(Some("50.0"), Some("100"));
        assert!((config.qps - 50.0).abs() < f32::EPSILON);
        assert_eq!(config.burst, 100);
    }

    #[test]
    fn test_from_values_rejects_non_numeric() {
        let config = RateLimitConfig::from_values(Some("fast"), Some("lots"));
        assert!((config.qps - KUBE_CLIENT_QPS).abs() < f32::EPSILON);
        assert_eq!(config.burst, KUBE_CLIENT_BURST);
    }

    #[test]
    fn test_from_values_rejects_zero_and_negative_qps() {
        let zero = RateLimitConfig::from_values(Some("0"), None);
        assert!((zero.qps - KUBE_CLIENT_QPS).abs() < f32::EPSILON);

        let negative = RateLimitConfig::from_values(Some("-5.0"), None);
        assert!((negative.qps - KUBE_CLIENT_QPS).abs() < f32::EPSILON);
    }

    #[test]
    fn test_from_values_rejects_zero_burst() {
        let config = RateLimitConfig::from_values(None, Some("0"));
        assert_eq!(config.burst, KUBE_CLIENT_BURST);
    }

    #[test]
    fn test_from_values_rejects_non_finite_qps() {
        let config = RateLimitConfig::from_values(Some("inf"), None);
        assert!((config.qps - KUBE_CLIENT_QPS).abs() < f32::EPSILON);
    }

    #[test]
    fn test_default_matches_constants() {
        let config = RateLimitConfig::default();
        assert!((config.qps - KUBE_CLIENT_QPS).abs() < f32::EPSILON);
        assert_eq!(config.burst, KUBE_CLIENT_BURST);
    }

    // ------------------------------------------------------------------
    // RateLimitConfig::period — the token-bucket window
    // ------------------------------------------------------------------

    #[test]
    fn test_period_is_burst_over_qps() {
        // 30 requests per 1.5s window == sustained 20 QPS with bursts of 30
        let config = RateLimitConfig {
            qps: 20.0,
            burst: 30,
        };
        assert_eq!(config.period(), Duration::from_millis(1500));
    }

    #[test]
    fn test_period_one_to_one() {
        let config = RateLimitConfig {
            qps: 10.0,
            burst: 10,
        };
        assert_eq!(config.period(), Duration::from_secs(1));
    }

    #[test]
    fn test_period_default_config() {
        // Defaults: 30 / 20.0 = 1.5s
        let config = RateLimitConfig::default();
        assert_eq!(config.period(), Duration::from_millis(1500));
    }

    // ------------------------------------------------------------------
    // resource_from_path — metric label extraction
    // ------------------------------------------------------------------

    #[test]
    fn test_resource_from_path_core_namespaced() {
        assert_eq!(
            resource_from_path("/api/v1/namespaces/default/pods"),
            "pods"
        );
        assert_eq!(
            resource_from_path("/api/v1/namespaces/default/pods/my-pod"),
            "pods"
        );
    }

    #[test]
    fn test_resource_from_path_core_cluster_scoped() {
        assert_eq!(resource_from_path("/api/v1/nodes"), "nodes");
        assert_eq!(resource_from_path("/api/v1/namespaces"), "namespaces");
        assert_eq!(
            resource_from_path("/api/v1/namespaces/default"),
            "namespaces"
        );
    }

    #[test]
    fn test_resource_from_path_custom_resource() {
        assert_eq!(
            resource_from_path("/apis/bindy.firestoned.io/v1beta1/namespaces/tenant-a/dnszones"),
            "dnszones"
        );
        assert_eq!(
            resource_from_path(
                "/apis/bindy.firestoned.io/v1beta1/namespaces/tenant-a/dnszones/example-com/status"
            ),
            "dnszones"
        );
    }

    #[test]
    fn test_resource_from_path_group_all_namespaces() {
        assert_eq!(
            resource_from_path("/apis/bindy.firestoned.io/v1beta1/dnszones"),
            "dnszones"
        );
    }

    #[test]
    fn test_resource_from_path_non_resource_urls() {
        assert_eq!(resource_from_path("/version"), "other");
        assert_eq!(resource_from_path("/openapi/v2"), "other");
        assert_eq!(resource_from_path("/api"), "other");
        assert_eq!(resource_from_path("/api/v1"), "other");
        assert_eq!(resource_from_path("/apis"), "other");
        assert_eq!(resource_from_path("/"), "other");
    }

    // ------------------------------------------------------------------
    // KubeApiMetricsLayer — request accounting middleware
    // ------------------------------------------------------------------

    /// Build a request against a unique resource plural so counters from other
    /// tests (the registry is global) cannot interfere with assertions.
    fn request_for(resource: &str) -> Request<()> {
        Request::builder()
            .method("GET")
            .uri(format!(
                "https://kube/apis/bindy.firestoned.io/v1beta1/namespaces/x/{resource}"
            ))
            .body(())
            .unwrap()
    }

    #[tokio::test]
    async fn test_metrics_layer_counts_successful_requests() {
        let service = tower::service_fn(|_req: Request<()>| async {
            Ok::<_, Infallible>(Response::builder().status(StatusCode::OK).body(()).unwrap())
        });
        let mut svc = KubeApiMetricsLayer.layer(service);

        let before = KUBE_API_REQUESTS_TOTAL
            .with_label_values(&["mlsuccesses", "get", "success"])
            .get();

        let response = (&mut svc)
            .oneshot(request_for("mlsuccesses"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let after = KUBE_API_REQUESTS_TOTAL
            .with_label_values(&["mlsuccesses", "get", "success"])
            .get();
        assert!((after - before - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_metrics_layer_counts_rate_limit_hits() {
        let service = tower::service_fn(|_req: Request<()>| async {
            Ok::<_, Infallible>(
                Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(())
                    .unwrap(),
            )
        });
        let mut svc = KubeApiMetricsLayer.layer(service);

        let hits_before = KUBE_API_RATE_LIMIT_HITS_TOTAL
            .with_label_values(&["mlthrottled", "get"])
            .get();
        let errors_before = KUBE_API_REQUESTS_TOTAL
            .with_label_values(&["mlthrottled", "get", "error"])
            .get();

        let response = (&mut svc)
            .oneshot(request_for("mlthrottled"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let hits_after = KUBE_API_RATE_LIMIT_HITS_TOTAL
            .with_label_values(&["mlthrottled", "get"])
            .get();
        let errors_after = KUBE_API_REQUESTS_TOTAL
            .with_label_values(&["mlthrottled", "get", "error"])
            .get();
        assert!((hits_after - hits_before - 1.0).abs() < f64::EPSILON);
        assert!((errors_after - errors_before - 1.0).abs() < f64::EPSILON);
    }

    // ------------------------------------------------------------------
    // build_rate_limited_client
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_build_rate_limited_client_constructs() {
        // Client construction is lazy: no connection is made until a request
        // is issued, so a dummy endpoint proves the middleware stack builds.
        let kube_config = kube::Config::new("http://127.0.0.1:9".parse().unwrap());
        let limits = RateLimitConfig::default();
        let client = build_rate_limited_client(
            kube_config,
            &limits,
            crate::request_timeout::request_timeout_from_value(None),
        );
        assert!(client.is_ok());
    }
}
