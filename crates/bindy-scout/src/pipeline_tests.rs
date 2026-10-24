// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Reconcile-level tests for Scout's source pipeline (ADR-0022), run against
//! an in-memory fake Kubernetes API server built on `wiremock`.
//!
//! The fake keeps a real ARecord store (apply, list by label selector,
//! delete) and an ordered log of every call, so a test can assert both what
//! Scout published and the order it did it in. Every source kind is driven
//! through [`run`], so the same assertions cover all five kinds.

#[cfg(test)]
mod tests {
    use crate::constants::ANNOTATION_ALLOW_ZONE_NAMESPACES;
    use crate::crd::DNSZone;
    use crate::guardrails::{parse_allowed_ip_ranges, parse_allowed_zones, Ipv4Range};
    use crate::scout::{
        GRPCRoute, GatewayServiceTarget, HTTPRoute, NamespacedName, TCPRoute, TLSRoute,
    };
    use crate::scout::{
        ScoutContext, ScoutError, ANNOTATION_IP, ANNOTATION_RECORD_NAME, ANNOTATION_SCOUT_ENABLED,
        ANNOTATION_TTL, ANNOTATION_ZONE, FINALIZER_SCOUT, LABEL_MANAGED_BY, LABEL_MANAGED_BY_SCOUT,
        LABEL_SOURCE_CLUSTER, LABEL_SOURCE_NAME, LABEL_SOURCE_NAMESPACE, LABEL_ZONE,
    };
    use k8s_openapi::api::core::v1::Service;
    use k8s_openapi::api::networking::v1::Ingress;
    use k8s_openapi::jiff::{SignedDuration, Timestamp};
    use kube::runtime::controller::Action;
    use kube::runtime::{reflector, watcher};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::sync::{Arc, Mutex};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    /// The source-kind label (ADR-0022). Spelled out here so a rename in
    /// `scout.rs` fails these tests instead of moving in lockstep.
    const LABEL_SOURCE_KIND: &str = "bindy.firestoned.io/source-kind";

    /// The contributor map annotation and label prefix (ADR-0026), spelled
    /// out for the same reason.
    const ANNOTATION_CONTRIBUTORS: &str = "bindy.firestoned.io/scout-contributors";
    const CONTRIBUTOR_LABEL_PREFIX: &str = "contributor.scout.bindy.firestoned.io/";

    const CLUSTER: &str = "prod";
    const TARGET_NS: &str = "bindy-system";
    const SRC_NS: &str = "team-a";
    const ZONE: &str = "example.com";
    const LB_IP: &str = "10.0.0.5";
    const ANNOTATION_IP_VALUE: &str = "10.9.9.9";
    const DEFAULT_IP: &str = "10.2.2.2";
    const GATEWAY_IP: &str = "10.3.3.3";
    const GATEWAY_SERVICE_IP: &str = "10.4.4.4";
    const GATEWAY_CLASS: &str = "traefik";

    // =====================================================================
    // Fake API server
    // =====================================================================

    /// State of the fake API server: the ARecord store and every other
    /// object a reconcile may read, plus the ordered call log.
    #[derive(Default)]
    struct FakeState {
        arecords: BTreeMap<String, Value>,
        log: Vec<String>,
        zones: Vec<Value>,
        eligible_namespaces: Vec<String>,
        gateways: BTreeMap<String, Value>,
        services: BTreeMap<String, Value>,
        gateway_status: Option<u16>,
        service_status: Option<u16>,
        arecord_list_status: Option<u16>,
        source: Value,
        /// When set, an applied record keeps whatever status it had (none,
        /// for a new one): the operator has not bound it to its zone yet.
        /// Otherwise every apply returns a served record, as if the operator
        /// had reconciled it already.
        records_unserved: bool,
        /// ARecords whose DELETE the API server refuses with this status,
        /// as an admission policy (ADR-0025) or RBAC would.
        refused_deletes: BTreeMap<String, u16>,
        /// `ARecord`s whose replace (PUT) the API server refuses with this
        /// status.
        refused_writes: BTreeMap<String, u16>,
        /// The last `resourceVersion` handed out.
        resource_version: u64,
        /// A concurrent writer: the next `ARecord` create or replace first
        /// stores this record (as another Scout reconcile would have), then
        /// fails with 409 because the caller's view is now stale.
        concurrent_write: Option<Value>,
    }

    /// The status the operator writes once a record is bound to its zone and
    /// pushed to BIND9.
    fn served_status() -> Value {
        json!({
            "zoneRef": {
                "apiVersion": "bindy.firestoned.io/v1beta1", "kind": "DNSZone",
                "name": "example-com", "namespace": TARGET_NS, "zoneName": ZONE,
            },
            "conditions": [{"type": "Ready", "status": "True", "reason": "ReconcileSucceeded"}],
        })
    }

    fn status_body(code: u16) -> Value {
        json!({
            "kind": "Status", "apiVersion": "v1", "metadata": {},
            "status": "Failure", "reason": if code == 404 { "NotFound" } else { "InternalError" },
            "message": "fake", "code": code,
        })
    }

    fn error(code: u16) -> ResponseTemplate {
        ResponseTemplate::new(code).set_body_json(status_body(code))
    }

    fn list_body(api_version: &str, kind: &str, items: Vec<Value>) -> Value {
        json!({
            "apiVersion": api_version, "kind": format!("{kind}List"),
            "metadata": {"resourceVersion": "1"}, "items": items,
        })
    }

    /// Kubernetes equality / set-based selector matching, as much of it as
    /// Scout uses: `k=v`, `k!=v`, `!k` and `k`.
    fn selector_matches(selector: &str, labels: &serde_json::Map<String, Value>) -> bool {
        selector.split(',').filter(|t| !t.is_empty()).all(|term| {
            if let Some(key) = term.strip_prefix('!') {
                return !labels.contains_key(key);
            }
            if let Some((k, v)) = term.split_once("!=") {
                return labels.get(k).and_then(Value::as_str) != Some(v);
            }
            if let Some((k, v)) = term.split_once('=') {
                return labels.get(k).and_then(Value::as_str) == Some(v);
            }
            labels.contains_key(term)
        })
    }

