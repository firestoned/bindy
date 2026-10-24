// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for gateway address resolution from Scout's watch caches (ADR-0028).

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::scout::{Gateway, GatewayServiceTarget, NamespacedName};
    use k8s_openapi::api::core::v1::Service;
    use kube::core::SelectorExt;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    const GW_NS: &str = "gateways";

    fn nn(namespace: &str, name: &str) -> NamespacedName {
        NamespacedName {
            namespace: namespace.to_string(),
            name: name.to_string(),
        }
    }

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// A Service in `namespace` with `labels`, of type `svc_type`, whose load
    /// balancer reports `ips`; `deleting` sets a deletionTimestamp.
    fn svc(
        namespace: &str,
        name: &str,
        lbls: &[(&str, &str)],
        svc_type: &str,
        ips: &[&str],
        deleting: bool,
    ) -> Arc<Service> {
        let ingress: Vec<_> = ips.iter().map(|ip| json!({"ip": ip})).collect();
        let mut meta = json!({"name": name, "namespace": namespace, "labels": labels(lbls)});
        if deleting {
            meta["deletionTimestamp"] = json!("2026-10-24T00:00:00Z");
            meta["finalizers"] = json!(["service.kubernetes.io/load-balancer-cleanup"]);
        }
        Arc::new(
            serde_json::from_value(json!({
                "apiVersion": "v1", "kind": "Service", "metadata": meta,
                "spec": {"type": svc_type},
                "status": {"loadBalancer": {"ingress": ingress}},
            }))
            .expect("valid Service"),
        )
    }

    fn lb(name: &str, lbls: &[(&str, &str)], ips: &[&str]) -> Arc<Service> {
        svc(GW_NS, name, lbls, "LoadBalancer", ips, false)
    }

    fn gateway(namespace: &str, name: &str, class: &str) -> Arc<Gateway> {
        Arc::new(
            serde_json::from_value(json!({
                "apiVersion": "gateway.networking.k8s.io/v1", "kind": "Gateway",
                "metadata": {"name": name, "namespace": namespace},
                "spec": {"gatewayClassName": class},
            }))
            .expect("valid Gateway"),
        )
    }

    /// The addresses the Services `gateway_lb_services` picks serve, sorted,
    /// each once.
    fn gateway_service_ips(
        target: &GatewayServiceTarget,
        gateway: &NamespacedName,
        services: &[Arc<Service>],
    ) -> Vec<String> {
        let mut ips: Vec<String> = gateway_lb_services(target, gateway, services)
            .into_iter()
            .flat_map(|svc| crate::scout::resolve_ips_from_service_lb_status(svc))
            .collect();
        ips.sort();
        ips.dedup();
        ips
    }

    fn labeled(selector: &str) -> GatewayServiceTarget {
        GatewayServiceTarget::Labeled {
            namespace: GW_NS.to_string(),
            selector: selector.to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Label selectors
    // -----------------------------------------------------------------------

    #[test]
    fn equality_based_selectors_parse_and_match() {
        let on = labels(&[("app", "traefik"), ("tier", "edge")]);
        for (selector, want) in [
            ("app=traefik", true),
            ("app==traefik", true),
            ("app!=traefik", false),
            ("app", true),
            ("!app", false),
            ("!missing", true),
            ("app=traefik, tier=edge", true),
            ("app=traefik,tier=core", false),
        ] {
            let parsed = parse_label_selector(selector).expect(selector);
            assert_eq!(parsed.matches(&on), want, "{selector}");
        }
    }

    #[test]
    fn unsupported_or_malformed_selectors_are_errors() {
        for selector in ["", "  ", "=traefik", "app in (a,b)", "app=a=b", "app=,"] {
            assert!(
                parse_label_selector(selector).is_err(),
                "{selector:?} must be refused at startup"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Target matching
    // -----------------------------------------------------------------------

    #[test]
    fn a_name_target_matches_only_that_service() {
        let target = GatewayServiceTarget::Name(nn(GW_NS, "traefik"));
        assert!(target_matches(&target, &lb("traefik", &[], &[])));
        assert!(!target_matches(&target, &lb("other", &[], &[])));
        assert!(!target_matches(
            &target,
            &svc("elsewhere", "traefik", &[], "LoadBalancer", &[], false)
        ));
    }

    #[test]
    fn a_labeled_target_matches_its_namespace_and_selector() {
        let target = labeled("app=traefik");
        assert!(target_matches(
            &target,
            &lb("vip-a", &[("app", "traefik")], &[])
        ));
        assert!(!target_matches(
            &target,
            &lb("vip-x", &[("app", "other")], &[])
        ));
        assert!(!target_matches(
            &target,
            &svc(
                "elsewhere",
                "vip-a",
                &[("app", "traefik")],
                "LoadBalancer",
                &[],
                false
            )
        ));
    }

    // -----------------------------------------------------------------------
    // Addresses (Decisions 1 and 2)
    // -----------------------------------------------------------------------

    #[test]
    fn a_labeled_target_publishes_every_matching_load_balancer() {
        let services = [
            lb("vip-b", &[("app", "traefik")], &["10.0.0.241"]),
            lb("vip-a", &[("app", "traefik")], &["10.0.0.246"]),
            lb(
                "vip-c",
                &[("app", "traefik")],
                &["10.0.0.240", "10.0.0.246"],
            ),
            lb("other", &[("app", "nginx")], &["10.0.9.9"]),
        ];
        let mut ips = gateway_service_ips(&labeled("app=traefik"), &nn(GW_NS, "gw"), &services);
        ips.sort();
        assert_eq!(ips, vec!["10.0.0.240", "10.0.0.241", "10.0.0.246"]);
    }

    #[test]
    fn pending_non_load_balancer_and_terminating_services_contribute_nothing() {
        let services = [
            lb("vip-a", &[("app", "traefik")], &["10.0.0.1"]),
            lb("vip-pending", &[("app", "traefik")], &[]),
            svc(
                GW_NS,
                "cluster-ip",
                &[("app", "traefik")],
                "ClusterIP",
                &["10.96.0.10"],
                false,
            ),
            svc(
                GW_NS,
                "vip-going",
                &[("app", "traefik")],
                "LoadBalancer",
                &["10.0.0.2"],
                true,
            ),
        ];
        assert_eq!(
            gateway_service_ips(&labeled("app=traefik"), &nn(GW_NS, "gw"), &services),
            vec!["10.0.0.1"]
        );
    }

    #[test]
    fn services_labelled_for_a_gateway_serve_only_that_gateway() {
        // GEP-1762: implementations label each Service they generate for a
        // Gateway with gateway.networking.k8s.io/gateway-name.
        let services = [
            lb(
                "gw-a-svc",
                &[("app", "envoy"), (GATEWAY_NAME_LABEL, "gw-a")],
                &["10.1.0.1"],
            ),
            lb(
                "gw-b-svc",
                &[("app", "envoy"), (GATEWAY_NAME_LABEL, "gw-b")],
                &["10.1.0.2"],
            ),
        ];
        let target = labeled("app=envoy");
        assert_eq!(
            gateway_service_ips(&target, &nn(GW_NS, "gw-a"), &services),
            vec!["10.1.0.1"]
        );
        assert_eq!(
            gateway_service_ips(&target, &nn(GW_NS, "gw-b"), &services),
            vec!["10.1.0.2"]
        );
        assert!(gateway_service_ips(&target, &nn(GW_NS, "gw-c"), &services).is_empty());
    }

    #[test]
    fn unlabelled_shared_services_serve_every_gateway_of_the_class() {
        let services = [
            lb("vip-a", &[("app", "traefik")], &["10.0.0.1"]),
            lb("vip-b", &[("app", "traefik")], &["10.0.0.2"]),
        ];
        let mut ips = gateway_service_ips(&labeled("app=traefik"), &nn(GW_NS, "any-gw"), &services);
        ips.sort();
        assert_eq!(ips, vec!["10.0.0.1", "10.0.0.2"]);
    }

    #[test]
    fn a_name_target_publishes_its_service_unless_it_is_going_away() {
        let target = GatewayServiceTarget::Name(nn(GW_NS, "traefik"));
        assert_eq!(
            gateway_service_ips(
                &target,
                &nn(GW_NS, "gw"),
                &[lb("traefik", &[], &["10.0.0.5"])]
            ),
            vec!["10.0.0.5"]
        );
        let going = [svc(
            GW_NS,
            "traefik",
            &[],
            "LoadBalancer",
            &["10.0.0.5"],
            true,
        )];
        assert!(gateway_service_ips(&target, &nn(GW_NS, "gw"), &going).is_empty());
        let not_lb = [svc(
            GW_NS,
            "traefik",
            &[],
            "ClusterIP",
            &["10.96.0.1"],
            false,
        )];
        assert!(gateway_service_ips(&target, &nn(GW_NS, "gw"), &not_lb).is_empty());
    }

    // -----------------------------------------------------------------------
    // What a change wakes (Decision 4)
    // -----------------------------------------------------------------------

    #[test]
    fn a_service_wakes_the_gateways_of_the_classes_its_targets_serve() {
        let targets: BTreeMap<String, GatewayServiceTarget> = [
            ("traefik".to_string(), labeled("app=traefik")),
            ("envoy".to_string(), labeled("app=envoy")),
        ]
        .into();
        let gateways = [
            gateway(GW_NS, "gw-t1", "traefik"),
            gateway("team-a", "gw-t2", "traefik"),
            gateway(GW_NS, "gw-e", "envoy"),
            gateway(GW_NS, "gw-other", "cilium"),
        ];
        let mut woken =
            gateways_for_service(&lb("vip", &[("app", "traefik")], &[]), &targets, &gateways);
        woken.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
        assert_eq!(woken, vec![nn(GW_NS, "gw-t1"), nn("team-a", "gw-t2")]);
        assert!(
            gateways_for_service(&lb("x", &[("app", "nope")], &[]), &targets, &gateways).is_empty()
        );
    }

    #[test]
    fn a_gateway_wakes_the_routes_that_name_it() {
        let routes = [
            (nn("team-a", "web"), vec![nn(GW_NS, "gw-1")]),
            (
                nn("team-a", "api"),
                vec![nn(GW_NS, "gw-2"), nn(GW_NS, "gw-1")],
            ),
            (nn("team-b", "db"), vec![nn(GW_NS, "gw-3")]),
        ];
        let mut woken = routes_for_gateways(&routes, &[nn(GW_NS, "gw-1")]);
        woken.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(woken, vec![nn("team-a", "api"), nn("team-a", "web")]);
        assert!(routes_for_gateways(&routes, &[]).is_empty());
    }
}
