// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Client-side Kubernetes API rate limiting and request accounting (ADR-0005).
//!
//! kube-rs deliberately has no `qps`/`burst` fields on [`kube::Config`]
//! (unlike client-go); its extension point is the tower middleware stack.
//! This module builds the operator's [`kube::Client`] with two layers
//! inserted via [`kube::client::ClientBuilder`]:
//!
//! 1. [`tower::limit::RateLimitLayer`] — bounds the sustained request rate to
//!    the configured QPS while allowing bursts up to the configured size.
//!    Requests over budget queue on the client (backpressure); they are not
//!    rejected.
//! 2. [`KubeApiMetricsLayer`] — counts every request, times it, and counts
//!    server-side throttles (HTTP 429) into the Prometheus registry.
//! 3. [`RequestTimeoutLayer`] (ADR-0014): a deadline on every non-watch
//!    request, so a stalled connection fails fast into the retry/backoff
//!    path instead of hanging for minutes. Watches are exempt.
//!
//! Defaults come from [`bindy_api::constants`] and can be overridden per
//! deployment with the `BINDY_KUBE_QPS` / `BINDY_KUBE_BURST` environment
//! variables. Invalid overrides fall back to the defaults with a warning —
//! a misconfigured limiter must never disable the operator or the limit.

use crate::metrics::{record_kube_api_rate_limit_hit, record_kube_api_request};
use crate::request_timeout::RequestTimeoutLayer;
use anyhow::Result;
use bindy_api::constants::{KUBE_CLIENT_BURST, KUBE_CLIENT_QPS};
use http::{Request, Response, StatusCode};
use kube::client::ClientBuilder;
use kube::Client;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower::limit::RateLimitLayer;
use tower::{Layer, Service};
use tracing::{info, warn};

/// Environment variable overriding the sustained queries-per-second limit.
pub const ENV_KUBE_QPS: &str = "BINDY_KUBE_QPS";

/// Environment variable overriding the burst size.
pub const ENV_KUBE_BURST: &str = "BINDY_KUBE_BURST";

/// Metric label used for non-resource request paths (`/version`, `/openapi`, ...).
const NON_RESOURCE_LABEL: &str = "other";

/// Client-side rate limit configuration for the Kubernetes API client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimitConfig {
    /// Sustained request rate (queries per second).
    pub qps: f32,
    /// Maximum burst of requests allowed above the sustained rate.
    pub burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            qps: KUBE_CLIENT_QPS,
            burst: KUBE_CLIENT_BURST,
        }
    }
}

impl RateLimitConfig {
    /// Build the configuration from `BINDY_KUBE_QPS` / `BINDY_KUBE_BURST`,
    /// falling back to the compiled defaults for unset or invalid values.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var(ENV_KUBE_QPS).ok().as_deref(),
            std::env::var(ENV_KUBE_BURST).ok().as_deref(),
        )
    }

    /// Build the configuration from raw override strings (testable core of
    /// [`Self::from_env`]).
    ///
    /// # Arguments
    /// * `qps` - Raw `BINDY_KUBE_QPS` value, if set
    /// * `burst` - Raw `BINDY_KUBE_BURST` value, if set
    #[must_use]
    pub fn from_values(qps: Option<&str>, burst: Option<&str>) -> Self {
        let qps = parse_override(qps, ENV_KUBE_QPS, KUBE_CLIENT_QPS, |v: &f32| {
            v.is_finite() && *v > 0.0
        });
        let burst = parse_override(burst, ENV_KUBE_BURST, KUBE_CLIENT_BURST, |v: &u32| *v > 0);
        Self { qps, burst }
    }

    /// The rate-limit window: allowing `burst` requests per `burst / qps`
    /// seconds yields the configured sustained QPS while letting bursts up
    /// to `burst` through unthrottled (a windowed approximation of
    /// client-go's token bucket).
    #[must_use]
    pub fn period(&self) -> Duration {
        Duration::from_secs_f64(f64::from(self.burst) / f64::from(self.qps))
    }
}

/// Parse an environment override, keeping `default` when the value is unset,
/// unparsable, or fails `valid`.
///
/// Shared by the rate-limit variables and the request deadline
/// ([`crate::request_timeout`]).
pub(crate) fn parse_override<T: FromStr + Copy + std::fmt::Display>(
    raw: Option<&str>,
    var_name: &str,
    default: T,
    valid: impl Fn(&T) -> bool,
) -> T {
    let Some(raw) = raw else {
        return default;
    };

    match raw.parse::<T>() {
        Ok(value) if valid(&value) => value,
        _ => {
            warn!(
                variable = var_name,
                value = raw,
                default = %default,
                "Invalid Kubernetes client override; using default"
            );
            default
        }
    }
}