    fn labels_of(obj: &Value) -> serde_json::Map<String, Value> {
        obj.pointer("/metadata/labels")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    impl FakeState {
        fn next_resource_version(&mut self) -> String {
            self.resource_version += 1;
            self.resource_version.to_string()
        }

        /// Stores `body` as `ARecord` `name` with a new `resourceVersion` and the
        /// status the operator would have written by now (see
        /// [`FakeState::records_unserved`]).
        fn store_arecord(&mut self, ns: &str, name: &str, mut body: Value) -> Value {
            body["metadata"]["namespace"] = json!(ns);
            body["metadata"]["resourceVersion"] = json!(self.next_resource_version());
            let status = if self.records_unserved {
                self.arecords
                    .get(name)
                    .and_then(|o| o.get("status"))
                    .cloned()
            } else {
                Some(served_status())
            };
            match status {
                Some(status) => body["status"] = status,
                None => {
                    if let Some(obj) = body.as_object_mut() {
                        obj.remove("status");
                    }
                }
            }
            self.arecords.insert(name.to_string(), body.clone());
            body
        }

        /// Runs the pending concurrent write, if any: returns `true` when the
        /// caller's write must now fail with 409.
        fn concurrent_write_happened(&mut self) -> bool {
            let Some(record) = self.concurrent_write.take() else {
                return false;
            };
            let name = record
                .pointer("/metadata/name")
                .and_then(Value::as_str)
                .expect("concurrent record has a name")
                .to_string();
            self.log.push(format!("CONCURRENT {name}"));
            self.store_arecord(TARGET_NS, &name, record);
            true
        }

        fn handle(&mut self, req: &Request) -> ResponseTemplate {
            let path = req.url.path().trim_start_matches('/').to_string();
            let segs: Vec<&str> = path.split('/').collect();
            let query: BTreeMap<String, String> = req
                .url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            let selector = query.get("labelSelector").cloned().unwrap_or_default();
            match (req.method.as_str(), segs.as_slice()) {
                (
                    "GET",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", _, "arecords"],
                ) => {
                    self.log.push(format!("LIST arecords {selector}"));
                    if let Some(code) = self.arecord_list_status {
                        return error(code);
                    }
                    let items = self
                        .arecords
                        .values()
                        .filter(|o| selector_matches(&selector, &labels_of(o)))
                        .cloned()
                        .collect();
                    ResponseTemplate::new(200).set_body_json(list_body(
                        "bindy.firestoned.io/v1beta1",
                        "ARecord",
                        items,
                    ))
                }
                (
                    "PATCH",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", _, "arecords", name],
                ) => {
                    // ADR-0026: records are written by conditional replace,
                    // never by server-side apply.
                    self.log.push(format!("UNEXPECTED APPLY {name}"));
                    error(405)
                }
                (
                    "GET",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", _, "arecords", name],
                ) => {
                    self.log.push(format!("GET arecord {name}"));
                    self.arecords.get(*name).map_or_else(
                        || error(404),
                        |o| ResponseTemplate::new(200).set_body_json(o.clone()),
                    )
                }
                (
                    "POST",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", ns, "arecords"],
                ) => {
                    let body: Value =
                        serde_json::from_slice(&req.body).expect("create body is JSON");
                    let name = body
                        .pointer("/metadata/name")
                        .and_then(Value::as_str)
                        .expect("created record has a name")
                        .to_string();
                    if self.concurrent_write_happened() || self.arecords.contains_key(&name) {
                        self.log.push(format!("CONFLICT CREATE {name}"));
                        return error(409);
                    }
                    self.log.push(format!("CREATE {name}"));
                    let stored = self.store_arecord(ns, &name, body);
                    ResponseTemplate::new(201).set_body_json(stored)
                }
                (
                    "PUT",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", ns, "arecords", name],
                ) => {
                    if let Some(code) = self.refused_writes.get(*name) {
                        self.log.push(format!("REFUSED REPLACE {name}"));
                        return error(*code);
                    }
                    let body: Value =
                        serde_json::from_slice(&req.body).expect("replace body is JSON");
                    if self.concurrent_write_happened() {
                        self.log.push(format!("CONFLICT REPLACE {name}"));
                        return error(409);
                    }
                    let Some(current) = self.arecords.get(*name) else {
                        return error(404);
                    };
                    let sent = body.pointer("/metadata/resourceVersion");
                    if sent.is_some() && sent != current.pointer("/metadata/resourceVersion") {
                        self.log.push(format!("CONFLICT REPLACE {name}"));
                        return error(409);
                    }
                    self.log.push(format!("REPLACE {name}"));
                    let stored = self.store_arecord(ns, name, body);
                    ResponseTemplate::new(200).set_body_json(stored)
                }
                (
                    "DELETE",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", _, "arecords", name],
                ) => {
                    if let Some(code) = self.refused_deletes.get(*name) {
                        self.log.push(format!("REFUSED DELETE {name}"));
                        return error(*code);
                    }
                    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                    if let Some(wanted) = body.pointer("/preconditions/resourceVersion") {
                        let current = self
                            .arecords
                            .get(*name)
                            .and_then(|o| o.pointer("/metadata/resourceVersion"));
                        if current.is_some() && current != Some(wanted) {
                            self.log.push(format!("CONFLICT DELETE {name}"));
                            return error(409);
                        }
                    }
                    self.log.push(format!("DELETE {name}"));
                    match self.arecords.remove(*name) {
                        Some(obj) => ResponseTemplate::new(200).set_body_json(obj),
                        None => error(404),
                    }
                }
                (
                    "GET",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", ns, "dnszones", name],
                ) => {
                    self.log.push(format!("GET dnszone {ns}/{name}"));
                    self.zones
                        .iter()
                        .find(|z| {
                            z.pointer("/metadata/name").and_then(Value::as_str) == Some(name)
                                && z.pointer("/metadata/namespace").and_then(Value::as_str)
                                    == Some(ns)
                        })
                        .map_or_else(
                            || error(404),
                            |z| ResponseTemplate::new(200).set_body_json(z.clone()),
                        )
                }
                ("GET", ["api", "v1", "namespaces"]) => {
                    self.log.push("LIST namespaces".to_string());
                    let field = query.get("fieldSelector").cloned().unwrap_or_default();
                    let wanted = field.trim_start_matches("metadata.name=").to_string();
                    let items = self
                        .eligible_namespaces
                        .iter()
                        .filter(|n| **n == wanted)
                        .map(|n| json!({"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": n}}))
                        .collect();
                    ResponseTemplate::new(200).set_body_json(list_body("v1", "Namespace", items))
                }
                (
                    "GET",
                    ["apis", "gateway.networking.k8s.io", "v1", "namespaces", ns, "gateways", name],
                ) => {
                    self.log.push(format!("GET gateway {ns}/{name}"));
                    if let Some(code) = self.gateway_status {
                        return error(code);
                    }
                    self.gateways.get(&format!("{ns}/{name}")).map_or_else(
                        || error(404),
                        |g| ResponseTemplate::new(200).set_body_json(g.clone()),
                    )
                }
                ("GET", ["api", "v1", "services"]) => {
                    // A source controller's cluster-wide Service watch: an
                    // empty list makes its store ready.
                    self.log.push("LIST services".to_string());
                    ResponseTemplate::new(200).set_body_json(list_body("v1", "Service", Vec::new()))
                }
                ("GET", ["api", "v1", "namespaces", ns, "services"]) => {
                    // Applies the label selector as the API server would; a
                    // matching non-LoadBalancer Service is still returned, so
                    // the code under test must drop it itself.
                    self.log.push(format!("LIST services {ns} {selector}"));
                    if let Some(code) = self.service_status {
                        return error(code);
                    }
                    let prefix = format!("{ns}/");
                    let items = self
                        .services
                        .iter()
                        .filter(|(k, v)| {
                            k.starts_with(&prefix) && selector_matches(&selector, &labels_of(v))
                        })
                        .map(|(_, v)| v.clone())
                        .collect();
                    ResponseTemplate::new(200).set_body_json(list_body("v1", "Service", items))
                }
                ("GET", ["api", "v1", "namespaces", ns, "services", name]) => {
                    self.log.push(format!("GET service {ns}/{name}"));
                    if let Some(code) = self.service_status {
                        return error(code);
                    }
                    self.services.get(&format!("{ns}/{name}")).map_or_else(
                        || error(404),
                        |s| ResponseTemplate::new(200).set_body_json(s.clone()),
                    )
                }
                ("PATCH", _) => {
                    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                    let ops: Vec<String> = body
                        .as_array()
                        .map(|ops| {
                            ops.iter()
                                .filter_map(|op| op.get("op").and_then(Value::as_str))
                                .filter(|op| *op != "test")
                                .map(ToString::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    self.log.push(format!("FINALIZER {}", ops.join(",")));
                    ResponseTemplate::new(200).set_body_json(self.source.clone())
                }
                _ => {
                    self.log.push(format!("UNHANDLED {} {path}", req.method));
                    error(404)
                }
            }
        }
    }

    struct Fake(Arc<Mutex<FakeState>>);

    impl Respond for Fake {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            self.0.lock().expect("fake state lock").handle(req)
        }
    }

    /// A fake API server plus the context a reconcile runs with.
    struct Harness {
        _server: MockServer,
        state: Arc<Mutex<FakeState>>,
        client: kube::Client,
        zones: Vec<DNSZone>,
        default_ips: Vec<String>,
        namespace_selector: Option<String>,
        gateway_services: BTreeMap<String, GatewayServiceTarget>,
        allowed_zones: Option<Vec<String>>,
        allowed_ip_ranges: Option<Vec<Ipv4Range>>,
    }

    impl Harness {
        /// A harness whose DNSZone `example.com` lives in the source namespace.
        async fn new() -> Self {
            let mut h = Self::without_zone().await;
            h.add_zone(SRC_NS, None);
            h
        }

        async fn without_zone() -> Self {
            let server = MockServer::start().await;
            let state = Arc::new(Mutex::new(FakeState::default()));
            Mock::given(wiremock::matchers::any())
                .respond_with(Fake(state.clone()))
                .mount(&server)
                .await;
            let config = kube::Config::new(server.uri().parse().expect("mock server uri"));
            let client = kube::Client::try_from(config).expect("client from mock config");
            Self {
                _server: server,
                state,
                client,
                zones: Vec::new(),
                default_ips: Vec::new(),
                namespace_selector: None,
                gateway_services: BTreeMap::new(),
                allowed_zones: None,
                allowed_ip_ranges: None,
            }
        }

        /// Makes the API server refuse deleting ARecord `name` with `code`.
        fn refuse_delete(&self, name: &str, code: u16) {
            self.state
                .lock()
                .expect("lock")
                .refused_deletes
                .insert(name.to_string(), code);
        }

        fn refused(&self) -> Vec<String> {
            self.log()
                .iter()
                .filter_map(|l| l.strip_prefix("REFUSED DELETE ").map(ToString::to_string))
                .collect()
        }

        /// Makes the API server refuse replacing `ARecord` `name` with `code`.
        fn refuse_write(&self, name: &str, code: u16) {
            self.state
                .lock()
                .expect("lock")
                .refused_writes
                .insert(name.to_string(), code);
        }

        /// Makes the next `ARecord` create or replace lose a race: `record` is
        /// stored first, and the write fails with 409.
        fn race_next_write(&self, record: Value) {
            self.state.lock().expect("lock").concurrent_write = Some(record);
        }

        fn record(&self, name: &str) -> Value {
            self.state
                .lock()
                .expect("lock")
                .arecords
                .get(name)
                .cloned()
                .unwrap_or_else(|| panic!("no ARecord {name}"))
        }

        fn add_zone(&mut self, namespace: &str, grant: Option<&str>) {
            let annotations = grant.map_or(
                Value::Null,
                |g| json!({ ANNOTATION_ALLOW_ZONE_NAMESPACES: g }),
            );
            let zone = json!({
                "apiVersion": "bindy.firestoned.io/v1beta1",
                "kind": "DNSZone",
                "metadata": {"name": "example-com", "namespace": namespace, "annotations": annotations},
                "spec": {
                    "zoneName": ZONE,
                    "soaRecord": {
                        "primaryNs": "ns1.example.com.", "adminEmail": "admin.example.com.",
                        "serial": 2_024_010_101_u32, "refresh": 3600, "retry": 600,
                        "expire": 604_800, "negativeTtl": 86_400
                    }
                }
            });
            self.state.lock().expect("lock").zones.push(zone.clone());
            self.zones
                .push(serde_json::from_value(zone).expect("valid DNSZone"));
        }

        fn ctx(&self) -> Arc<ScoutContext> {
            let (store, mut writer) = reflector::store::<DNSZone>();
            writer.apply_watcher_event(&watcher::Event::Init);
            for z in &self.zones {
                writer.apply_watcher_event(&watcher::Event::InitApply(z.clone()));
            }
            writer.apply_watcher_event(&watcher::Event::InitDone);
            Arc::new(ScoutContext {
                client: self.client.clone(),
                remote_client: self.client.clone(),
                target_namespace: TARGET_NS.to_string(),
                cluster_name: CLUSTER.to_string(),
                excluded_namespaces: vec!["kube-system".to_string()],
                default_ips: self.default_ips.clone(),
                gateway_services: self.gateway_services.clone(),
                default_zone: None,
                namespace_selector: self.namespace_selector.clone(),
                allowed_zones: self.allowed_zones.clone(),
                allowed_ip_ranges: self.allowed_ip_ranges.clone(),
                zone_store: store,
            })
        }

        fn seed(&self, mut record: Value) {
            let name = record
                .pointer("/metadata/name")
                .and_then(Value::as_str)
                .expect("seeded record has a name")
                .to_string();
            let mut state = self.state.lock().expect("lock");
            if record.pointer("/metadata/resourceVersion").is_none() {
                record["metadata"]["resourceVersion"] = json!(state.next_resource_version());
            }
            state.arecords.insert(name, record);
        }

        fn log(&self) -> Vec<String> {
            self.state.lock().expect("lock").log.clone()
        }

        /// Whether the operator has bound the records Scout applies (see
        /// [`FakeState::records_unserved`]).
        /// Serving also binds the records already stored, as the operator
        /// would by the next reconcile: since ADR-0026 an unchanged record is
        /// not rewritten, so only the operator updates its status.
        fn set_records_served(&self, served: bool) {
            let mut state = self.state.lock().expect("lock");
            state.records_unserved = !served;
            if served {
                for record in state.arecords.values_mut() {
                    record["status"] = served_status();
                }
            }
        }

        fn clear_log(&self) {
            self.state.lock().expect("lock").log.clear();
        }

        fn records(&self) -> Vec<Value> {
            self.state
                .lock()
                .expect("lock")
                .arecords
                .values()
                .cloned()
                .collect()
        }

        fn record_names(&self) -> Vec<String> {
            self.state
                .lock()
                .expect("lock")
                .arecords
                .keys()
                .cloned()
                .collect()
        }

        /// The `ARecord`s written (created or replaced), in order.
        fn applied(&self) -> Vec<String> {
            self.log()
                .iter()
                .filter_map(|l| {
                    l.strip_prefix("CREATE ")
                        .or_else(|| l.strip_prefix("REPLACE "))
                        .map(ToString::to_string)
                })
                .collect()
        }

        fn deleted(&self) -> Vec<String> {
            self.log()
                .iter()
                .filter_map(|l| l.strip_prefix("DELETE ").map(ToString::to_string))
                .collect()
        }

        fn finalizer_ops(&self) -> Vec<String> {
            self.log()
                .iter()
                .filter_map(|l| l.strip_prefix("FINALIZER ").map(ToString::to_string))
                .collect()
        }

        /// Runs one reconcile with this harness's context, after storing
        /// `src` as the object a finalizer patch returns.
        async fn run(&self, src: &Src) -> Result<Action, ScoutError> {
            self.state.lock().expect("lock").source = src.json();
            run(src, self.ctx()).await
        }
    }

    // =====================================================================
    // Source fixtures
    // =====================================================================

    /// One source object of any kind Scout watches.
    #[derive(Clone)]
    #[allow(
        clippy::large_enum_variant,
        reason = "test fixture, built a handful of times per test"
    )]
    enum Src {
        Ingress(Ingress),
        Service(Service),
        Http(HTTPRoute),
        Grpc(GRPCRoute),
        Tls(TLSRoute),
        Tcp(TCPRoute),
    }

