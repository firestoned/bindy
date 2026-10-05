// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! A bounded deadline for non-watch Kubernetes API requests (ADR-0014).
//!
//! kube-rs 4.2 sets no read timeout by default (`kube::Config::read_timeout`
//! is `None`), and the timeouts it does have are connection-level timers on
//! the pooled socket, shared with watch streams. A request sent down a
//! stalled connection therefore waits for minutes. This module adds a
//! per-request deadline to the operator's client stack instead:
//!
//! - [`RequestTimeoutLayer`] starts one deadline when a non-watch request is
//!   dispatched and enforces it while waiting for the response headers and,
//!   through [`DeadlineBody`], while the response body is read.
//! - Watch requests (`watch=true` in the query, see [`is_watch_request`])
//!   pass through untouched: they are long-lived by design and are bounded
//!   by the server-side `timeoutSeconds` and the kube-runtime watcher's idle
//!   timeout.
//!
//! A deadline that passes fails the request with [`RequestTimeoutError`].
//! kube-rs surfaces middleware errors as `kube::Error::Service`, which
//! [`crate::retry`] classifies as retryable, so the existing backoff takes
//! over on a fresh connection.
//!
//! The default is [`KUBE_CLIENT_REQUEST_TIMEOUT_SECS`]; deployments override
//! it with `BINDY_KUBE_REQUEST_TIMEOUT_SECS`. Invalid overrides fall back to
//! the default with a warning, like the ADR-0005 rate-limit variables.

use crate::rate_limit::parse_override;
use bindy_api::constants::KUBE_CLIENT_REQUEST_TIMEOUT_SECS;
use http::{Request, Response, Uri};
use http_body::{Body, Frame, SizeHint};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::{Instant, Sleep};
use tower::{BoxError, Layer, Service};

/// Environment variable overriding the non-watch request deadline, in seconds.
pub const ENV_KUBE_REQUEST_TIMEOUT_SECS: &str = "BINDY_KUBE_REQUEST_TIMEOUT_SECS";

/// Query parameter that marks a Kubernetes watch request.
const WATCH_QUERY_PARAM: &str = "watch";

/// Values of [`WATCH_QUERY_PARAM`] the API server treats as "watch".
const WATCH_ENABLED_VALUES: [&str; 2] = ["true", "1"];

/// The error a non-watch request fails with when its deadline passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Kubernetes API request timed out after {timeout:?} (client-side request deadline)")]
pub struct RequestTimeoutError {
    timeout: Duration,
}

impl RequestTimeoutError {
    /// Create the error for a deadline of `timeout`.
    ///
    /// # Arguments
    /// * `timeout` - The deadline that passed
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// The deadline that passed.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

/// Read the request deadline from `BINDY_KUBE_REQUEST_TIMEOUT_SECS`, falling
/// back to [`KUBE_CLIENT_REQUEST_TIMEOUT_SECS`] when unset or invalid.
#[must_use]
pub fn request_timeout_from_env() -> Duration {
    request_timeout_from_value(std::env::var(ENV_KUBE_REQUEST_TIMEOUT_SECS).ok().as_deref())
}

/// Parse a raw override into the request deadline (testable core of
/// [`request_timeout_from_env`]).
///
/// Accepts a positive whole number of seconds. An unset, unparsable or zero
/// value keeps the default, with a warning for the invalid cases.
///
/// # Arguments
/// * `raw` - Raw `BINDY_KUBE_REQUEST_TIMEOUT_SECS` value, if set
#[must_use]
pub fn request_timeout_from_value(raw: Option<&str>) -> Duration {
    Duration::from_secs(parse_override(
        raw,
        ENV_KUBE_REQUEST_TIMEOUT_SECS,
        KUBE_CLIENT_REQUEST_TIMEOUT_SECS,
        |secs: &u64| *secs > 0,
    ))
}

/// Whether a request is a Kubernetes watch (`watch=true` or `watch=1` in the
/// query string). kube-rs sends `watch=true` for every watch.
///
/// # Arguments
/// * `uri` - The request URI
#[must_use]
pub fn is_watch_request(uri: &Uri) -> bool {
    let Some(query) = uri.query() else {
        return false;
    };

    query.split('&').any(|pair| {
        pair.split_once('=').is_some_and(|(key, value)| {
            key == WATCH_QUERY_PARAM && WATCH_ENABLED_VALUES.contains(&value)
        })
    })
}

/// Tower [`Layer`] enforcing a deadline on every non-watch request.
#[derive(Debug, Clone, Copy)]
pub struct RequestTimeoutLayer {
    timeout: Duration,
}

impl RequestTimeoutLayer {
    /// Create the layer with a per-request deadline of `timeout`.
    ///
    /// # Arguments
    /// * `timeout` - Deadline covering the response headers and body
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// The per-request deadline.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl<S> Layer<S> for RequestTimeoutLayer {
    type Service = RequestTimeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestTimeout {
            inner,
            timeout: self.timeout,
        }
    }
}

