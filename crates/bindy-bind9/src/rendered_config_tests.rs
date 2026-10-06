// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Every BIND9 configuration the builders render parses (ADR-0013 stage 1).
//!
//! Unit tests elsewhere assert fragments of the rendered text; these assert
//! the assembled `named.conf` and `named.conf.options` are valid BIND9
//! configuration, parsed by hornet, so a template edit that leaves a stray
//! brace or an unterminated statement (bug-177) fails here instead of
//! crash-looping `named`.
//!
//! Stage 3: the files are written by hornet's writer, so each is already in
//! hornet's canonical form and carries nothing through a raw carrier.

#[cfg(test)]
mod tests {
    use crate::bind9_resources::{build_cluster_configmap, build_configmap};
    use crate::crd::{Bind9Cluster, Bind9Instance};
    use hornet_bind9::named_conf::{NamedConf, Statement};
    use k8s_openapi::api::core::v1::ConfigMap;
    use serde_json::{json, Value};

    /// The `named.conf*` files of a ConfigMap.
    fn named_conf_files(configmap: &ConfigMap) -> Vec<(String, String)> {
        configmap
            .data
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|(name, _)| name.starts_with("named.conf"))
            .collect()
    }

    /// Assert every `named.conf*` file is exactly what hornet's writer emits
    /// for it, and that nothing in it rides a raw carrier (`Unknown`
    /// statements, `extra` clauses), so every value is typed.
    fn assert_written_by_hornet(label: &str, configmap: &ConfigMap) {
        for (name, text) in named_conf_files(configmap) {
            let conf = hornet_bind9::parse_named_conf(&text)
                .unwrap_or_else(|e| panic!("{label}: {name} does not parse: {e}"));
            let rewritten = hornet_bind9::write_named_conf(
                &conf,
                &hornet_bind9::writer::WriteOptions::default(),
            );
            assert_eq!(
                text, rewritten,
                "{label}: {name} is not hornet's canonical output"
            );
            assert_no_raw_carriers(&format!("{label}: {name}"), &conf);
        }
    }

    fn assert_no_raw_carriers(label: &str, conf: &NamedConf) {
        for statement in &conf.statements {
            match statement {
                Statement::Unknown { keyword, .. } => {
                    panic!("{label}: `{keyword}` is an unmodelled raw block")
                }
                Statement::Options(options) => {
                    assert!(
                        options.extra.is_empty(),
                        "{label}: raw options {:?}",
                        options.extra
                    );
                }
                Statement::DnssecPolicy(policy) => {
                    assert!(
                        policy.extra.is_empty(),
                        "{label}: raw policy {:?}",
                        policy.extra
                    );
                }
                _ => {}
            }
        }
    }

    /// Parse every `named.conf*` file in a ConfigMap and fail on a parse error
    /// or an Error-severity validation finding.
    fn assert_config_parses(label: &str, configmap: &ConfigMap) {
        let data = configmap.data.clone().unwrap_or_default();
        let files: Vec<_> = data
            .iter()
            .filter(|(name, _)| name.starts_with("named.conf"))
            .collect();
        assert!(
            !files.is_empty(),
            "{label}: ConfigMap has no named.conf files"
        );
        for (name, text) in files {
            let conf = hornet_bind9::parse_named_conf(text)
                .unwrap_or_else(|e| panic!("{label}: {name} does not parse: {e}\n{text}"));
            let errors: Vec<String> = hornet_bind9::validate_named_conf(&conf)
                .into_iter()
                .filter(|finding| finding.severity == hornet_bind9::Severity::Error)
                .map(|finding| finding.message)
                .collect();
            assert!(
                errors.is_empty(),
                "{label}: {name} is invalid: {errors:?}\n{text}"
            );
        }
    }

    fn instance(config: Value) -> Bind9Instance {
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Instance",
            "metadata": {"name": "dns", "namespace": "dns-system"},
            "spec": {"clusterRef": "dns", "role": "primary", "config": config}
        }))
        .expect("valid Bind9Instance")
    }

    fn cluster(global: Value, acls: Value) -> Bind9Cluster {
        serde_json::from_value(json!({
            "apiVersion": "bindy.firestoned.io/v1beta1",
            "kind": "Bind9Cluster",
            "metadata": {"name": "dns", "namespace": "dns-system"},
            "spec": {"global": global, "acls": acls}
        }))
        .expect("valid Bind9Cluster")
    }

    /// One `Bind9Config` per option the builders render, plus combinations.
    fn config_matrix() -> Vec<(&'static str, Value)> {
        vec![
            ("empty", json!({})),
            ("recursion on", json!({"recursion": true})),
            ("recursion off", json!({"recursion": false})),
            ("allow-query any", json!({"allowQuery": ["any"]})),
            (
                "allow-query cidrs",
                json!({"allowQuery": ["10.0.0.0/8", "192.168.1.0/24", "localhost"]}),
            ),
            ("allow-transfer empty", json!({"allowTransfer": []})),
            (
                "allow-transfer list",
                json!({"allowTransfer": ["10.0.0.5", "10.0.0.6"]}),
            ),
            ("forwarders", json!({"forwarders": ["8.8.8.8", "1.1.1.1"]})),
            (
                "listen-on",
                json!({"listenOn": ["10.0.0.1"], "listenOnV6": ["::1"]}),
            ),
            (
                "rate limit off",
                json!({"rateLimit": {"responsesPerSecond": 0}}),
            ),
            (
                "rate limit on",
                json!({"rateLimit": {"responsesPerSecond": 20}}),
            ),
            (
                "dnssec validation off",
                json!({"dnssec": {"validation": false}}),
            ),
            (
                "dnssec validation on",
                json!({"dnssec": {"validation": true}}),
            ),
            (
                "dnssec signing default",
                json!({"dnssec": {"signing": {"enabled": true}}}),
            ),
            (
                "dnssec signing custom",
                json!({"dnssec": {"validation": true, "signing": {
                    "enabled": true, "policy": "custom-policy", "algorithm": "ECDSAP384SHA384",
                    "kskLifetime": "730d", "zskLifetime": "60d"
                }}}),
            ),
            (
                "everything",
                json!({
                    "recursion": true,
                    "allowQuery": ["10.0.0.0/8"],
                    "allowTransfer": ["10.0.0.5"],
                    "forwarders": ["8.8.8.8"],
                    "listenOn": ["any"],
                    "listenOnV6": ["any"],
                    "rateLimit": {"responsesPerSecond": 15},
                    "dnssec": {"validation": true, "signing": {"enabled": true}}
                }),
            ),
        ]
    }

    #[test]
    fn every_instance_option_renders_a_config_that_parses() {
        for (label, config) in config_matrix() {
            let configmap = build_configmap("dns", "dns-system", &instance(config), None, None)
                .unwrap_or_else(|e| panic!("instance {label}: render failed: {e}"));
            assert_config_parses(&format!("instance {label}"), &configmap);
            assert_written_by_hornet(&format!("instance {label}"), &configmap);
        }
    }

    #[test]
    fn every_cluster_option_renders_a_config_that_parses() {
        let acls = json!({"internal": ["10.0.0.0/8", "192.168.0.0/16"], "trusted": ["127.0.0.1"]});
        for (label, global) in config_matrix() {
            let configmap =
                build_cluster_configmap("dns", "dns-system", &cluster(global, acls.clone()))
                    .unwrap_or_else(|e| panic!("cluster {label}: render failed: {e}"));
            assert_config_parses(&format!("cluster {label}"), &configmap);
            assert_written_by_hornet(&format!("cluster {label}"), &configmap);
        }
    }

    #[test]
    fn every_example_deserializes_and_renders_a_config_that_parses() {
        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut rendered = 0;
        for entry in std::fs::read_dir(&examples).expect("examples/ exists") {
            let path = entry.expect("readable entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable example");
            for document in serde_yaml::Deserializer::from_str(&text) {
                let Ok(value) = <serde_yaml::Value as serde::Deserialize>::deserialize(document)
                else {
                    break;
                };
                let kind = value
                    .get("kind")
                    .and_then(serde_yaml::Value::as_str)
                    .unwrap_or("");
                let name = value
                    .get("metadata")
                    .and_then(|m| m.get("name"))
                    .and_then(serde_yaml::Value::as_str)
                    .unwrap_or("?");
                let label = format!(
                    "{}:{kind}/{name}",
                    path.file_name().unwrap().to_string_lossy()
                );
                let configmap = match kind {
                    "Bind9Instance" => {
                        let instance: Bind9Instance = serde_yaml::from_value(value)
                            .unwrap_or_else(|e| panic!("{label} does not deserialize: {e}"));
                        build_configmap("x", "ns", &instance, None, None)
                    }
                    "Bind9Cluster" => {
                        let cluster: Bind9Cluster = serde_yaml::from_value(value)
                            .unwrap_or_else(|e| panic!("{label} does not deserialize: {e}"));
                        build_cluster_configmap("x", "ns", &cluster)
                    }
                    _ => continue,
                }
                .unwrap_or_else(|e| panic!("{label}: render failed: {e}"));
                assert_config_parses(&label, &configmap);
                assert_written_by_hornet(&label, &configmap);
                rendered += 1;
            }
        }
        assert!(
            rendered > 0,
            "no Bind9Instance or Bind9Cluster examples found"
        );
    }

    fn signing(ksk_lifetime: &str) -> Value {
        json!({"dnssec": {"signing": {"enabled": true, "kskLifetime": ksk_lifetime}}})
    }

    #[test]
    fn a_lifetime_bind_rejects_is_refused_before_publishing() {
        // BIND has no `y` TTL unit; one year is `P1Y` or `365d`.
        let error = build_configmap("dns", "dns-system", &instance(signing("1y")), None, None)
            .expect_err("1y must be refused");
        assert!(error.to_string().contains("1y"), "{error}");
    }

    #[test]
    fn lifetimes_bind_accepts_render() {
        for lifetime in ["P1Y", "365d", "8760h", "unlimited"] {
            let configmap = build_configmap(
                "dns",
                "dns-system",
                &instance(signing(lifetime)),
                None,
                None,
            )
            .unwrap_or_else(|e| panic!("{lifetime}: render failed: {e}"));
            let options = &configmap.data.as_ref().expect("data")["named.conf.options"];
            assert!(
                options.contains(&format!("ksk lifetime {lifetime} ")),
                "{lifetime}: {options}"
            );
        }
    }
}
