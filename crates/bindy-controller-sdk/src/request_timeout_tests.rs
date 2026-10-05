// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit and stack tests for the non-watch request deadline (ADR-0014).

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::rate_limit::{build_rate_limited_client, RateLimitConfig};
    use bindy_api::constants::KUBE_CLIENT_REQUEST_TIMEOUT_SECS;
    use http::{Request, Response, StatusCode, Uri};
    use http_body::{Body, Frame};
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tower::{Layer, ServiceExt};

    /// Deadline used by the layer tests: short, so the suite stays fast.
    const TEST_TIMEOUT: Duration = Duration::from_millis(100);

    /// How long a test waits to prove a watch was NOT cut: several deadlines.
    const NOT_CUT_GRACE: Duration = Duration::from_millis(400);

    /// Upper bound for a test expecting the deadline to fire.
    const MUST_FIRE_WITHIN: Duration = Duration::from_secs(5);

    const NON_WATCH_URI: &str =
        "http://kube/apis/bindy.firestoned.io/v1beta1/namespaces/x/dnszones";
    const WATCH_URI: &str =
        "http://kube/apis/bindy.firestoned.io/v1beta1/dnszones?&watch=true&timeoutSeconds=290&allowWatchBookmarks=true&resourceVersion=0";

    /// A response body that never yields a frame (a stalled connection).
    #[derive(Debug)]
    struct PendingBody;

    impl Body for PendingBody {
        type Data = &'static [u8];
        type Error = Infallible;

        fn poll_frame(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            Poll::Pending
        }
    }

    /// A response body that yields one data frame, then ends.
    #[derive(Debug)]
    struct OneFrameBody(Option<&'static [u8]>);

    impl Body for OneFrameBody {
        type Data = &'static [u8];
        type Error = Infallible;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            Poll::Ready(self.0.take().map(|data| Ok(Frame::data(data))))
        }
    }

    fn get(uri: &str) -> Request<()> {
        Request::builder().method("GET").uri(uri).body(()).unwrap()
    }

    /// Poll a body for its next frame.
    async fn next_frame<B: Body + Unpin>(body: &mut B) -> Option<Result<Frame<B::Data>, B::Error>> {
        std::future::poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx)).await
    }

    fn assert_is_timeout(err: &(dyn std::error::Error + 'static)) {
        assert!(
            err.downcast_ref::<RequestTimeoutError>().is_some(),
            "expected RequestTimeoutError, got: {err}"
        );
    }

    // ------------------------------------------------------------------
    // Configuration
    // ------------------------------------------------------------------

    #[test]
    fn test_from_value_defaults_when_unset() {
        assert_eq!(
            request_timeout_from_value(None),
            Duration::from_secs(KUBE_CLIENT_REQUEST_TIMEOUT_SECS)
        );
    }

    #[test]
    fn test_from_value_parses_override() {
        assert_eq!(
            request_timeout_from_value(Some("5")),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn test_from_value_rejects_invalid_values() {
        let default = Duration::from_secs(KUBE_CLIENT_REQUEST_TIMEOUT_SECS);
        for raw in ["", "abc", "0", "-3", "1.5"] {
            assert_eq!(
                request_timeout_from_value(Some(raw)),
                default,
                "value {raw:?}"
            );
        }
    }

    #[test]
    fn test_env_var_name() {
        assert_eq!(
            ENV_KUBE_REQUEST_TIMEOUT_SECS,
            "BINDY_KUBE_REQUEST_TIMEOUT_SECS"
        );
    }

    // ------------------------------------------------------------------
    // Watch detection
    // ------------------------------------------------------------------

    #[test]
    fn test_is_watch_request_kube_rs_watch_query() {
        assert!(is_watch_request(&WATCH_URI.parse::<Uri>().unwrap()));
    }

    #[test]
    fn test_is_watch_request_accepts_true_and_one() {
        assert!(is_watch_request(
            &"/api/v1/pods?watch=true".parse::<Uri>().unwrap()
        ));
        assert!(is_watch_request(
            &"/api/v1/pods?watch=1".parse::<Uri>().unwrap()
        ));
    }

    #[test]
    fn test_is_watch_request_rejects_non_watch() {
        for uri in [
            "/api/v1/pods",
            "/api/v1/pods?watch=false",
            "/api/v1/pods?limit=100&continue=abc",
            "/api/v1/pods?labelSelector=watch%3Dtrue",
            "/api/v1/pods?watcher=true",
        ] {
            assert!(!is_watch_request(&uri.parse::<Uri>().unwrap()), "uri {uri}");
        }
    }

    // ------------------------------------------------------------------
    // Layer: header phase
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_non_watch_request_times_out_when_no_response() {
        let inner = tower::service_fn(|_req: Request<()>| {
            std::future::pending::<Result<Response<OneFrameBody>, Infallible>>()
        });
        let svc = RequestTimeoutLayer::new(TEST_TIMEOUT).layer(inner);

        let result = tokio::time::timeout(MUST_FIRE_WITHIN, svc.oneshot(get(NON_WATCH_URI)))
            .await
            .expect("the request deadline must fire before the test guard");

        let err = result.expect_err("a stalled non-watch request must fail");
        assert_is_timeout(err.as_ref());
    }

    #[tokio::test]
    async fn test_watch_request_is_not_timed_out() {
        let inner = tower::service_fn(|_req: Request<()>| {
            std::future::pending::<Result<Response<OneFrameBody>, Infallible>>()
        });
        let svc = RequestTimeoutLayer::new(TEST_TIMEOUT).layer(inner);

        let result = tokio::time::timeout(NOT_CUT_GRACE, svc.oneshot(get(WATCH_URI))).await;

        assert!(
            result.is_err(),
            "a watch must not be cut by the request deadline"
        );
    }

    #[tokio::test]
    async fn test_fast_response_passes_through() {
        let inner = tower::service_fn(|_req: Request<()>| async {
            Ok::<_, Infallible>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(OneFrameBody(Some(b"ok")))
                    .unwrap(),
            )
        });
        let svc = RequestTimeoutLayer::new(TEST_TIMEOUT).layer(inner);

        let response = svc.oneshot(get(NON_WATCH_URI)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut body = response.into_body();
        let frame = next_frame(&mut body).await.unwrap().unwrap();
        assert_eq!(frame.into_data().unwrap(), b"ok".as_slice());
        assert!(next_frame(&mut body).await.is_none());
    }

    // ------------------------------------------------------------------
    // Layer: body phase
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_non_watch_body_times_out_when_stalled() {
        let inner = tower::service_fn(|_req: Request<()>| async {
            Ok::<_, Infallible>(Response::new(PendingBody))
        });
        let svc = RequestTimeoutLayer::new(TEST_TIMEOUT).layer(inner);

        let mut body = svc.oneshot(get(NON_WATCH_URI)).await.unwrap().into_body();
        let frame = tokio::time::timeout(MUST_FIRE_WITHIN, next_frame(&mut body))
            .await
            .expect("the request deadline must fire before the test guard");

        let err = frame
            .expect("a timeout is reported as an error frame")
            .unwrap_err();
        assert_is_timeout(err.as_ref());
    }

    #[tokio::test]
    async fn test_watch_body_is_not_timed_out() {
        let inner = tower::service_fn(|_req: Request<()>| async {
            Ok::<_, Infallible>(Response::new(PendingBody))
        });
        let svc = RequestTimeoutLayer::new(TEST_TIMEOUT).layer(inner);

        let mut body = svc.oneshot(get(WATCH_URI)).await.unwrap().into_body();
        let frame = tokio::time::timeout(NOT_CUT_GRACE, next_frame(&mut body)).await;

        assert!(
            frame.is_err(),
            "a watch stream must not be cut by the request deadline"
        );
    }

    // ------------------------------------------------------------------
    // Through the real client stack, against a server that never responds
    // ------------------------------------------------------------------

    /// Bind a TCP listener that accepts connections and never answers.
    async fn silent_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        (format!("http://{addr}"), handle)
    }

    fn client_for(url: &str) -> kube::Client {
        let config = kube::Config::new(url.parse().unwrap());
        build_rate_limited_client(config, &RateLimitConfig::default(), TEST_TIMEOUT).unwrap()
    }

    fn raw_get(path: &str) -> Request<Vec<u8>> {
        Request::builder()
            .method("GET")
            .uri(path)
            .body(Vec::new())
            .unwrap()
    }

    #[tokio::test]
    async fn test_client_stack_times_out_stalled_non_watch_request() {
        let (url, server) = silent_server().await;
        let client = client_for(&url);

        let result = tokio::time::timeout(
            MUST_FIRE_WITHIN,
            client.request_text(raw_get("/api/v1/namespaces/x/configmaps/y")),
        )
        .await
        .expect("the request deadline must fire before the test guard");

        let err = result.expect_err("a stalled request must fail");
        let kube::Error::Service(source) = &err else {
            panic!("expected kube::Error::Service, got: {err:?}");
        };
        assert_is_timeout(source.as_ref());
        server.abort();
    }

    #[tokio::test]
    async fn test_client_stack_does_not_cut_watch_request() {
        let (url, server) = silent_server().await;
        let client = client_for(&url);

        let result = tokio::time::timeout(
            NOT_CUT_GRACE,
            client.request_text(raw_get("/api/v1/configmaps?watch=true&timeoutSeconds=290")),
        )
        .await;

        assert!(
            result.is_err(),
            "a watch must not be cut by the request deadline"
        );
        server.abort();
    }
}
