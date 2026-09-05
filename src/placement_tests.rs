// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for `placement`.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use kube::api::ObjectMeta;

    use crate::crd::{
        Bind9Cluster, Bind9ClusterCommonSpec, Bind9ClusterSpec, Bind9Instance, Bind9InstanceSpec,
        ClusterBind9Provider, ClusterBind9ProviderSpec, NodeInclusionPolicy, PlacementConfig,
        PrimaryConfig, SecondaryConfig, ServerRole, SpreadRule, SpreadScope, WhenUnsatisfiable,
    };
    use crate::placement::{
        build_pod_placement, resolve_placement, validate_placement, PlacementContext,
        PlacementRejection,
    };

    // ------------------------------------------------------------------
    // Fixtures
    // ------------------------------------------------------------------

    fn instance(name: &str, role: ServerRole, cluster_ref: &str, replicas: i32) -> Bind9Instance {
        #[allow(deprecated)]
        Bind9Instance {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some("dns".into()),
                ..Default::default()
            },
            spec: Bind9InstanceSpec {
                cluster_ref: cluster_ref.to_string(),
                role,
                replicas: Some(replicas),
                version: None,
                image: None,
                config_map_refs: None,
                config: None,
                primary_servers: None,
                volumes: None,
                volume_mounts: None,
                rndc_secret_ref: None,
                rndc_key: None,
                storage: None,
                placement: None,
                bindcar_config: None,
            },
            status: None,
        }
    }

    fn common_spec() -> Bind9ClusterCommonSpec {
        Bind9ClusterCommonSpec {
            version: Some("9.18".into()),
            primary: None,
            secondary: None,
            image: None,
            config_map_refs: None,
            global: None,
            rndc_secret_refs: None,
            acls: None,
            volumes: None,
            volume_mounts: None,
        }
    }

    fn cluster(common: Bind9ClusterCommonSpec) -> Bind9Cluster {
        Bind9Cluster::new("my-dns", Bind9ClusterSpec { common })
    }

    fn provider(common: Bind9ClusterCommonSpec) -> ClusterBind9Provider {
        ClusterBind9Provider::new(
            "shared-dns",
            ClusterBind9ProviderSpec {
                namespace: Some("bindy-system".into()),
                common,
            },
        )
    }

    fn selector_labels() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("app".into(), "bind9".into());
        m.insert(
            "app.kubernetes.io/instance".into(),
            "my-dns-primary-0".into(),
        );
        m
    }

    /// A cluster-managed primary: one Pod, three sibling instances.
    fn managed_ctx<'a>(labels: &'a BTreeMap<String, String>) -> PlacementContext<'a> {
        PlacementContext {
            instance_name: "my-dns-primary-0",
            cluster_name: Some("my-dns"),
            role: ServerRole::Primary,
            instance_replicas: 1,
            role_instance_count: 3,
            cluster_instance_count: 5,
            instance_selector_labels: labels,
        }
    }

    /// A standalone instance with its own replicas and no owning cluster.
    fn standalone_ctx<'a>(
        labels: &'a BTreeMap<String, String>,
        replicas: i32,
    ) -> PlacementContext<'a> {
        PlacementContext {
            instance_name: "solo",
            cluster_name: None,
            role: ServerRole::Primary,
            instance_replicas: replicas,
            role_instance_count: 1,
            cluster_instance_count: 1,
            instance_selector_labels: labels,
        }
    }

    fn rule(topology_key: &str) -> SpreadRule {
        SpreadRule {
            topology_key: topology_key.into(),
            max_skew: None,
            when_unsatisfiable: None,
            scope: None,
            min_domains: None,
            node_affinity_policy: None,
            node_taints_policy: None,
        }
    }

    // ------------------------------------------------------------------
    // Resolution precedence
    // ------------------------------------------------------------------

    #[test]
    fn resolve_returns_none_when_nothing_is_configured() {
        let inst = instance("my-dns-primary-0", ServerRole::Primary, "my-dns", 1);
        let c = cluster(common_spec());
        assert!(resolve_placement(&inst, Some(&c), None).is_none());
    }

    #[test]
    fn resolve_prefers_instance_over_role() {
        let mut inst = instance("my-dns-primary-0", ServerRole::Primary, "my-dns", 1);
        inst.spec.placement = Some(PlacementConfig {
            spread: Some(vec![rule("instance-level")]),
        });

        let mut common = common_spec();
        common.primary = Some(PrimaryConfig {
            placement: Some(PlacementConfig {
                spread: Some(vec![rule("role-level")]),
            }),
            ..Default::default()
        });
        let c = cluster(common);

        let resolved = resolve_placement(&inst, Some(&c), None).expect("placement resolves");
        assert_eq!(
            resolved.spread.as_ref().unwrap()[0].topology_key,
            "instance-level"
        );
    }

    #[test]
    fn resolve_returns_none_when_the_role_block_sets_no_placement() {
        // A primary block that exists but does not set `placement` must not be
        // mistaken for a configured empty block — the caller needs `None` here
        // so the operator default can apply.
        let inst = instance("my-dns-primary-0", ServerRole::Primary, "my-dns", 1);
        let mut common = common_spec();
        common.primary = Some(PrimaryConfig {
            replicas: Some(3),
            ..Default::default()
        });
        let c = cluster(common);

        assert!(resolve_placement(&inst, Some(&c), None).is_none());
    }

    #[test]
    fn resolve_uses_the_role_block_matching_the_instance_role() {
        let secondary = instance("my-dns-secondary-0", ServerRole::Secondary, "my-dns", 1);

        let mut common = common_spec();
        common.primary = Some(PrimaryConfig {
            placement: Some(PlacementConfig {
                spread: Some(vec![rule("primary-only")]),
            }),
            ..Default::default()
        });
        common.secondary = Some(SecondaryConfig {
            placement: Some(PlacementConfig {
                spread: Some(vec![rule("secondary-only")]),
            }),
            ..Default::default()
        });
        let c = cluster(common);

        let resolved = resolve_placement(&secondary, Some(&c), None).unwrap();
        assert_eq!(
            resolved.spread.as_ref().unwrap()[0].topology_key,
            "secondary-only"
        );
    }

    #[test]
    fn resolve_reads_from_a_cluster_scoped_provider() {
        let inst = instance("shared-dns-primary-0", ServerRole::Primary, "shared-dns", 1);
        let mut common = common_spec();
        common.primary = Some(PrimaryConfig {
            placement: Some(PlacementConfig {
                spread: Some(vec![rule("provider-role-level")]),
            }),
            ..Default::default()
        });
        let p = provider(common);

        let resolved = resolve_placement(&inst, None, Some(&p)).unwrap();
        assert_eq!(
            resolved.spread.as_ref().unwrap()[0].topology_key,
            "provider-role-level"
        );
    }

    // ------------------------------------------------------------------
    // Defaults
    // ------------------------------------------------------------------

    #[test]
    fn default_spreads_cluster_managed_primaries_across_zones() {
        // The regression this whole feature exists to prevent: three primaries,
        // one Pod each, all free to land in the same zone.
        let labels = selector_labels();
        let resolved = build_pod_placement(None, &managed_ctx(&labels));

        let constraints = resolved
            .topology_spread_constraints
            .expect("default spread applies to a 3-primary cluster");
        assert_eq!(constraints.len(), 1);

        let c = &constraints[0];
        assert_eq!(c.topology_key, "topology.kubernetes.io/zone");
        assert_eq!(c.max_skew, 1);
        assert_eq!(c.when_unsatisfiable, "ScheduleAnyway");

        // The selector must match SIBLING Deployments, not just this one.
        let match_labels = c
            .label_selector
            .as_ref()
            .unwrap()
            .match_labels
            .clone()
            .unwrap();
        assert_eq!(
            match_labels
                .get("bindy.firestoned.io/cluster")
                .map(String::as_str),
            Some("my-dns")
        );
        assert_eq!(
            match_labels
                .get("bindy.firestoned.io/role")
                .map(String::as_str),
            Some("primary")
        );
        assert!(
            !match_labels.contains_key("app.kubernetes.io/instance"),
            "a per-instance selector would balance a set of one and spread nothing"
        );
    }

    #[test]
    fn default_is_soft_so_a_single_zone_cluster_still_schedules() {
        let labels = selector_labels();
        let resolved = build_pod_placement(None, &managed_ctx(&labels));
        assert_eq!(
            resolved.topology_spread_constraints.unwrap()[0].when_unsatisfiable,
            "ScheduleAnyway"
        );
    }

    #[test]
    fn default_skipped_for_a_single_primary() {
        let labels = selector_labels();
        let mut ctx = managed_ctx(&labels);
        ctx.role_instance_count = 1;
        assert!(build_pod_placement(None, &ctx)
            .topology_spread_constraints
            .is_none());
    }

    #[test]
    fn default_uses_instance_scope_for_a_standalone_multi_replica_instance() {
        let labels = selector_labels();
        let resolved = build_pod_placement(None, &standalone_ctx(&labels, 3));

        let constraints = resolved.topology_spread_constraints.unwrap();
        assert_eq!(constraints.len(), 1);
        assert_eq!(
            constraints[0].label_selector.as_ref().unwrap().match_labels,
            Some(labels),
            "a standalone Deployment really does own every Pod in the set"
        );
    }

    #[test]
    fn default_skipped_for_a_standalone_single_replica_instance() {
        let labels = selector_labels();
        let resolved = build_pod_placement(None, &standalone_ctx(&labels, 1));
        assert!(resolved.topology_spread_constraints.is_none());
    }

    #[test]
    fn default_does_not_apply_to_secondaries() {
        // Issue #467 asked for primaries, and applying a default to secondaries
        // would silently change where already-running secondary Pods schedule
        // the moment the operator is upgraded.
        let labels = selector_labels();
        let mut ctx = managed_ctx(&labels);
        ctx.role = ServerRole::Secondary;
        ctx.instance_name = "my-dns-secondary-0";

        assert!(
            build_pod_placement(None, &ctx)
                .topology_spread_constraints
                .is_none(),
            "secondaries must opt in via secondary.placement.spread"
        );
    }

    #[test]
    fn secondaries_can_opt_in_explicitly() {
        let labels = selector_labels();
        let mut ctx = managed_ctx(&labels);
        ctx.role = ServerRole::Secondary;

        let config = PlacementConfig {
            spread: Some(vec![rule("topology.kubernetes.io/zone")]),
        };
        let constraints = build_pod_placement(Some(&config), &ctx)
            .topology_spread_constraints
            .expect("an explicit rule is honoured for secondaries");
        assert_eq!(constraints.len(), 1);

        // ...and the generated selector targets sibling SECONDARIES, not primaries.
        let match_labels = constraints[0]
            .label_selector
            .as_ref()
            .unwrap()
            .match_labels
            .clone()
            .unwrap();
        assert_eq!(
            match_labels
                .get("bindy.firestoned.io/role")
                .map(String::as_str),
            Some("secondary")
        );
    }

    #[test]
    fn default_does_not_apply_to_a_standalone_secondary() {
        let labels = selector_labels();
        let mut ctx = standalone_ctx(&labels, 3);
        ctx.role = ServerRole::Secondary;
        assert!(build_pod_placement(None, &ctx)
            .topology_spread_constraints
            .is_none());
    }

    #[test]
    fn empty_spread_list_opts_out_of_the_default() {
        let labels = selector_labels();
        let config = PlacementConfig {
            spread: Some(vec![]),
        };
        let resolved = build_pod_placement(Some(&config), &managed_ctx(&labels));
        assert!(
            resolved.topology_spread_constraints.is_none(),
            "`spread: []` is the documented opt-out and must beat the default"
        );
    }

    // ------------------------------------------------------------------
    // Explicit rules
    // ------------------------------------------------------------------

    #[test]
    fn explicit_rules_replace_the_default_entirely() {
        let labels = selector_labels();
        let config = PlacementConfig {
            spread: Some(vec![
                SpreadRule {
                    topology_key: "failure-domain.acme.io/rack".into(),
                    max_skew: Some(2),
                    when_unsatisfiable: Some(WhenUnsatisfiable::DoNotSchedule),
                    scope: Some(SpreadScope::Role),
                    min_domains: Some(3),
                    node_affinity_policy: Some(NodeInclusionPolicy::Honor),
                    node_taints_policy: Some(NodeInclusionPolicy::Ignore),
                },
                rule("kubernetes.io/hostname"),
            ]),
        };

        let constraints = build_pod_placement(Some(&config), &managed_ctx(&labels))
            .topology_spread_constraints
            .unwrap();
        assert_eq!(constraints.len(), 2);

        let rack = &constraints[0];
        assert_eq!(rack.topology_key, "failure-domain.acme.io/rack");
        assert_eq!(rack.max_skew, 2);
        assert_eq!(rack.when_unsatisfiable, "DoNotSchedule");
        assert_eq!(rack.min_domains, Some(3));
        assert_eq!(rack.node_affinity_policy.as_deref(), Some("Honor"));
        assert_eq!(rack.node_taints_policy.as_deref(), Some("Ignore"));

        // No zone rule was requested, so none is emitted.
        assert!(constraints
            .iter()
            .all(|c| c.topology_key != "topology.kubernetes.io/zone"));
    }

    #[test]
    fn min_domains_dropped_for_a_soft_constraint() {
        // Kubernetes rejects minDomains alongside ScheduleAnyway. Dropping it
        // is friendlier than emitting a Pod spec the API server will refuse.
        let labels = selector_labels();
        let config = PlacementConfig {
            spread: Some(vec![SpreadRule {
                min_domains: Some(3),
                when_unsatisfiable: Some(WhenUnsatisfiable::ScheduleAnyway),
                ..rule("topology.kubernetes.io/zone")
            }]),
        };
        let constraints = build_pod_placement(Some(&config), &managed_ctx(&labels))
            .topology_spread_constraints
            .unwrap();
        assert_eq!(constraints[0].min_domains, None);
    }

    #[test]
    fn cluster_scope_selects_on_the_cluster_label_alone() {
        let labels = selector_labels();
        let config = PlacementConfig {
            spread: Some(vec![SpreadRule {
                scope: Some(SpreadScope::Cluster),
                ..rule("topology.kubernetes.io/zone")
            }]),
        };
        let constraints = build_pod_placement(Some(&config), &managed_ctx(&labels))
            .topology_spread_constraints
            .unwrap();
        let match_labels = constraints[0]
            .label_selector
            .as_ref()
            .unwrap()
            .match_labels
            .clone()
            .unwrap();
        assert_eq!(match_labels.len(), 1);
        assert_eq!(
            match_labels
                .get("bindy.firestoned.io/cluster")
                .map(String::as_str),
            Some("my-dns")
        );
    }

    #[test]
    fn role_scope_falls_back_to_instance_scope_without_a_cluster() {
        // A standalone instance's Pods carry no cluster label, so a Role-scoped
        // selector would match nothing and silently spread nothing.
        let labels = selector_labels();
        let config = PlacementConfig {
            spread: Some(vec![SpreadRule {
                scope: Some(SpreadScope::Role),
                ..rule("topology.kubernetes.io/zone")
            }]),
        };
        let constraints = build_pod_placement(Some(&config), &standalone_ctx(&labels, 2))
            .topology_spread_constraints
            .unwrap();
        assert_eq!(
            constraints[0].label_selector.as_ref().unwrap().match_labels,
            Some(labels)
        );
    }

    // ------------------------------------------------------------------
    // Validation — correctness
    // ------------------------------------------------------------------

    #[test]
    fn valid_config_passes() {
        let config = PlacementConfig {
            spread: Some(vec![SpreadRule {
                max_skew: Some(1),
                when_unsatisfiable: Some(WhenUnsatisfiable::DoNotSchedule),
                min_domains: Some(3),
                ..rule("topology.kubernetes.io/zone")
            }]),
        };
        assert!(validate_placement(&config).is_ok());
    }

    #[test]
    fn rejects_too_many_spread_rules() {
        let config = PlacementConfig {
            spread: Some((0..9).map(|i| rule(&format!("key{i}"))).collect()),
        };
        assert!(matches!(
            validate_placement(&config),
            Err(PlacementRejection::TooManySpreadRules { count: 9 })
        ));
    }

    #[test]
    fn rejects_invalid_topology_keys() {
        for bad in ["", "bad key", "/name", "prefix/", &"x".repeat(64)] {
            let config = PlacementConfig {
                spread: Some(vec![rule(bad)]),
            };
            assert!(
                matches!(
                    validate_placement(&config),
                    Err(PlacementRejection::InvalidTopologyKey { .. })
                ),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn accepts_well_formed_topology_keys() {
        for good in [
            "topology.kubernetes.io/zone",
            "kubernetes.io/hostname",
            "failure-domain.acme.io/rack",
            "karpenter.sh/capacity-type",
            "zone",
        ] {
            let config = PlacementConfig {
                spread: Some(vec![rule(good)]),
            };
            assert!(
                validate_placement(&config).is_ok(),
                "expected {good:?} to be accepted"
            );
        }
    }

    #[test]
    fn topology_key_length_boundaries_match_kubernetes() {
        // 253 (prefix) + '/' + 63 (name) = 317 is the largest key the API
        // server accepts. An earlier revision capped at 316 and rejected it.
        let prefix_253 = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(61)
        );
        assert_eq!(prefix_253.len(), 253);
        let name_63 = "b".repeat(63);
        let max_key = format!("{prefix_253}/{name_63}");
        assert_eq!(max_key.len(), 317);

        let config = PlacementConfig {
            spread: Some(vec![rule(&max_key)]),
        };
        assert!(
            validate_placement(&config).is_ok(),
            "a maximally-sized but valid qualified name must be accepted"
        );

        // 254-character prefix is over the DNS subdomain cap.
        let prefix_254 = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(62)
        );
        assert_eq!(prefix_254.len(), 254);
        let config = PlacementConfig {
            spread: Some(vec![rule(&format!("{prefix_254}/zone"))]),
        };
        assert!(matches!(
            validate_placement(&config),
            Err(PlacementRejection::InvalidTopologyKey { .. })
        ));

        // 64-character name segment is over the qualified-name cap.
        let config = PlacementConfig {
            spread: Some(vec![rule(&format!("example.com/{}", "b".repeat(64)))]),
        };
        assert!(matches!(
            validate_placement(&config),
            Err(PlacementRejection::InvalidTopologyKey { .. })
        ));
    }

    #[test]
    fn accepts_a_dns_label_longer_than_63_chars() {
        // Kubernetes' IsDNS1123Subdomain bounds only the 253-character TOTAL,
        // not individual labels, so `<64 a's>/zone` is accepted by the API
        // server both as a node label and as a topologyKey. Rejecting it here
        // would refuse input the platform accepts.
        let config = PlacementConfig {
            spread: Some(vec![rule(&format!("{}/zone", "a".repeat(64)))]),
        };
        assert!(validate_placement(&config).is_ok());
    }

    #[test]
    fn rejects_malformed_dns_prefix_segments() {
        // Each segment must start and end with an alphanumeric, be lowercase,
        // and be non-empty — mirroring what the API server enforces.
        for bad in [
            "abc-/zone", // trailing hyphen
            "-abc/zone", // leading hyphen
            "Abc/zone",  // uppercase
            "a..b/zone", // empty segment
            "abc./zone", // trailing dot leaves an empty segment
            "abc/-zone", // name segment starts with a hyphen
            "abc/zone-", // name segment ends with a hyphen
        ] {
            let config = PlacementConfig {
                spread: Some(vec![rule(bad)]),
            };
            assert!(
                matches!(
                    validate_placement(&config),
                    Err(PlacementRejection::InvalidTopologyKey { .. })
                ),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_duplicate_topology_key_and_when_unsatisfiable_pairs() {
        // Kubernetes rejects the Pod outright; catching it here surfaces a
        // condition on the CR the user actually edited.
        let config = PlacementConfig {
            spread: Some(vec![
                rule("topology.kubernetes.io/zone"),
                rule("topology.kubernetes.io/zone"),
            ]),
        };
        assert!(matches!(
            validate_placement(&config),
            Err(PlacementRejection::DuplicateSpreadRule { .. })
        ));
    }

    #[test]
    fn allows_same_topology_key_with_different_when_unsatisfiable() {
        let config = PlacementConfig {
            spread: Some(vec![
                SpreadRule {
                    when_unsatisfiable: Some(WhenUnsatisfiable::DoNotSchedule),
                    ..rule("topology.kubernetes.io/zone")
                },
                SpreadRule {
                    when_unsatisfiable: Some(WhenUnsatisfiable::ScheduleAnyway),
                    ..rule("topology.kubernetes.io/zone")
                },
            ]),
        };
        assert!(validate_placement(&config).is_ok());
    }

    #[test]
    fn rejects_non_positive_max_skew() {
        let config = PlacementConfig {
            spread: Some(vec![SpreadRule {
                max_skew: Some(0),
                ..rule("topology.kubernetes.io/zone")
            }]),
        };
        assert!(matches!(
            validate_placement(&config),
            Err(PlacementRejection::InvalidMaxSkew { value: 0, .. })
        ));
    }

    #[test]
    fn rejects_min_domains_without_a_hard_constraint() {
        let config = PlacementConfig {
            spread: Some(vec![SpreadRule {
                min_domains: Some(2),
                ..rule("topology.kubernetes.io/zone")
            }]),
        };
        assert!(matches!(
            validate_placement(&config),
            Err(PlacementRejection::MinDomainsRequiresHardConstraint { index: 0 })
        ));
    }

    #[test]
    fn rejects_non_positive_min_domains() {
        let config = PlacementConfig {
            spread: Some(vec![SpreadRule {
                min_domains: Some(0),
                when_unsatisfiable: Some(WhenUnsatisfiable::DoNotSchedule),
                ..rule("topology.kubernetes.io/zone")
            }]),
        };
        assert!(matches!(
            validate_placement(&config),
            Err(PlacementRejection::InvalidMinDomains { value: 0, .. })
        ));
    }

    // ------------------------------------------------------------------
    // Generated CRD schema
    // ------------------------------------------------------------------
    //
    // The limits below are enforced by the API server from the generated
    // schema, which is what makes them hold without an admission policy. The
    // Rust validators above are only a backstop, so a dropped `#[schemars]`
    // attribute would silently move enforcement from admission time to
    // reconcile time with every unit test still green. These assertions run in
    // CI with no cluster and catch exactly that.

    /// Digs out the `placement` schema from a generated CRD.
    fn placement_schema(crd: &serde_json::Value, path: &[&str]) -> serde_json::Value {
        let mut node =
            &crd["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"];
        for segment in path {
            node = &node["properties"][segment];
        }
        node["properties"]["placement"].clone()
    }

    #[test]
    fn generated_schema_enforces_the_spread_rule_cap() {
        use kube::CustomResourceExt;

        let crd = serde_json::to_value(crate::crd::Bind9Cluster::crd()).unwrap();
        let placement = placement_schema(&crd, &["primary"]);

        assert_eq!(
            placement["properties"]["spread"]["maxItems"].as_u64(),
            Some(crate::constants::MAX_SPREAD_RULES as u64),
            "spread must carry maxItems so the API server rejects an over-long list"
        );
    }

    #[test]
    fn generated_schema_constrains_topology_key() {
        use kube::CustomResourceExt;

        let crd = serde_json::to_value(crate::crd::Bind9Instance::crd()).unwrap();
        let placement = placement_schema(&crd, &[]);
        let key = &placement["properties"]["spread"]["items"]["properties"]["topologyKey"];

        // 253 prefix + '/' + 63 name.
        assert_eq!(key["maxLength"].as_u64(), Some(317));
        assert_eq!(key["minLength"].as_u64(), Some(1));
        assert!(
            key["pattern"].as_str().is_some_and(|p| !p.is_empty()),
            "topologyKey must carry a charset pattern"
        );
    }

    #[test]
    fn generated_schema_carries_the_cross_field_cel_rules() {
        use kube::CustomResourceExt;

        let crd = serde_json::to_value(crate::crd::ClusterBind9Provider::crd()).unwrap();
        let placement = placement_schema(&crd, &["secondary"]);
        let rules = placement["properties"]["spread"]["items"]["x-kubernetes-validations"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let joined: String = rules
            .iter()
            .filter_map(|r| r["rule"].as_str())
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            joined.contains("minDomains"),
            "the minDomains/DoNotSchedule pairing must be enforced by CEL: {joined}"
        );
        assert!(
            joined.contains("indexOf('/')"),
            "the 253-character prefix cap must be enforced by CEL (RE2 has no lookahead): {joined}"
        );
    }

    #[test]
    fn generated_schema_bounds_numeric_fields() {
        use kube::CustomResourceExt;

        let crd = serde_json::to_value(crate::crd::Bind9Cluster::crd()).unwrap();
        let item = placement_schema(&crd, &["primary"])["properties"]["spread"]["items"].clone();

        assert_eq!(item["properties"]["maxSkew"]["minimum"].as_f64(), Some(1.0));
        assert_eq!(
            item["properties"]["minDomains"]["minimum"].as_f64(),
            Some(1.0)
        );
        for (field, expected) in [
            ("scope", vec!["Instance", "Role", "Cluster"]),
            ("whenUnsatisfiable", vec!["DoNotSchedule", "ScheduleAnyway"]),
        ] {
            let got: Vec<&str> = item["properties"][field]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect();
            assert_eq!(got, expected, "{field} must be a closed enum");
        }
    }
}