    impl Src {
        fn json(&self) -> Value {
            match self {
                Src::Ingress(o) => serde_json::to_value(o),
                Src::Service(o) => serde_json::to_value(o),
                Src::Http(o) => serde_json::to_value(o),
                Src::Grpc(o) => serde_json::to_value(o),
                Src::Tls(o) => serde_json::to_value(o),
                Src::Tcp(o) => serde_json::to_value(o),
            }
            .expect("source serializes")
        }

        fn kind(&self) -> &'static str {
            match self {
                Src::Ingress(_) => "Ingress",
                Src::Service(_) => "Service",
                Src::Http(_) => "HTTPRoute",
                Src::Grpc(_) => "GRPCRoute",
                Src::Tls(_) => "TLSRoute",
                Src::Tcp(_) => "TCPRoute",
            }
        }
    }

    /// Dispatches one reconcile for any kind. This adapter is the only code
    /// that knows how a kind is reconciled.
    async fn run(src: &Src, ctx: Arc<ScoutContext>) -> Result<Action, ScoutError> {
        use crate::pipeline::reconcile_source;
        match src.clone() {
            Src::Ingress(o) => reconcile_source(Arc::new(o), ctx).await,
            Src::Service(o) => reconcile_source(Arc::new(o), ctx).await,
            Src::Http(o) => reconcile_source(Arc::new(o), ctx).await,
            Src::Grpc(o) => reconcile_source(Arc::new(o), ctx).await,
            Src::Tls(o) => reconcile_source(Arc::new(o), ctx).await,
            Src::Tcp(o) => reconcile_source(Arc::new(o), ctx).await,
        }
    }

    /// Metadata builder for a source object.
    #[derive(Clone)]
    struct Meta {
        name: String,
        namespace: String,
        annotations: BTreeMap<String, String>,
        finalized: bool,
        deleting_secs_ago: Option<i64>,
    }

    /// An opted-in, finalized source in the default zone.
    fn meta(name: &str) -> Meta {
        let mut annotations = BTreeMap::new();
        annotations.insert(ANNOTATION_SCOUT_ENABLED.to_string(), "true".to_string());
        annotations.insert(ANNOTATION_ZONE.to_string(), ZONE.to_string());
        Meta {
            name: name.to_string(),
            namespace: SRC_NS.to_string(),
            annotations,
            finalized: true,
            deleting_secs_ago: None,
        }
    }

    impl Meta {
        fn ns(mut self, ns: &str) -> Self {
            self.namespace = ns.to_string();
            self
        }
        fn ann(mut self, key: &str, value: &str) -> Self {
            self.annotations.insert(key.to_string(), value.to_string());
            self
        }
        fn without(mut self, key: &str) -> Self {
            self.annotations.remove(key);
            self
        }
        fn unfinalized(mut self) -> Self {
            self.finalized = false;
            self
        }
        fn deleting(mut self, secs_ago: i64) -> Self {
            self.deleting_secs_ago = Some(secs_ago);
            self
        }
        fn json(&self) -> Value {
            let mut m = json!({
                "name": self.name, "namespace": self.namespace,
                "annotations": self.annotations, "resourceVersion": "7",
            });
            if self.finalized {
                m["finalizers"] = json!([FINALIZER_SCOUT]);
            }
            if let Some(secs) = self.deleting_secs_ago {
                let ts = Timestamp::now() - SignedDuration::from_secs(secs);
                m["deletionTimestamp"] = json!(ts.to_string());
            }
            m
        }
    }

    fn ingress(meta: &Meta, hosts: &[&str], lb_ips: &[&str]) -> Src {
        let rules: Vec<Value> = hosts
            .iter()
            .map(|h| {
                if h.is_empty() {
                    json!({})
                } else {
                    json!({"host": h})
                }
            })
            .collect();
        let lb: Vec<Value> = lb_ips.iter().map(|ip| json!({"ip": ip})).collect();
        Src::Ingress(
            serde_json::from_value(json!({
                "apiVersion": "networking.k8s.io/v1", "kind": "Ingress",
                "metadata": meta.json(), "spec": {"rules": rules},
                "status": {"loadBalancer": {"ingress": lb}},
            }))
            .expect("valid Ingress"),
        )
    }

    fn service_json(meta: &Meta, svc_type: &str, lb_ips: &[&str]) -> Value {
        let lb: Vec<Value> = lb_ips.iter().map(|ip| json!({"ip": ip})).collect();
        json!({
            "apiVersion": "v1", "kind": "Service", "metadata": meta.json(),
            "spec": {"type": svc_type},
            "status": {"loadBalancer": {"ingress": lb}},
        })
    }

    fn service(meta: &Meta, svc_type: &str, lb_ips: &[&str]) -> Src {
        Src::Service(
            serde_json::from_value(service_json(meta, svc_type, lb_ips)).expect("valid Service"),
        )
    }

    fn parent_refs(gateway: Option<&str>) -> Value {
        gateway.map_or(json!([]), |g| json!([{"name": g}]))
    }

    fn httproute(meta: &Meta, hostnames: &[&str], gateway: Option<&str>) -> Src {
        Src::Http(
            serde_json::from_value(json!({
                "apiVersion": "gateway.networking.k8s.io/v1", "kind": "HTTPRoute",
                "metadata": meta.json(),
                "spec": {"hostnames": hostnames, "parentRefs": parent_refs(gateway)},
            }))
            .expect("valid HTTPRoute"),
        )
    }

    fn grpcroute(meta: &Meta, hostnames: &[&str], gateway: Option<&str>) -> Src {
        Src::Grpc(
            serde_json::from_value(json!({
                "apiVersion": "gateway.networking.k8s.io/v1", "kind": "GRPCRoute",
                "metadata": meta.json(),
                "spec": {"hostnames": hostnames, "parentRefs": parent_refs(gateway)},
            }))
            .expect("valid GRPCRoute"),
        )
    }

    fn tlsroute(meta: &Meta, hostnames: &[&str], gateway: Option<&str>) -> Src {
        Src::Tls(
            serde_json::from_value(json!({
                "apiVersion": "gateway.networking.k8s.io/v1alpha2", "kind": "TLSRoute",
                "metadata": meta.json(),
                "spec": {"hostnames": hostnames, "parentRefs": parent_refs(gateway), "rules": []},
            }))
            .expect("valid TLSRoute"),
        )
    }

    fn tcproute(meta: &Meta, gateway: Option<&str>) -> Src {
        Src::Tcp(
            serde_json::from_value(json!({
                "apiVersion": "gateway.networking.k8s.io/v1alpha2", "kind": "TCPRoute",
                "metadata": meta.json(),
                "spec": {"parentRefs": parent_refs(gateway), "rules": []},
            }))
            .expect("valid TCPRoute"),
        )
    }

    /// One publishable object of every kind, all named `name`, each with an
    /// explicit IP annotation and (for TCPRoute) a record-name override.
    fn every_kind(meta: &Meta) -> Vec<Src> {
        let m = meta.clone().ann(ANNOTATION_IP, ANNOTATION_IP_VALUE);
        vec![
            ingress(&m, &["api.example.com"], &[]),
            service(&m, "LoadBalancer", &[]),
            httproute(&m, &["api.example.com"], None),
            grpcroute(&m, &["api.example.com"], None),
            tlsroute(&m, &["api.example.com"], None),
            tcproute(&with_record_name(&m), None),
        ]
    }

    /// TCPRoute needs a record-name override; keep one the test already set.
    fn with_record_name(m: &Meta) -> Meta {
        if m.annotations.contains_key(ANNOTATION_RECORD_NAME) {
            return m.clone();
        }
        m.clone().ann(ANNOTATION_RECORD_NAME, "api")
    }

    fn gateway_json(name: &str, addresses: &[(&str, &str)]) -> Value {
        let addrs: Vec<Value> = addresses
            .iter()
            .map(|(t, v)| json!({"type": t, "value": v}))
            .collect();
        json!({
            "apiVersion": "gateway.networking.k8s.io/v1", "kind": "Gateway",
            "metadata": {"name": name, "namespace": SRC_NS},
            "spec": {"gatewayClassName": GATEWAY_CLASS},
            "status": {"addresses": addrs},
        })
    }

    /// An ARecord as an earlier reconcile (or an earlier Scout) left it.
    fn record(
        name: &str,
        cluster: &str,
        source_name: &str,
        zone: &str,
        kind: Option<&str>,
    ) -> Value {
        let mut labels = json!({
            LABEL_MANAGED_BY: LABEL_MANAGED_BY_SCOUT,
            LABEL_SOURCE_CLUSTER: cluster,
            LABEL_SOURCE_NAMESPACE: SRC_NS,
            LABEL_SOURCE_NAME: source_name,
            LABEL_ZONE: zone,
        });
        if let Some(kind) = kind {
            labels[LABEL_SOURCE_KIND] = json!(kind);
        }
        json!({
            "apiVersion": "bindy.firestoned.io/v1beta1", "kind": "ARecord",
            "metadata": {"name": name, "namespace": TARGET_NS, "labels": labels},
            "spec": {"name": "old", "ipv4Addresses": ["192.0.2.1"]},
        })
    }

    fn spec_names(records: &[Value]) -> Vec<String> {
        let mut names: Vec<String> = records
            .iter()
            .filter_map(|r| r.pointer("/spec/name").and_then(Value::as_str))
            .map(ToString::to_string)
            .collect();
        names.sort();
        names
    }

    fn ips_of(record: &Value) -> Vec<String> {
        record
            .pointer("/spec/ipv4Addresses")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn only_record(h: &Harness) -> Value {
        let records = h.records();
        assert_eq!(
            records.len(),
            1,
            "expected exactly one record, got {records:?}"
        );
        records[0].clone()
    }

    fn requeued(action: &Action) -> bool {
        *action != Action::await_change()
    }

    // =====================================================================
    // ADR-0026 helpers, computed independently of `scout.rs` and
    // `contributors.rs`
    // =====================================================================

    fn sha256_hex(input: &str, bytes: usize) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(input.as_bytes())
            .iter()
            .take(bytes)
            .fold(String::new(), |mut hex, b| {
                let _ = write!(hex, "{b:02x}");
                hex
            })
    }

    /// The per-name record ADR-0026 Decision 1 specifies for `dns_name` in
    /// [`ZONE`], written by `cluster`.
    fn expected_name_in(cluster: &str, dns_name: &str) -> String {
        let readable = if dns_name == "@" { "apex" } else { dns_name };
        let hash = sha256_hex(&format!("{cluster}\0{ZONE}\0{dns_name}"), 4);
        format!(
            "scout-{cluster}-{}-{}-{hash}",
            readable.replace('.', "-"),
            ZONE.replace('.', "-")
        )
    }

    /// [`expected_name_in`] for this Scout's cluster.
    fn expected_name(dns_name: &str) -> String {
        expected_name_in(CLUSTER, dns_name)
    }

    fn contributor_key(kind: &str, namespace: &str, name: &str) -> String {
        format!("{kind}/{namespace}/{name}")
    }

    fn contributor_label(kind: &str, namespace: &str, name: &str) -> String {
        format!(
            "{CONTRIBUTOR_LABEL_PREFIX}{}",
            sha256_hex(&format!("{kind}\0{namespace}\0{name}"), 8)
        )
    }

    /// The contributor map of a stored record.
    fn contributors(record: &Value) -> BTreeMap<String, Value> {
        let raw = record
            .pointer("/metadata/annotations")
            .and_then(|a| a.get(ANNOTATION_CONTRIBUTORS))
            .and_then(Value::as_str)
            .expect("record has a contributor map");
        serde_json::from_str(raw).expect("contributor map is JSON")
    }

    /// A per-name record as an earlier reconcile left it: `entries` are
    /// `(kind, namespace, name, ips)`.
    fn name_record(
        cluster: &str,
        dns_name: &str,
        entries: &[(&str, &str, &str, &[&str])],
    ) -> Value {
        let mut labels = json!({
            LABEL_MANAGED_BY: LABEL_MANAGED_BY_SCOUT,
            LABEL_SOURCE_CLUSTER: cluster,
            LABEL_ZONE: ZONE,
        });
        let mut map = serde_json::Map::new();
        let mut ips: Vec<String> = Vec::new();
        for (kind, ns, name, entry_ips) in entries {
            labels[contributor_label(kind, ns, name)] = json!("true");
            map.insert(contributor_key(kind, ns, name), json!({ "ips": entry_ips }));
            ips.extend(entry_ips.iter().map(ToString::to_string));
        }
        ips.sort();
        ips.dedup();
        json!({
            "apiVersion": "bindy.firestoned.io/v1beta1", "kind": "ARecord",
            "metadata": {
                "name": expected_name_in(cluster, dns_name), "namespace": TARGET_NS,
                "labels": labels,
                "annotations": { ANNOTATION_CONTRIBUTORS: Value::Object(map).to_string() },
            },
            "spec": {"name": dns_name, "ipv4Addresses": ips},
        })
    }

    /// A `LoadBalancer` Service in [`SRC_NS`] published at the zone apex, as
    /// in issue #556 (one Service per availability zone).
    fn apex_service(name: &str, ip: &str) -> Src {
        service(
            &meta(name).ann(ANNOTATION_RECORD_NAME, "@"),
            "LoadBalancer",
            &[ip],
        )
    }

    // =====================================================================
    // Pinned behaviour: what each kind publishes
    // =====================================================================

    #[tokio::test]
    async fn ingress_publishes_one_record_per_in_zone_host() {
        let h = Harness::new().await;
        let src = ingress(
            &meta("web"),
            &["api.example.com", "", "www.example.com", "api.other.org"],
            &[LB_IP],
        );

        let action = h.run(&src).await.expect("reconcile succeeds");

        assert_eq!(action, Action::await_change());
        let records = h.records();
        assert_eq!(spec_names(&records), vec!["api", "www"]);
        for r in &records {
            assert_eq!(ips_of(r), vec![LB_IP]);
            let labels = labels_of(r);
            assert_eq!(labels[LABEL_MANAGED_BY], json!(LABEL_MANAGED_BY_SCOUT));
            assert_eq!(labels[LABEL_SOURCE_CLUSTER], json!(CLUSTER));
            assert_eq!(labels[LABEL_ZONE], json!(ZONE));
            assert!(labels.contains_key(&contributor_label("Ingress", SRC_NS, "web")));
            assert!(
                !labels.contains_key(LABEL_SOURCE_NAME),
                "a per-name record names no single source (ADR-0026)"
            );
            assert_eq!(
                contributors(r).keys().collect::<Vec<_>>(),
                vec![&contributor_key("Ingress", SRC_NS, "web")]
            );
            assert_eq!(r.pointer("/metadata/namespace"), Some(&json!(TARGET_NS)));
        }
    }

    #[tokio::test]
    async fn service_publishes_its_own_name_in_the_zone() {
        let h = Harness::new().await;
        h.run(&service(&meta("db"), "LoadBalancer", &[LB_IP]))
            .await
            .expect("reconcile succeeds");
        let r = only_record(&h);
        assert_eq!(r.pointer("/spec/name"), Some(&json!("db")));
        assert_eq!(ips_of(&r), vec![LB_IP]);
    }

    #[tokio::test]
    async fn a_non_loadbalancer_service_is_not_published() {
        let h = Harness::new().await;
        h.run(&service(&meta("db").unfinalized(), "ClusterIP", &[]))
            .await
            .expect("reconcile succeeds");
        assert!(h.applied().is_empty());
        assert!(h.finalizer_ops().is_empty());
    }

    #[tokio::test]
    async fn httproute_and_tlsroute_publish_one_record_per_hostname() {
        let m = meta("web").ann(ANNOTATION_IP, ANNOTATION_IP_VALUE);
        for src in [
            httproute(&m, &["a.example.com", "", "b.example.com"], None),
            grpcroute(&m, &["a.example.com", "", "b.example.com"], None),
            tlsroute(&m, &["a.example.com", "", "b.example.com"], None),
        ] {
            let h = Harness::new().await;
            h.run(&src).await.expect("reconcile succeeds");
            assert_eq!(spec_names(&h.records()), vec!["a", "b"], "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn tcproute_publishes_only_with_a_valid_record_name_override() {
        let m = meta("pg").ann(ANNOTATION_IP, ANNOTATION_IP_VALUE);

        let h = Harness::new().await;
        let action = h.run(&tcproute(&m, None)).await.expect("reconcile");
        assert!(requeued(&action), "no override must requeue");
        assert!(h.applied().is_empty());

        let h = Harness::new().await;
        let action = h
            .run(&tcproute(
                &m.clone().ann(ANNOTATION_RECORD_NAME, "bad name"),
                None,
            ))
            .await
            .expect("reconcile");
        assert!(requeued(&action), "an invalid override must requeue");
        assert!(h.applied().is_empty());

        let h = Harness::new().await;
        h.run(&tcproute(&m.ann(ANNOTATION_RECORD_NAME, "db"), None))
            .await
            .expect("reconcile");
        assert_eq!(only_record(&h).pointer("/spec/name"), Some(&json!("db")));
    }

    #[tokio::test]
    async fn the_record_name_override_applies_to_every_kind() {
        for src in every_kind(&meta("web").ann(ANNOTATION_RECORD_NAME, "@")) {
            let h = Harness::new().await;
            h.run(&src).await.expect("reconcile");
            assert_eq!(
                only_record(&h).pointer("/spec/name"),
                Some(&json!("@")),
                "{}",
                src.kind()
            );
        }
    }

    #[tokio::test]
    async fn an_invalid_record_name_override_publishes_nothing_for_a_service() {
        let h = Harness::new().await;
        let action = h
            .run(&service(
                &meta("db").ann(ANNOTATION_RECORD_NAME, "a b"),
                "LoadBalancer",
                &[LB_IP],
            ))
            .await
            .expect("reconcile");
        assert!(requeued(&action));
        assert!(h.applied().is_empty());
    }

    #[tokio::test]
    async fn a_valid_ttl_annotation_reaches_the_record() {
        for src in every_kind(&meta("web").ann(ANNOTATION_TTL, "300")) {
            let h = Harness::new().await;
            h.run(&src).await.expect("reconcile");
            assert_eq!(
                only_record(&h).pointer("/spec/ttl"),
                Some(&json!(300)),
                "{}",
                src.kind()
            );
        }
    }

    // =====================================================================
    // Pinned behaviour: opt-in, finalizer, opt-out
    // =====================================================================

    #[tokio::test]
    async fn an_object_that_is_not_opted_in_is_ignored() {
        for src in every_kind(&meta("web").without(ANNOTATION_SCOUT_ENABLED).unfinalized()) {
            let h = Harness::new().await;
            let action = h.run(&src).await.expect("reconcile");
            assert_eq!(action, Action::await_change());
            assert!(h.applied().is_empty(), "{}", src.kind());
            assert!(h.finalizer_ops().is_empty(), "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn the_finalizer_is_added_before_anything_is_published() {
        for src in every_kind(&meta("web").unfinalized()) {
            let h = Harness::new().await;
            let action = h.run(&src).await.expect("reconcile");
            assert_eq!(action, Action::await_change());
            assert!(h.applied().is_empty(), "{}", src.kind());
            assert_eq!(h.finalizer_ops(), vec!["add"], "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn opting_out_removes_the_records_and_the_finalizer() {
        for src in every_kind(&meta("web").without(ANNOTATION_SCOUT_ENABLED)) {
            let h = Harness::new().await;
            h.seed(record("own", CLUSTER, "web", ZONE, Some(src.kind())));
            h.run(&src).await.expect("reconcile");
            assert_eq!(h.deleted(), vec!["own"], "{}", src.kind());
            assert_eq!(h.finalizer_ops(), vec!["remove"], "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn opting_out_by_stripping_every_annotation_still_cleans_up_stale_records() {
        // bug-190: the zone annotation is gone with the opt-out, so the zone
        // for stale-cluster cleanup comes from the deleted record's label.
        let mut m = meta("web");
        m.annotations.clear();
        let h = Harness::new().await;
        h.seed(record("own", CLUSTER, "web", ZONE, Some("Ingress")));
        h.seed(record("stale", "old-prod", "web", ZONE, Some("Ingress")));
        h.run(&ingress(&m, &["api.example.com"], &[]))
            .await
            .expect("reconcile");
        let mut deleted = h.deleted();
        deleted.sort();
        assert_eq!(deleted, vec!["own", "stale"]);
    }

    #[tokio::test]
    async fn a_service_that_stops_being_a_loadbalancer_is_released() {
        let h = Harness::new().await;
        h.seed(record("own", CLUSTER, "db", ZONE, Some("Service")));
        h.run(&service(&meta("db"), "ClusterIP", &[]))
            .await
            .expect("reconcile");
        assert_eq!(h.deleted(), vec!["own"]);
        assert_eq!(h.finalizer_ops(), vec!["remove"]);
    }

    #[tokio::test]
    async fn a_namespace_outside_the_selector_releases_the_object() {
        let mut h = Harness::new().await;
        h.namespace_selector = Some("scout=on".to_string());
        let src = ingress(&meta("web"), &["api.example.com"], &[LB_IP]);
        h.seed(record("own", CLUSTER, "web", ZONE, Some("Ingress")));
        h.run(&src).await.expect("reconcile");
        assert!(h.applied().is_empty());
        assert_eq!(h.deleted(), vec!["own"]);
        assert_eq!(h.finalizer_ops(), vec!["remove"]);
    }

    #[tokio::test]
    async fn an_excluded_namespace_is_skipped() {
        let h = Harness::new().await;
        let src = ingress(
            &meta("web").ns("kube-system"),
            &["api.example.com"],
            &[LB_IP],
        );
        h.run(&src).await.expect("reconcile");
        assert!(h.log().is_empty(), "got {:?}", h.log());
    }

    // =====================================================================
    // Pinned behaviour: zone authorization
    // =====================================================================

    #[tokio::test]
    async fn a_zone_that_does_not_grant_the_namespace_publishes_nothing() {
        for src in every_kind(&meta("web")) {
            let mut h = Harness::without_zone().await;
            h.add_zone("other-ns", None);
            let action = h.run(&src).await.expect("reconcile");
            assert!(requeued(&action), "{}", src.kind());
            assert!(h.applied().is_empty(), "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_zone_that_grants_the_namespace_is_published_into() {
        for src in every_kind(&meta("web")) {
            let mut h = Harness::without_zone().await;
            h.add_zone("other-ns", Some(SRC_NS));
            h.run(&src).await.expect("reconcile");
            assert_eq!(h.records().len(), 1, "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_missing_zone_publishes_nothing() {
        for src in every_kind(&meta("web")) {
            let h = Harness::without_zone().await;
            let action = h.run(&src).await.expect("reconcile");
            assert!(requeued(&action), "{}", src.kind());
            assert!(h.applied().is_empty(), "{}", src.kind());
        }
    }

    // =====================================================================
    // Pinned behaviour: IP precedence
    // =====================================================================

    #[tokio::test]
    async fn ingress_and_service_ips_prefer_annotation_then_defaults_then_status() {
        let builders: [fn(&Meta) -> Src; 2] = [
            |m| ingress(m, &["api.example.com"], &[LB_IP]),
            |m| service(m, "LoadBalancer", &[LB_IP]),
        ];
        for build in builders {
            let mut h = Harness::new().await;
            h.default_ips = vec![DEFAULT_IP.to_string()];
            h.run(&build(&meta("web").ann(ANNOTATION_IP, ANNOTATION_IP_VALUE)))
                .await
                .expect("reconcile");
            assert_eq!(ips_of(&only_record(&h)), vec![ANNOTATION_IP_VALUE]);

            let mut h = Harness::new().await;
            h.default_ips = vec![DEFAULT_IP.to_string()];
            h.run(&build(&meta("web"))).await.expect("reconcile");
            assert_eq!(ips_of(&only_record(&h)), vec![DEFAULT_IP]);

            let h = Harness::new().await;
            h.run(&build(&meta("web"))).await.expect("reconcile");
            assert_eq!(ips_of(&only_record(&h)), vec![LB_IP]);
        }
    }

    #[tokio::test]
    async fn route_ips_prefer_annotation_then_gateway_then_defaults() {
        let builders: [fn(&Meta) -> Src; 4] = [
            |m| httproute(m, &["api.example.com"], Some("gw")),
            |m| grpcroute(m, &["api.example.com"], Some("gw")),
            |m| tlsroute(m, &["api.example.com"], Some("gw")),
            |m| tcproute(&m.clone().ann(ANNOTATION_RECORD_NAME, "api"), Some("gw")),
        ];
        for build in builders {
            let mut h = Harness::new().await;
            h.default_ips = vec![DEFAULT_IP.to_string()];
            h.state.lock().expect("lock").gateways.insert(
                format!("{SRC_NS}/gw"),
                gateway_json("gw", &[("IPAddress", GATEWAY_IP)]),
            );
            h.run(&build(&meta("web").ann(ANNOTATION_IP, ANNOTATION_IP_VALUE)))
                .await
                .expect("reconcile");
            assert_eq!(ips_of(&only_record(&h)), vec![ANNOTATION_IP_VALUE]);

            h.state.lock().expect("lock").arecords.clear();
            h.run(&build(&meta("web"))).await.expect("reconcile");
            assert_eq!(ips_of(&only_record(&h)), vec![GATEWAY_IP]);

            // No such Gateway (404): fall back to the defaults.
            let mut h = Harness::new().await;
            h.default_ips = vec![DEFAULT_IP.to_string()];
            h.run(&build(&meta("web"))).await.expect("reconcile");
            assert_eq!(ips_of(&only_record(&h)), vec![DEFAULT_IP]);
        }
    }

    #[tokio::test]
    async fn a_gateway_without_addresses_uses_its_mapped_service() {
        let mut h = Harness::new().await;
        h.gateway_services.insert(
            GATEWAY_CLASS.to_string(),
            GatewayServiceTarget::Name(NamespacedName {
                namespace: SRC_NS.to_string(),
                name: "traefik".to_string(),
            }),
        );
        {
            let mut s = h.state.lock().expect("lock");
            s.gateways
                .insert(format!("{SRC_NS}/gw"), gateway_json("gw", &[]));
            s.services.insert(
                format!("{SRC_NS}/traefik"),
                service_json(&meta("traefik"), "LoadBalancer", &[GATEWAY_SERVICE_IP]),
            );
        }
        h.run(&httproute(&meta("web"), &["api.example.com"], Some("gw")))
            .await
            .expect("reconcile");
        assert_eq!(ips_of(&only_record(&h)), vec![GATEWAY_SERVICE_IP]);
    }

    /// The label selector of [`labelled_gateway_target`].
    const GATEWAY_SELECTOR_KEY: &str = "app";
    const GATEWAY_SELECTOR_VALUE: &str = "traefik";

    /// Maps the gateway class to every Service in `SRC_NS` labelled
    /// `app=traefik` (issue #569).
    fn labelled_gateway_target() -> GatewayServiceTarget {
        GatewayServiceTarget::Labeled {
            namespace: SRC_NS.to_string(),
            selector: format!("{GATEWAY_SELECTOR_KEY}={GATEWAY_SELECTOR_VALUE}"),
        }
    }

    /// A Service fixture carrying `app=<app>`.
    fn labelled_service_json(name: &str, app: &str, svc_type: &str, lb_ips: &[&str]) -> Value {
        let mut svc = service_json(&meta(name), svc_type, lb_ips);
        svc["metadata"]["labels"] = json!({ GATEWAY_SELECTOR_KEY: app });
        svc
    }

    #[tokio::test]
    async fn a_labelled_gateway_target_publishes_every_matching_service() {
        const VIP_A: &str = "10.4.4.1";
        const VIP_B: &str = "10.4.4.2";
        const VIP_C: &str = "10.4.4.3";
        const INTERNAL_IP: &str = "10.4.4.9";
        const UNMATCHED_IP: &str = "10.4.4.8";
        let mut h = Harness::new().await;
        h.gateway_services
            .insert(GATEWAY_CLASS.to_string(), labelled_gateway_target());
        {
            let mut s = h.state.lock().expect("lock");
            s.gateways
                .insert(format!("{SRC_NS}/gw"), gateway_json("gw", &[]));
            for (name, app, svc_type, ips) in [
                ("vip-a", GATEWAY_SELECTOR_VALUE, "LoadBalancer", vec![VIP_A]),
                (
                    "vip-b",
                    GATEWAY_SELECTOR_VALUE,
                    "LoadBalancer",
                    vec![VIP_B, VIP_C],
                ),
                (
                    "vip-pending",
                    GATEWAY_SELECTOR_VALUE,
                    "LoadBalancer",
                    vec![],
                ),
                (
                    "internal",
                    GATEWAY_SELECTOR_VALUE,
                    "ClusterIP",
                    vec![INTERNAL_IP],
                ),
                // A LoadBalancer with an address that the selector does not
                // match: its address must never be published.
                ("other-lb", "other", "LoadBalancer", vec![UNMATCHED_IP]),
            ] {
                s.services.insert(
                    format!("{SRC_NS}/{name}"),
                    labelled_service_json(name, app, svc_type, &ips),
                );
            }
        }
        h.run(&httproute(&meta("web"), &["api.example.com"], Some("gw")))
            .await
            .expect("reconcile");
        assert_eq!(ips_of(&only_record(&h)), vec![VIP_A, VIP_B, VIP_C]);
        assert!(h.log().contains(&format!(
            "LIST services {SRC_NS} {GATEWAY_SELECTOR_KEY}={GATEWAY_SELECTOR_VALUE}"
        )));
    }

    #[tokio::test]
    async fn a_labelled_gateway_target_list_error_fails_the_reconcile() {
        let mut h = Harness::new().await;
        h.gateway_services
            .insert(GATEWAY_CLASS.to_string(), labelled_gateway_target());
        {
            let mut s = h.state.lock().expect("lock");
            s.gateways
                .insert(format!("{SRC_NS}/gw"), gateway_json("gw", &[]));
            s.service_status = Some(500);
        }
        let result = h
            .run(&httproute(&meta("web"), &["api.example.com"], Some("gw")))
            .await;
        assert!(result.is_err(), "got {result:?}");
        assert!(h.applied().is_empty());
    }

    #[tokio::test]
    async fn no_ip_from_any_source_publishes_nothing() {
        for src in [
            ingress(&meta("web"), &["api.example.com"], &[]),
            service(&meta("web"), "LoadBalancer", &[]),
            httproute(&meta("web"), &["api.example.com"], None),
            grpcroute(&meta("web"), &["api.example.com"], None),
        ] {
            let h = Harness::new().await;
            let action = h.run(&src).await.expect("reconcile");
            assert!(requeued(&action), "{}", src.kind());
            assert!(h.applied().is_empty(), "{}", src.kind());
        }
    }

    // =====================================================================
    // Pinned behaviour: deletion, grace period, stale clusters, pruning
    // =====================================================================

    #[tokio::test]
    async fn deletion_removes_the_records_and_the_finalizer() {
        for src in every_kind(&meta("web").deleting(1)) {
            let h = Harness::new().await;
            h.seed(record("own", CLUSTER, "web", ZONE, Some(src.kind())));
            let action = h.run(&src).await.expect("reconcile");
            assert_eq!(action, Action::await_change());
            assert_eq!(h.deleted(), vec!["own"], "{}", src.kind());
            assert_eq!(h.finalizer_ops(), vec!["remove"], "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_failed_cleanup_within_the_grace_period_keeps_the_finalizer() {
        for src in every_kind(&meta("web").deleting(1)) {
            let h = Harness::new().await;
            h.state.lock().expect("lock").arecord_list_status = Some(500);
            let action = h.run(&src).await.expect("reconcile");
            assert!(requeued(&action), "{}", src.kind());
            assert!(h.finalizer_ops().is_empty(), "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_failed_cleanup_past_the_grace_period_releases_the_finalizer() {
        const PAST_GRACE_SECS: i64 = 400;
        for src in every_kind(&meta("web").deleting(PAST_GRACE_SECS)) {
            let h = Harness::new().await;
            h.state.lock().expect("lock").arecord_list_status = Some(500);
            let action = h.run(&src).await.expect("reconcile");
            assert_eq!(action, Action::await_change(), "{}", src.kind());
            assert_eq!(h.finalizer_ops(), vec!["remove"], "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn records_from_a_previous_cluster_name_in_the_same_zone_are_deleted() {
        for src in every_kind(&meta("web")) {
            let h = Harness::new().await;
            h.seed(record("stale", "old-prod", "web", ZONE, Some(src.kind())));
            h.seed(record(
                "other-zone",
                "north",
                "web",
                "other.org",
                Some(src.kind()),
            ));
            h.run(&src).await.expect("reconcile");
            assert_eq!(h.deleted(), vec!["stale"], "{}", src.kind());
            assert!(h.record_names().contains(&"other-zone".to_string()));
        }
    }

    // =====================================================================
    // ADR-0025: zone and address allowlists, refused deletes
    // =====================================================================

    #[tokio::test]
    async fn a_zone_outside_the_allowed_zones_publishes_nothing() {
        for src in every_kind(&meta("web")) {
            let mut h = Harness::new().await;
            h.allowed_zones = Some(parse_allowed_zones("other.org").expect("zones"));
            let action = h.run(&src).await.expect("reconcile");
            assert_eq!(action, Action::await_change(), "{}", src.kind());
            assert!(h.applied().is_empty(), "{}", src.kind());
            assert!(
                !h.log().iter().any(|l| l.starts_with("GET dnszone")),
                "{}: the refusal comes before the zone lookup",
                src.kind()
            );
        }
    }

    #[tokio::test]
    async fn a_refused_zone_keeps_the_records_already_published() {
        for src in every_kind(&meta("web")) {
            let mut h = Harness::new().await;
            h.allowed_zones = Some(parse_allowed_zones("other.org").expect("zones"));
            h.seed(record("own", CLUSTER, "web", ZONE, Some(src.kind())));
            h.run(&src).await.expect("reconcile");
            assert!(h.deleted().is_empty(), "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_zone_inside_the_allowed_zones_publishes() {
        for src in every_kind(&meta("web")) {
            let mut h = Harness::new().await;
            h.allowed_zones = Some(parse_allowed_zones("other.org, Example.com").expect("zones"));
            h.run(&src).await.expect("reconcile");
            assert_eq!(h.applied().len(), 1, "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn an_address_outside_the_allowed_ranges_publishes_nothing() {
        for src in every_kind(&meta("web")) {
            let mut h = Harness::new().await;
            h.allowed_ip_ranges = Some(parse_allowed_ip_ranges("10.0.0.0/24").expect("ranges"));
            let action = h.run(&src).await.expect("reconcile");
            assert!(requeued(&action), "{}", src.kind());
            assert!(h.applied().is_empty(), "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_discovered_address_outside_the_allowed_ranges_publishes_nothing() {
        let mut h = Harness::new().await;
        h.allowed_ip_ranges = Some(parse_allowed_ip_ranges("192.0.2.0/24").expect("ranges"));
        let action = h
            .run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        assert!(requeued(&action));
        assert!(h.applied().is_empty());
    }

    #[tokio::test]
    async fn addresses_inside_the_allowed_ranges_publish() {
        for src in every_kind(&meta("web")) {
            let mut h = Harness::new().await;
            h.allowed_ip_ranges = Some(parse_allowed_ip_ranges("10.9.9.0/24").expect("ranges"));
            h.run(&src).await.expect("reconcile");
            assert_eq!(h.applied().len(), 1, "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_refused_delete_of_another_clusters_record_is_skipped_on_publish() {
        for code in [403, 422] {
            for src in every_kind(&meta("web")) {
                let h = Harness::new().await;
                h.seed(record("stale", "old-prod", "web", ZONE, Some(src.kind())));
                h.refuse_delete("stale", code);
                let action = h
                    .run(&src)
                    .await
                    .expect("a refusal does not fail the reconcile");
                assert_eq!(action, Action::await_change(), "{} {code}", src.kind());
                assert_eq!(h.refused(), vec!["stale"], "{} {code}", src.kind());
                assert_eq!(h.applied().len(), 1, "{} {code}", src.kind());
            }
        }
    }

    #[tokio::test]
    async fn a_refused_delete_of_another_clusters_record_does_not_hold_the_finalizer() {
        for src in every_kind(&meta("web").deleting(1)) {
            let h = Harness::new().await;
            h.seed(record("own", CLUSTER, "web", ZONE, Some(src.kind())));
            h.seed(record("stale", "old-prod", "web", ZONE, Some(src.kind())));
            h.refuse_delete("stale", 422);
            let action = h.run(&src).await.expect("reconcile");
            assert_eq!(action, Action::await_change(), "{}", src.kind());
            assert_eq!(h.deleted(), vec!["own"], "{}", src.kind());
            assert_eq!(h.finalizer_ops(), vec!["remove"], "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_refused_delete_of_the_sources_own_record_still_fails() {
        let h = Harness::new().await;
        h.seed(record(
            "scout-prod-team-a-web-1",
            CLUSTER,
            "web",
            ZONE,
            Some("Ingress"),
        ));
        h.refuse_delete("scout-prod-team-a-web-1", 422);
        let result = h
            .run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await;
        assert!(
            result.is_err(),
            "pruning its own record must not be skipped"
        );
    }

    #[tokio::test]
    async fn a_host_dropped_from_the_source_is_pruned() {
        for src in [
            ingress(&meta("web"), &["api.example.com"], &[LB_IP]),
            httproute(
                &meta("web").ann(ANNOTATION_IP, ANNOTATION_IP_VALUE),
                &["api.example.com"],
                None,
            ),
        ] {
            let h = Harness::new().await;
            h.seed(record(
                "scout-prod-team-a-web-1",
                CLUSTER,
                "web",
                ZONE,
                Some(src.kind()),
            ));
            h.run(&src).await.expect("reconcile");
            assert_eq!(
                h.deleted(),
                vec!["scout-prod-team-a-web-1"],
                "{}",
                src.kind()
            );
            assert_eq!(spec_names(&h.records()), vec!["api"]);
        }
    }

    // =====================================================================
    // New behaviour (ADR-0022): source-kind label and unambiguous names
    // =====================================================================

    #[tokio::test]
    async fn every_record_lists_its_source_as_a_contributor() {
        for src in every_kind(&meta("web")) {
            let h = Harness::new().await;
            h.run(&src).await.expect("reconcile");
            let r = only_record(&h);
            assert_eq!(
                contributors(&r).keys().collect::<Vec<_>>(),
                vec![&contributor_key(src.kind(), SRC_NS, "web")],
                "{}",
                src.kind()
            );
            assert!(
                labels_of(&r).contains_key(&contributor_label(src.kind(), SRC_NS, "web")),
                "{}",
                src.kind()
            );
        }
    }

    #[tokio::test]
    async fn a_record_is_named_from_its_cluster_zone_and_dns_name() {
        for src in every_kind(&meta("web")) {
            let h = Harness::new().await;
            h.run(&src).await.expect("reconcile");
            // A Service publishes its own name; every other kind here "api".
            let dns_name = if src.kind() == "Service" {
                "web"
            } else {
                "api"
            };
            assert_eq!(
                h.record_names(),
                vec![expected_name(dns_name)],
                "{}",
                src.kind()
            );
        }
    }

    #[tokio::test]
    async fn sources_of_different_kinds_that_share_a_name_share_one_record() {
        let h = Harness::new().await;
        for src in every_kind(&meta("web")) {
            h.run(&src).await.expect("reconcile");
        }
        let mut names = h.record_names();
        names.sort();
        let mut expected = vec![expected_name("api"), expected_name("web")];
        expected.sort();
        assert_eq!(names, expected);
        let mut keys: Vec<String> = contributors(&h.record(&expected_name("api")))
            .into_keys()
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            ["GRPCRoute", "HTTPRoute", "Ingress", "TCPRoute", "TLSRoute"]
                .map(|k| contributor_key(k, SRC_NS, "web"))
        );
        // Reconciling every kind again changes nothing.
        h.clear_log();
        for src in every_kind(&meta("web")) {
            h.run(&src).await.expect("reconcile");
        }
        assert!(h.deleted().is_empty(), "deleted {:?}", h.deleted());
        assert!(h.applied().is_empty(), "wrote {:?}", h.applied());
    }

    #[tokio::test]
    async fn namespaces_whose_joined_names_collide_keep_separate_records() {
        let mut h = Harness::without_zone().await;
        h.add_zone(SRC_NS, None);
        h.add_zone("team", None);
        let a = ingress(&meta("web").ns("team-a"), &["x.example.com"], &[LB_IP]);
        let b = ingress(&meta("a-web").ns("team"), &["y.example.com"], &[LB_IP]);
        h.run(&a).await.expect("reconcile");
        h.run(&b).await.expect("reconcile");
        assert_eq!(spec_names(&h.records()), vec!["x", "y"]);
    }

    #[tokio::test]
    async fn releasing_a_service_leaves_a_same_named_ingress_alone() {
        let h = Harness::new().await;
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        h.run(&service(&meta("web"), "LoadBalancer", &[LB_IP]))
            .await
            .expect("reconcile");
        h.run(&service(&meta("web"), "ClusterIP", &[]))
            .await
            .expect("reconcile");
        assert_eq!(h.record_names(), vec![expected_name("api")]);
    }

    #[tokio::test]
    async fn a_legacy_record_is_deleted_only_after_its_replacement_is_applied() {
        let h = Harness::new().await;
        h.seed(record(
            "scout-prod-team-a-web-0",
            CLUSTER,
            "web",
            ZONE,
            None,
        ));
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");

        let new_name = expected_name("api");
        let log = h.log();
        let applied_at = log
            .iter()
            .position(|l| *l == format!("CREATE {new_name}"))
            .expect("new record created");
        let deleted_at = log
            .iter()
            .position(|l| l == "DELETE scout-prod-team-a-web-0")
            .expect("legacy record deleted");
        assert!(
            applied_at < deleted_at,
            "apply must precede delete: {log:?}"
        );
        assert_eq!(h.record_names(), vec![new_name]);
    }

    #[tokio::test]
    async fn a_legacy_record_is_kept_until_its_replacement_is_served() {
        let h = Harness::new().await;
        h.set_records_served(false);
        h.seed(record(
            "scout-prod-team-a-web-0",
            CLUSTER,
            "web",
            ZONE,
            None,
        ));
        let src = ingress(&meta("web"), &["api.example.com"], &[LB_IP]);

        let action = h.run(&src).await.expect("reconcile");
        assert!(h.deleted().is_empty(), "deleted {:?}", h.deleted());
        assert!(requeued(&action), "the handover must be rechecked");

        h.set_records_served(true);
        let action = h.run(&src).await.expect("reconcile");
        assert_eq!(h.deleted(), vec!["scout-prod-team-a-web-0"]);
        assert!(!requeued(&action), "nothing left to hand over");
    }

    #[tokio::test]
    async fn a_previous_cluster_names_record_is_kept_until_its_replacement_is_served() {
        for src in every_kind(&meta("web")) {
            let h = Harness::new().await;
            h.set_records_served(false);
            h.seed(record("stale", "old-prod", "web", ZONE, Some(src.kind())));
            h.seed(record("legacy-stale", "old-prod", "web", ZONE, None));

            let action = h.run(&src).await.expect("reconcile");
            assert!(h.deleted().is_empty(), "{}: {:?}", src.kind(), h.deleted());
            assert!(requeued(&action), "{}", src.kind());

            h.set_records_served(true);
            h.run(&src).await.expect("reconcile");
            let mut deleted = h.deleted();
            deleted.sort();
            assert_eq!(deleted, vec!["legacy-stale", "stale"], "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn an_unserved_record_with_nothing_to_hand_over_is_not_rechecked() {
        let h = Harness::new().await;
        h.set_records_served(false);
        let action = h
            .run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        assert!(!requeued(&action), "no legacy or stale record to wait for");
    }

    #[tokio::test]
    async fn a_dropped_host_is_withdrawn_even_while_the_new_records_are_unserved() {
        // Withdrawing removes this source's own entry for a name it no
        // longer declares: no other record takes over that name, so there is
        // nothing to wait for.
        let h = Harness::new().await;
        h.set_records_served(false);
        h.seed(name_record(
            CLUSTER,
            "old",
            &[("Ingress", SRC_NS, "web", &[LB_IP])],
        ));
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        assert_eq!(h.deleted(), vec![expected_name("old")]);
    }

    #[test]
    fn a_record_is_served_only_once_bound_to_its_zone_and_ready() {
        use crate::pipeline::arecord_served;
        let with_status = |status: Value| -> crate::crd::ARecord {
            serde_json::from_value(json!({
                "apiVersion": "bindy.firestoned.io/v1beta1", "kind": "ARecord",
                "metadata": {"name": "r", "namespace": TARGET_NS},
                "spec": {"name": "api", "ipv4Addresses": ["192.0.2.1"]},
                "status": status,
            }))
            .expect("valid ARecord")
        };
        let served = served_status();
        let mut not_ready = served.clone();
        not_ready["conditions"][0]["status"] = json!("False");
        let mut unbound = served.clone();
        unbound.as_object_mut().expect("object").remove("zoneRef");
        let mut no_conditions = served.clone();
        no_conditions["conditions"] = json!([]);

        assert!(arecord_served(&with_status(served)));
        assert!(!arecord_served(&with_status(not_ready)));
        assert!(!arecord_served(&with_status(unbound)));
        assert!(!arecord_served(&with_status(no_conditions)));
        assert!(!arecord_served(&with_status(Value::Null)));
    }

    #[test]
    fn the_handover_waits_only_when_there_is_something_to_replace() {
        use crate::pipeline::{handover_step, Handover};
        assert_eq!(handover_step(0, false), Handover::Nothing);
        assert_eq!(handover_step(0, true), Handover::Nothing);
        assert_eq!(handover_step(2, false), Handover::Wait);
        assert_eq!(handover_step(2, true), Handover::Delete);
    }

    #[tokio::test]
    async fn a_legacy_record_of_another_cluster_or_source_is_kept() {
        let h = Harness::new().await;
        h.seed(record(
            "legacy-other-cluster",
            "north",
            "web",
            "other.org",
            None,
        ));
        h.seed(record("legacy-other-source", CLUSTER, "api", ZONE, None));
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        assert!(h.deleted().is_empty(), "deleted {:?}", h.deleted());
    }

    #[tokio::test]
    async fn a_legacy_record_from_a_previous_cluster_name_is_still_stale() {
        let h = Harness::new().await;
        h.seed(record("legacy-stale", "old-prod", "web", ZONE, None));
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        assert_eq!(h.deleted(), vec!["legacy-stale"]);
    }

    #[tokio::test]
    async fn deleting_a_source_also_removes_its_legacy_records() {
        let h = Harness::new().await;
        h.seed(record(
            "scout-prod-team-a-web-0",
            CLUSTER,
            "web",
            ZONE,
            None,
        ));
        h.run(&ingress(
            &meta("web").deleting(1),
            &["api.example.com"],
            &[LB_IP],
        ))
        .await
        .expect("reconcile");
        assert_eq!(h.deleted(), vec!["scout-prod-team-a-web-0"]);
    }

    // =====================================================================
    // New behaviour (ADR-0022 Decision 6): fail fast on input
    // =====================================================================

    #[tokio::test]
    async fn a_non_ipv4_status_address_is_skipped() {
        let h = Harness::new().await;
        h.run(&ingress(
            &meta("web"),
            &["api.example.com"],
            &["2001:db8::1", LB_IP],
        ))
        .await
        .expect("reconcile");
        assert_eq!(ips_of(&only_record(&h)), vec![LB_IP]);

        let h = Harness::new().await;
        h.run(&service(
            &meta("db"),
            "LoadBalancer",
            &["2001:db8::1", LB_IP],
        ))
        .await
        .expect("reconcile");
        assert_eq!(ips_of(&only_record(&h)), vec![LB_IP]);
    }

    #[tokio::test]
    async fn a_non_ipv4_gateway_address_is_skipped() {
        let h = Harness::new().await;
        h.state.lock().expect("lock").gateways.insert(
            format!("{SRC_NS}/gw"),
            gateway_json(
                "gw",
                &[("IPAddress", "2001:db8::1"), ("IPAddress", GATEWAY_IP)],
            ),
        );
        h.run(&httproute(&meta("web"), &["api.example.com"], Some("gw")))
            .await
            .expect("reconcile");
        assert_eq!(ips_of(&only_record(&h)), vec![GATEWAY_IP]);
    }

    #[tokio::test]
    async fn an_invalid_ttl_publishes_nothing_for_the_source() {
        for ttl in ["abc", "-5", "0", "1.5"] {
            for src in every_kind(&meta("web").ann(ANNOTATION_TTL, ttl)) {
                let h = Harness::new().await;
                let action = h.run(&src).await.expect("reconcile");
                assert!(requeued(&action), "{} ttl={ttl}", src.kind());
                assert!(h.applied().is_empty(), "{} ttl={ttl}", src.kind());
            }
        }
    }

    #[tokio::test]
    async fn an_invalid_record_name_override_publishes_nothing_and_keeps_existing_records() {
        for src in every_kind(&meta("web").ann(ANNOTATION_RECORD_NAME, "a b")) {
            let h = Harness::new().await;
            h.seed(record("own", CLUSTER, "web", ZONE, Some(src.kind())));
            let action = h.run(&src).await.expect("reconcile");
            assert!(requeued(&action), "{}", src.kind());
            assert!(h.applied().is_empty(), "{}", src.kind());
            assert!(h.deleted().is_empty(), "{}", src.kind());
        }
    }

    #[tokio::test]
    async fn a_gateway_lookup_error_fails_the_reconcile() {
        let mut h = Harness::new().await;
        h.default_ips = vec![DEFAULT_IP.to_string()];
        h.state.lock().expect("lock").gateway_status = Some(500);
        let result = h
            .run(&httproute(&meta("web"), &["api.example.com"], Some("gw")))
            .await;
        assert!(
            result.is_err(),
            "a 500 must fail the reconcile, got {result:?}"
        );
        assert!(h.applied().is_empty());
    }

    #[tokio::test]
    async fn a_gateway_service_lookup_error_fails_the_reconcile() {
        let mut h = Harness::new().await;
        h.gateway_services.insert(
            GATEWAY_CLASS.to_string(),
            GatewayServiceTarget::Name(NamespacedName {
                namespace: SRC_NS.to_string(),
                name: "traefik".to_string(),
            }),
        );
        {
            let mut s = h.state.lock().expect("lock");
            s.gateways
                .insert(format!("{SRC_NS}/gw"), gateway_json("gw", &[]));
            s.service_status = Some(500);
        }
        let result = h
            .run(&httproute(&meta("web"), &["api.example.com"], Some("gw")))
            .await;
        assert!(result.is_err(), "got {result:?}");
    }

    #[tokio::test]
    async fn an_ignored_object_costs_no_api_call() {
        let mut h = Harness::new().await;
        h.namespace_selector = Some("scout=on".to_string());
        for src in every_kind(&meta("web").without(ANNOTATION_SCOUT_ENABLED).unfinalized()) {
            h.run(&src).await.expect("reconcile");
        }
        h.run(&service(&meta("db").unfinalized(), "ClusterIP", &[]))
            .await
            .expect("reconcile");
        assert!(h.log().is_empty(), "got {:?}", h.log());
    }

    #[tokio::test]
    async fn an_opted_in_object_still_checks_its_namespace() {
        let mut h = Harness::new().await;
        h.namespace_selector = Some("scout=on".to_string());
        h.state
            .lock()
            .expect("lock")
            .eligible_namespaces
            .push(SRC_NS.to_string());
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        assert!(h.log().contains(&"LIST namespaces".to_string()));
        assert_eq!(h.records().len(), 1);
    }

    // =====================================================================
    // ADR-0026: one record per DNS name (issue #556)
    // =====================================================================

    const VIP_A: &str = "10.0.0.246";
    const VIP_B: &str = "10.0.0.241";
    const VIP_C: &str = "10.0.0.240";

    /// Publishes the three Services of issue #556 at the apex.
    async fn publish_three_apex_services(h: &Harness) {
        for (name, ip) in [("svc-a", VIP_A), ("svc-b", VIP_B), ("svc-c", VIP_C)] {
            h.run(&apex_service(name, ip)).await.expect("reconcile");
        }
    }

    #[tokio::test]
    async fn sources_sharing_a_name_publish_one_record_with_every_address() {
        let h = Harness::new().await;
        publish_three_apex_services(&h).await;

        let r = only_record(&h);
        assert_eq!(
            r.pointer("/metadata/name"),
            Some(&json!(expected_name("@")))
        );
        assert_eq!(r.pointer("/spec/name"), Some(&json!("@")));
        assert_eq!(ips_of(&r), vec![VIP_C, VIP_B, VIP_A], "sorted numerically");
        let mut keys: Vec<String> = contributors(&r).into_keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["svc-a", "svc-b", "svc-c"].map(|n| contributor_key("Service", SRC_NS, n))
        );
        for n in ["svc-a", "svc-b", "svc-c"] {
            assert!(labels_of(&r).contains_key(&contributor_label("Service", SRC_NS, n)));
        }
    }

    #[tokio::test]
    async fn every_write_after_the_first_is_conditional_on_the_version_read() {
        let h = Harness::new().await;
        publish_three_apex_services(&h).await;
        let name = expected_name("@");
        assert_eq!(
            h.applied(),
            vec![name.clone(), name.clone(), name],
            "one create, then two replaces, never an apply"
        );
        assert!(
            !h.log().iter().any(|l| l.starts_with("UNEXPECTED")),
            "{:?}",
            h.log()
        );
    }

    #[tokio::test]
    async fn a_source_leaving_a_shared_name_removes_only_its_own_addresses() {
        let h = Harness::new().await;
        publish_three_apex_services(&h).await;
        h.clear_log();

        let gone = service(
            &meta("svc-b").ann(ANNOTATION_RECORD_NAME, "@").deleting(1),
            "LoadBalancer",
            &[VIP_B],
        );
        h.run(&gone).await.expect("reconcile");

        assert!(h.deleted().is_empty(), "the name stays: {:?}", h.log());
        let r = only_record(&h);
        assert_eq!(ips_of(&r), vec![VIP_C, VIP_A]);
        assert!(!contributors(&r).contains_key(&contributor_key("Service", SRC_NS, "svc-b")));
        assert!(!labels_of(&r).contains_key(&contributor_label("Service", SRC_NS, "svc-b")));
        assert_eq!(h.finalizer_ops(), vec!["remove"]);
    }

    #[tokio::test]
    async fn the_last_source_leaving_a_name_deletes_its_record() {
        let h = Harness::new().await;
        h.run(&apex_service("svc-a", VIP_A))
            .await
            .expect("reconcile");
        let opted_out = service(
            &meta("svc-a")
                .ann(ANNOTATION_RECORD_NAME, "@")
                .without(ANNOTATION_SCOUT_ENABLED),
            "LoadBalancer",
            &[VIP_A],
        );
        h.run(&opted_out).await.expect("reconcile");
        assert_eq!(h.deleted(), vec![expected_name("@")]);
        assert!(h.records().is_empty());
    }

    #[tokio::test]
    async fn an_address_change_rewrites_only_that_sources_entry() {
        let h = Harness::new().await;
        publish_three_apex_services(&h).await;
        h.run(&apex_service("svc-a", "10.0.0.250"))
            .await
            .expect("reconcile");
        assert_eq!(ips_of(&only_record(&h)), vec![VIP_C, VIP_B, "10.0.0.250"]);
    }

    #[tokio::test]
    async fn nothing_is_written_when_the_set_is_unchanged() {
        let h = Harness::new().await;
        publish_three_apex_services(&h).await;
        h.clear_log();
        publish_three_apex_services(&h).await;
        assert!(h.applied().is_empty(), "wrote {:?}", h.log());
        assert!(h.deleted().is_empty());
    }

    #[tokio::test]
    async fn contributors_with_different_ttls_publish_the_lowest() {
        let h = Harness::new().await;
        for (name, ip, ttl) in [("svc-a", VIP_A, "300"), ("svc-b", VIP_B, "60")] {
            let src = service(
                &meta(name)
                    .ann(ANNOTATION_RECORD_NAME, "@")
                    .ann(ANNOTATION_TTL, ttl),
                "LoadBalancer",
                &[ip],
            );
            h.run(&src).await.expect("reconcile");
        }
        h.run(&apex_service("svc-c", VIP_C))
            .await
            .expect("reconcile");
        assert_eq!(only_record(&h).pointer("/spec/ttl"), Some(&json!(60)));
    }

    #[tokio::test]
    async fn a_lost_race_is_retried_and_keeps_the_concurrent_contributor() {
        let h = Harness::new().await;
        h.run(&apex_service("svc-a", VIP_A))
            .await
            .expect("reconcile");
        // While svc-c reconciles, svc-b's reconcile writes first.
        h.race_next_write(name_record(
            CLUSTER,
            "@",
            &[
                ("Service", SRC_NS, "svc-a", &[VIP_A]),
                ("Service", SRC_NS, "svc-b", &[VIP_B]),
            ],
        ));
        h.run(&apex_service("svc-c", VIP_C))
            .await
            .expect("reconcile");

        assert!(
            h.log().iter().any(|l| l.starts_with("CONFLICT")),
            "{:?}",
            h.log()
        );
        assert_eq!(ips_of(&only_record(&h)), vec![VIP_C, VIP_B, VIP_A]);
    }

    #[tokio::test]
    async fn a_lost_race_on_the_first_create_is_retried_as_a_replace() {
        let h = Harness::new().await;
        h.race_next_write(name_record(
            CLUSTER,
            "@",
            &[("Service", SRC_NS, "svc-b", &[VIP_B])],
        ));
        h.run(&apex_service("svc-a", VIP_A))
            .await
            .expect("reconcile");
        assert_eq!(ips_of(&only_record(&h)), vec![VIP_B, VIP_A]);
    }

    #[tokio::test]
    async fn the_hosts_of_one_source_that_share_a_name_write_it_once() {
        let h = Harness::new().await;
        let src = ingress(
            &meta("web").ann(ANNOTATION_RECORD_NAME, "@"),
            &["a.example.com", "b.example.com"],
            &[LB_IP],
        );
        h.run(&src).await.expect("reconcile");
        assert_eq!(h.applied(), vec![expected_name("@")]);
        assert_eq!(contributors(&only_record(&h)).len(), 1);
    }

    #[tokio::test]
    async fn a_host_dropped_from_a_shared_name_keeps_the_other_contributors() {
        let h = Harness::new().await;
        h.seed(name_record(
            CLUSTER,
            "old",
            &[
                ("Ingress", SRC_NS, "web", &[LB_IP]),
                ("Service", SRC_NS, "other", &[VIP_A]),
            ],
        ));
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        let old = h.record(&expected_name("old"));
        assert_eq!(ips_of(&old), vec![VIP_A]);
        assert_eq!(
            contributors(&old).keys().collect::<Vec<_>>(),
            vec![&contributor_key("Service", SRC_NS, "other")]
        );
    }

    #[tokio::test]
    async fn a_per_source_record_from_before_adr_0026_is_handed_over() {
        let h = Harness::new().await;
        h.set_records_served(false);
        h.seed(record(
            "per-source",
            CLUSTER,
            "svc-a",
            ZONE,
            Some("Service"),
        ));
        let src = apex_service("svc-a", VIP_A);

        let action = h.run(&src).await.expect("reconcile");
        assert!(h.deleted().is_empty(), "deleted {:?}", h.deleted());
        assert!(requeued(&action), "the handover must be rechecked");

        h.set_records_served(true);
        h.run(&src).await.expect("reconcile");
        assert_eq!(h.deleted(), vec!["per-source"]);
        assert_eq!(h.record_names(), vec![expected_name("@")]);
    }

    #[tokio::test]
    async fn a_previous_cluster_names_shared_record_loses_only_this_sources_entry() {
        let h = Harness::new().await;
        let stale = name_record(
            "old-prod",
            "@",
            &[
                ("Service", SRC_NS, "svc-a", &[VIP_A]),
                ("Service", SRC_NS, "svc-b", &[VIP_B]),
            ],
        );
        let stale_name = expected_name_in("old-prod", "@");
        h.seed(stale);
        h.run(&apex_service("svc-a", VIP_A))
            .await
            .expect("reconcile");

        let left = h.record(&stale_name);
        assert_eq!(
            contributors(&left).keys().collect::<Vec<_>>(),
            vec![&contributor_key("Service", SRC_NS, "svc-b")]
        );
        assert_eq!(ips_of(&left), vec![VIP_B]);
        assert!(h.record_names().contains(&expected_name("@")));
    }

    #[tokio::test]
    async fn a_refused_withdrawal_from_another_clusters_record_is_skipped() {
        for code in [403, 422] {
            let h = Harness::new().await;
            h.seed(name_record(
                "old-prod",
                "@",
                &[
                    ("Service", SRC_NS, "svc-a", &[VIP_A]),
                    ("Service", SRC_NS, "svc-b", &[VIP_B]),
                ],
            ));
            h.refuse_write(&expected_name_in("old-prod", "@"), code);
            let action = h
                .run(&apex_service("svc-a", VIP_A))
                .await
                .expect("a refusal does not fail the reconcile");
            assert_eq!(action, Action::await_change(), "{code}");
            assert!(h.record_names().contains(&expected_name("@")), "{code}");
        }
    }

    #[tokio::test]
    async fn a_refused_withdrawal_from_the_sources_own_record_still_fails() {
        let h = Harness::new().await;
        h.seed(name_record(
            CLUSTER,
            "old",
            &[
                ("Ingress", SRC_NS, "web", &[LB_IP]),
                ("Service", SRC_NS, "other", &[VIP_A]),
            ],
        ));
        h.refuse_write(&expected_name("old"), 422);
        let result = h
            .run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await;
        assert!(
            result.is_err(),
            "withdrawing from its own record must not be skipped"
        );
    }

    #[tokio::test]
    async fn deleting_a_source_withdraws_it_from_every_name_it_publishes() {
        let h = Harness::new().await;
        h.seed(name_record(
            CLUSTER,
            "api",
            &[
                ("Ingress", SRC_NS, "web", &[LB_IP]),
                ("Service", SRC_NS, "other", &[VIP_A]),
            ],
        ));
        h.seed(name_record(
            CLUSTER,
            "www",
            &[("Ingress", SRC_NS, "web", &[LB_IP])],
        ));
        h.run(&ingress(
            &meta("web").deleting(1),
            &["api.example.com", "www.example.com"],
            &[LB_IP],
        ))
        .await
        .expect("reconcile");
        assert_eq!(h.deleted(), vec![expected_name("www")]);
        assert_eq!(ips_of(&h.record(&expected_name("api"))), vec![VIP_A]);
        assert_eq!(h.finalizer_ops(), vec!["remove"]);
    }

    #[tokio::test]
    async fn a_malformed_contributor_map_fails_the_reconcile() {
        let h = Harness::new().await;
        let mut broken = name_record(CLUSTER, "@", &[("Service", SRC_NS, "svc-b", &[VIP_B])]);
        broken["metadata"]["annotations"][ANNOTATION_CONTRIBUTORS] = json!("{not json");
        h.seed(broken);
        let result = h.run(&apex_service("svc-a", VIP_A)).await;
        assert!(result.is_err(), "got {result:?}");
        assert!(h.applied().is_empty());
    }

    // =====================================================================
    // ADR-0002 addendum: Scout stops promptly on SIGTERM
    // =====================================================================

    #[tokio::test]
    async fn a_source_controller_stops_when_the_shutdown_fires() {
        use bindy_controller_sdk::namespace_scope::NamespaceScope;
        use bindy_controller_sdk::shutdown;
        use bindy_controller_sdk::watch::WatchSet;
        // With the Recreate strategy the new Scout starts only once the old
        // pod is gone; a Scout that ignores SIGTERM holds that gap open for
        // the whole termination grace period.
        let h = Harness::new().await;
        let mut ws = WatchSet::new(h.client.clone(), NamespaceScope::All);
        let _services = ws.register::<Service>("Service");
        let (trigger, signal) = shutdown::channel();
        let controller = crate::pipeline::source_controller::<Service>(&ws, h.ctx(), signal);
        trigger.fire();
        tokio::time::timeout(std::time::Duration::from_secs(5), controller)
            .await
            .expect("the controller ends once the shutdown fires");
    }
}