/// Build a [`Client`] whose middleware stack enforces the given rate limits,
/// bounds every non-watch request with `request_timeout`, and records
/// per-request Prometheus metrics.
///
/// # Arguments
/// * `config` - Kubernetes client configuration (e.g., from `Config::infer()`)
/// * `limits` - Client-side rate limits to enforce
/// * `request_timeout` - Deadline for each non-watch request, covering the
///   response headers and body (ADR-0014); watch requests are exempt
///
/// # Errors
/// Returns an error if the client TLS/auth stack cannot be built from `config`.
pub fn build_rate_limited_client(
    config: kube::Config,
    limits: &RateLimitConfig,
    request_timeout: Duration,
) -> Result<Client> {
    let builder = ClientBuilder::try_from(config)?;
    let client = builder
        // Innermost: the deadline starts when the limiter releases the
        // request, so time queued for a rate-limit slot does not count.
        .with_layer(&RequestTimeoutLayer::new(request_timeout))
        // Metrics see the request after the limiter releases it, so recorded
        // durations exclude client-side queueing, and they wrap the deadline,
        // so a timed-out request is recorded as an error.
        .with_layer(&KubeApiMetricsLayer)
        .with_layer(&RateLimitLayer::new(u64::from(limits.burst), limits.period()))
        .build();

    info!(
        qps = limits.qps,
        burst = limits.burst,
        period_ms = limits.period().as_millis(),
        request_timeout_secs = request_timeout.as_secs(),
        "Kubernetes client initialized with client-side rate limiting and request deadline"
    );

    Ok(client)
}

/// Extract the resource plural from a Kubernetes API request path, for use as
/// a low-cardinality metric label.
///
/// Handles core-group paths (`/api/v1/...`), named-group paths
/// (`/apis/<group>/<version>/...`), both cluster- and namespace-scoped, with
/// or without object names and subresources. Non-resource paths
/// (`/version`, `/openapi/...`) map to `"other"`.
///
/// # Arguments
/// * `path` - The request URI path
#[must_use]
pub fn resource_from_path(path: &str) -> &str {
    let mut segments = path.split('/').filter(|s| !s.is_empty());

    // Strip the API prefix: `/api/<version>` or `/apis/<group>/<version>`.
    match segments.next() {
        Some("api") => {
            if segments.next().is_none() {
                return NON_RESOURCE_LABEL;
            }
        }
        Some("apis") => {
            if segments.next().is_none() || segments.next().is_none() {
                return NON_RESOURCE_LABEL;
            }
        }
        _ => return NON_RESOURCE_LABEL,
    }

    let Some(first) = segments.next() else {
        return NON_RESOURCE_LABEL;
    };

    if first != "namespaces" {
        return first;
    }

    // `/namespaces`, `/namespaces/<name>` → operations on Namespace objects;
    // `/namespaces/<name>/<resource>/...` → the namespaced resource.
    match (segments.next(), segments.next()) {
        (Some(_), Some(resource)) => resource,
        _ => first,
    }
}

/// Tower [`Layer`] recording per-request Kubernetes API client metrics.
#[derive(Debug, Clone, Copy, Default)]
pub struct KubeApiMetricsLayer;

impl<S> Layer<S> for KubeApiMetricsLayer {
    type Service = KubeApiMetrics<S>;

    fn layer(&self, inner: S) -> Self::Service {
        KubeApiMetrics { inner }
    }
}

/// Middleware [`Service`] produced by [`KubeApiMetricsLayer`].
#[derive(Debug, Clone)]
pub struct KubeApiMetrics<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for KubeApiMetrics<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = std::result::Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let verb = request.method().as_str().to_ascii_lowercase();
        let resource = resource_from_path(request.uri().path()).to_owned();
        let start = Instant::now();
        let future = self.inner.call(request);

        Box::pin(async move {
            let result = future.await;

            match &result {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        record_kube_api_rate_limit_hit(&resource, &verb);
                    }
                    // Redirects (3xx) are followed by the stack and count as success.
                    let success = status.is_success() || status.is_redirection();
                    record_kube_api_request(&resource, &verb, success, start.elapsed());
                }
                Err(_) => {
                    record_kube_api_request(&resource, &verb, false, start.elapsed());
                }
            }

            result
        })
    }
}

#[cfg(test)]
#[path = "rate_limit_tests.rs"]
mod rate_limit_tests;
