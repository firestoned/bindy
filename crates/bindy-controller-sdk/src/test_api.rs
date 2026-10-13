// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! A scripted, recording mock of the Kubernetes API server for unit tests.
//!
//! [`scripted_client`] returns a `kube::Client` whose API server answers each
//! request with the next reply from a script and records what it was asked:
//! method, path, query, content type and JSON body. A request past the end of
//! the script is answered with HTTP 500, so a test that makes more calls than
//! it scripted fails loudly instead of hanging.

use kube::Client;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The default namespace the mock client is built with.
pub(crate) const MOCK_DEFAULT_NAMESPACE: &str = "default";

/// HTTP 200, the reply most scripts use.
pub(crate) const HTTP_OK: u16 = 200;

/// HTTP 500, the reply to a request the script did not foresee.
const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;

/// One request the mock API server saw.
#[derive(Clone, Debug)]
pub(crate) struct SeenRequest {
    /// HTTP method, upper case (`GET`, `POST`, `PATCH`, `DELETE`)
    pub method: String,
    /// Request path, e.g. `/api/v1/namespaces/ns/configmaps/name`
    pub path: String,
    /// Raw query string, empty when there is none
    pub query: String,
    /// The request's `Content-Type` header, empty when there is none
    pub content_type: String,
    /// The JSON body, `Null` when the request had none
    pub body: serde_json::Value,
}

/// Requests the mock API server saw, in order.
pub(crate) type Seen = Arc<Mutex<Vec<SeenRequest>>>;

/// A Kubernetes `Status` failure body with `code` and `reason`.
pub(crate) fn status_body(code: u16, reason: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "Status",
        "apiVersion": "v1",
        "metadata": {},
        "status": "Failure",
        "message": format!("mock {reason}"),
        "reason": reason,
        "code": code,
    })
}

/// A client whose API server answers request N with `replies[N]` (status
/// code and JSON body) and records every request.
pub(crate) fn scripted_client(replies: Vec<(u16, serde_json::Value)>) -> (Client, Seen) {
    let seen: Seen = Arc::default();
    let log = seen.clone();
    let script = Arc::new(Mutex::new(VecDeque::from(replies)));
    let service = tower::service_fn(move |req: http::Request<kube::client::Body>| {
        let log = log.clone();
        let script = script.clone();
        async move {
            let (parts, body) = req.into_parts();
            let bytes = body.collect_bytes().await.unwrap_or_default();
            let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            log.lock().expect("lock").push(SeenRequest {
                method: parts.method.to_string(),
                path: parts.uri.path().to_string(),
                query: parts.uri.query().unwrap_or_default().to_string(),
                content_type: parts
                    .headers
                    .get(http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string(),
                body,
            });
            let (code, reply) = script.lock().expect("lock").pop_front().unwrap_or((
                HTTP_INTERNAL_SERVER_ERROR,
                status_body(HTTP_INTERNAL_SERVER_ERROR, "UnscriptedRequest"),
            ));
            Ok::<_, std::convert::Infallible>(
                http::Response::builder()
                    .status(code)
                    .header("content-type", "application/json")
                    .body(kube::client::Body::from(
                        serde_json::to_vec(&reply).expect("reply serializes"),
                    ))
                    .expect("response builds"),
            )
        }
    });
    (Client::new(service, MOCK_DEFAULT_NAMESPACE), seen)
}

/// The requests seen so far, cloned out of the lock.
pub(crate) fn requests(seen: &Seen) -> Vec<SeenRequest> {
    seen.lock().expect("lock").clone()
}
