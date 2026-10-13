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
    use crate::scout::{GatewayServiceTarget, HTTPRoute, NamespacedName, TCPRoute, TLSRoute};
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
    use std::sync::{Arc, Mutex};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    /// The source-kind label (ADR-0022). Spelled out here so a rename in
    /// `scout.rs` fails these tests instead of moving in lockstep.
    const LABEL_SOURCE_KIND: &str = "bindy.firestoned.io/source-kind";

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
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", ns, "arecords", name],
                ) => {
                    let mut body: Value =
                        serde_json::from_slice(&req.body).expect("apply body is JSON");
                    body["metadata"]["namespace"] = json!(ns);
                    let status = if self.records_unserved {
                        self.arecords
                            .get(*name)
                            .and_then(|o| o.get("status"))
                            .cloned()
                    } else {
                        Some(served_status())
                    };
                    if let Some(status) = status {
                        body["status"] = status;
                    }
                    self.log.push(format!("APPLY {name}"));
                    self.arecords.insert((*name).to_string(), body.clone());
                    ResponseTemplate::new(200).set_body_json(body)
                }
                (
                    "DELETE",
                    ["apis", "bindy.firestoned.io", "v1beta1", "namespaces", _, "arecords", name],
                ) => {
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
            }
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
                zone_store: store,
            })
        }

        fn seed(&self, record: Value) {
            let name = record
                .pointer("/metadata/name")
                .and_then(Value::as_str)
                .expect("seeded record has a name")
                .to_string();
            self.state
                .lock()
                .expect("lock")
                .arecords
                .insert(name, record);
        }

        fn log(&self) -> Vec<String> {
            self.state.lock().expect("lock").log.clone()
        }

        /// Whether the operator has bound the records Scout applies (see
        /// [`FakeState::records_unserved`]).
        fn set_records_served(&self, served: bool) {
            self.state.lock().expect("lock").records_unserved = !served;
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

        fn applied(&self) -> Vec<String> {
            self.log()
                .iter()
                .filter_map(|l| l.strip_prefix("APPLY ").map(ToString::to_string))
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
        Tls(TLSRoute),
        Tcp(TCPRoute),
    }

    impl Src {
        fn json(&self) -> Value {
            match self {
                Src::Ingress(o) => serde_json::to_value(o),
                Src::Service(o) => serde_json::to_value(o),
                Src::Http(o) => serde_json::to_value(o),
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
            assert_eq!(labels[LABEL_SOURCE_NAMESPACE], json!(SRC_NS));
            assert_eq!(labels[LABEL_SOURCE_NAME], json!("web"));
            assert_eq!(labels[LABEL_ZONE], json!(ZONE));
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
        let builders: [fn(&Meta) -> Src; 3] = [
            |m| httproute(m, &["api.example.com"], Some("gw")),
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

    #[tokio::test]
    async fn no_ip_from_any_source_publishes_nothing() {
        for src in [
            ingress(&meta("web"), &["api.example.com"], &[]),
            service(&meta("web"), "LoadBalancer", &[]),
            httproute(&meta("web"), &["api.example.com"], None),
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
    async fn every_record_carries_its_source_kind_label() {
        for src in every_kind(&meta("web")) {
            let h = Harness::new().await;
            h.run(&src).await.expect("reconcile");
            assert_eq!(
                labels_of(&only_record(&h)).get(LABEL_SOURCE_KIND),
                Some(&json!(src.kind())),
                "{}",
                src.kind()
            );
        }
    }

    /// The name the ADR specifies, computed independently of `scout.rs`.
    fn expected_name(kind: &str, namespace: &str, name: &str, index: usize) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(format!("{CLUSTER}\0{kind}\0{namespace}\0{name}").as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("scout-{CLUSTER}-{namespace}-{name}-{}-{index}", &hex[..8])
    }

    #[tokio::test]
    async fn a_record_is_named_from_a_readable_prefix_a_hash_and_the_host_index() {
        for src in every_kind(&meta("web")) {
            let h = Harness::new().await;
            h.run(&src).await.expect("reconcile");
            assert_eq!(
                h.record_names(),
                vec![expected_name(src.kind(), SRC_NS, "web", 0)],
                "{}",
                src.kind()
            );
        }
    }

    #[tokio::test]
    async fn same_named_sources_of_different_kinds_keep_separate_records() {
        let h = Harness::new().await;
        for src in every_kind(&meta("web")) {
            h.run(&src).await.expect("reconcile");
        }
        let names = h.record_names();
        assert_eq!(names.len(), 5, "one record per kind, got {names:?}");
        let mut kinds: Vec<String> = h
            .records()
            .iter()
            .filter_map(|r| labels_of(r).get(LABEL_SOURCE_KIND).cloned())
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect();
        kinds.sort();
        assert_eq!(
            kinds,
            vec!["HTTPRoute", "Ingress", "Service", "TCPRoute", "TLSRoute"]
        );
        // Reconciling every kind again prunes nothing of another kind's.
        h.clear_log();
        for src in every_kind(&meta("web")) {
            h.run(&src).await.expect("reconcile");
        }
        assert!(h.deleted().is_empty(), "deleted {:?}", h.deleted());
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
        assert_eq!(
            h.record_names(),
            vec![expected_name("Ingress", SRC_NS, "web", 0)]
        );
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

        let new_name = expected_name("Ingress", SRC_NS, "web", 0);
        let log = h.log();
        let applied_at = log
            .iter()
            .position(|l| *l == format!("APPLY {new_name}"))
            .expect("new record applied");
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
    async fn a_dropped_host_is_pruned_even_while_the_new_records_are_unserved() {
        // Pruning removes this source's own record for a host it no longer
        // declares: no other record takes over that name, so there is
        // nothing to wait for.
        let h = Harness::new().await;
        h.set_records_served(false);
        h.seed(record(
            "scout-prod-team-a-web-1",
            CLUSTER,
            "web",
            ZONE,
            Some("Ingress"),
        ));
        h.run(&ingress(&meta("web"), &["api.example.com"], &[LB_IP]))
            .await
            .expect("reconcile");
        assert_eq!(h.deleted(), vec!["scout-prod-team-a-web-1"]);
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
}