/// Middleware [`Service`] produced by [`RequestTimeoutLayer`].
#[derive(Debug, Clone)]
pub struct RequestTimeout<S> {
    inner: S,
    timeout: Duration,
}

type BoxedResponseFuture<B> =
    Pin<Box<dyn Future<Output = Result<Response<DeadlineBody<B>>, BoxError>> + Send>>;

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RequestTimeout<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    S::Error: Into<BoxError>,
    S::Future: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = Response<DeadlineBody<ResBody>>;
    type Error = BoxError;
    type Future = BoxedResponseFuture<ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        if is_watch_request(request.uri()) {
            let future = self.inner.call(request);
            return Box::pin(async move {
                let response = future.await.map_err(Into::into)?;
                Ok(response.map(DeadlineBody::unbounded))
            });
        }

        let timeout = self.timeout;
        let deadline = Instant::now() + timeout;
        let future = self.inner.call(request);

        Box::pin(async move {
            let Ok(result) = tokio::time::timeout_at(deadline, future).await else {
                return Err(Box::new(RequestTimeoutError::new(timeout)) as BoxError);
            };
            let response = result.map_err(Into::into)?;
            Ok(response.map(|body| DeadlineBody::bounded(body, deadline, timeout)))
        })
    }
}

/// Response body that fails with [`RequestTimeoutError`] if the request's
/// deadline passes before the body is fully read. Watch responses carry no
/// deadline and pass every frame through unchanged.
#[derive(Debug)]
pub struct DeadlineBody<B> {
    inner: B,
    deadline: Option<Pin<Box<Sleep>>>,
    timeout: Duration,
}

impl<B> DeadlineBody<B> {
    /// Wrap a body with no deadline (watch responses).
    fn unbounded(inner: B) -> Self {
        Self {
            inner,
            deadline: None,
            timeout: Duration::ZERO,
        }
    }

    /// Wrap a body that must be fully read by `deadline`.
    fn bounded(inner: B, deadline: Instant, timeout: Duration) -> Self {
        Self {
            inner,
            deadline: Some(Box::pin(tokio::time::sleep_until(deadline))),
            timeout,
        }
    }
}

impl<B> Body for DeadlineBody<B>
where
    B: Body + Unpin,
    B::Error: Into<BoxError>,
{
    type Data = B::Data;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();

        if let Poll::Ready(frame) = Pin::new(&mut this.inner).poll_frame(cx) {
            return Poll::Ready(frame.map(|result| result.map_err(Into::into)));
        }

        let Some(deadline) = this.deadline.as_mut() else {
            return Poll::Pending;
        };

        if deadline.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }

        Poll::Ready(Some(Err(
            Box::new(RequestTimeoutError::new(this.timeout)) as BoxError
        )))
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
#[path = "request_timeout_tests.rs"]
mod request_timeout_tests;
