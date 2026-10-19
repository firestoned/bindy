// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Tests for Scout's zone and address allowlists (ADR-0025, decisions 1 and 2).

#[cfg(test)]
mod tests {
    use super::super::*;

    fn zones(raw: &str) -> Vec<String> {
        parse_allowed_zones(raw).expect("valid zone list")
    }

    fn ranges(raw: &str) -> Vec<Ipv4Range> {
        parse_allowed_ip_ranges(raw).expect("valid range list")
    }

    fn ips(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    // ---------------------------------------------------------------------
    // parse_allowed_zones
    // ---------------------------------------------------------------------

    #[test]
    fn zones_are_trimmed_lowercased_and_lose_a_trailing_dot() {
        assert_eq!(
            zones(" Foo.Example.com. , bar.example.com"),
            vec!["foo.example.com", "bar.example.com"]
        );
    }

    #[test]
    fn empty_zone_entries_are_ignored() {
        assert_eq!(zones("foo.example.com,,"), vec!["foo.example.com"]);
    }

    #[test]
    fn a_zone_list_with_no_entries_is_rejected() {
        assert!(parse_allowed_zones(" , ").is_err());
    }

    #[test]
    fn a_wildcard_zone_is_rejected() {
        let err = parse_allowed_zones("foo.example.com,*").expect_err("wildcard refused");
        assert!(err.to_string().contains('*'), "{err}");
    }

    #[test]
    fn zone_entries_that_are_not_dns_names_are_rejected() {
        for bad in [
            "foo..example.com",
            "-foo.example.com",
            "foo-.example.com",
            "foo example.com",
            "foo.example.com;",
            ".",
        ] {
            assert!(parse_allowed_zones(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn an_over_long_zone_label_is_rejected() {
        let label = "a".repeat(64);
        assert!(parse_allowed_zones(&format!("{label}.example.com")).is_err());
    }

    // ---------------------------------------------------------------------
    // zone_is_allowed
    // ---------------------------------------------------------------------

    #[test]
    fn a_listed_zone_is_allowed() {
        assert!(zone_is_allowed(
            &zones("foo.example.com"),
            "foo.example.com"
        ));
    }

    #[test]
    fn a_subzone_of_a_listed_zone_is_allowed() {
        assert!(zone_is_allowed(
            &zones("foo.example.com"),
            "api.foo.example.com"
        ));
    }

    #[test]
    fn zone_matching_ignores_case_and_a_trailing_dot() {
        assert!(zone_is_allowed(
            &zones("foo.example.com"),
            "FOO.example.com."
        ));
    }

    #[test]
    fn a_sibling_zone_is_not_allowed() {
        assert!(!zone_is_allowed(
            &zones("foo.example.com"),
            "bar.example.com"
        ));
    }

    #[test]
    fn a_parent_zone_is_not_allowed() {
        assert!(!zone_is_allowed(&zones("foo.example.com"), "example.com"));
    }

    #[test]
    fn a_suffix_that_is_not_a_label_boundary_is_not_allowed() {
        assert!(!zone_is_allowed(&zones("example.com"), "badexample.com"));
    }

    // ---------------------------------------------------------------------
    // parse_allowed_ip_ranges and first_ip_outside
    // ---------------------------------------------------------------------

    #[test]
    fn a_bare_address_is_a_single_host_range() {
        let r = ranges("10.0.0.5");
        assert_eq!(first_ip_outside(&r, &ips(&["10.0.0.5"])), None);
        assert_eq!(
            first_ip_outside(&r, &ips(&["10.0.0.6"])),
            Some("10.0.0.6".to_string())
        );
    }

    #[test]
    fn addresses_inside_any_range_pass() {
        let r = ranges("10.0.0.0/24, 192.0.2.0/28");
        assert_eq!(
            first_ip_outside(&r, &ips(&["10.0.0.200", "192.0.2.15"])),
            None
        );
    }

    #[test]
    fn the_first_address_outside_every_range_is_reported() {
        let r = ranges("10.0.0.0/24");
        assert_eq!(
            first_ip_outside(&r, &ips(&["10.0.0.1", "10.0.1.1", "8.8.8.8"])),
            Some("10.0.1.1".to_string())
        );
    }

    #[test]
    fn a_zero_prefix_range_allows_every_address() {
        let r = ranges("0.0.0.0/0");
        assert_eq!(first_ip_outside(&r, &ips(&["203.0.113.9"])), None);
    }

    #[test]
    fn host_bits_in_a_range_are_masked_off() {
        let r = ranges("10.0.0.77/24");
        assert_eq!(first_ip_outside(&r, &ips(&["10.0.0.1"])), None);
    }

    #[test]
    fn an_address_that_does_not_parse_is_outside() {
        let r = ranges("10.0.0.0/8");
        assert_eq!(
            first_ip_outside(&r, &ips(&["not-an-ip"])),
            Some("not-an-ip".to_string())
        );
    }

    #[test]
    fn malformed_ranges_are_rejected() {
        for bad in [
            "10.0.0.0/33",
            "10.0.0/24",
            "10.0.0.0/",
            "10.0.0.0/x",
            "2001:db8::/32",
            "",
        ] {
            assert!(
                parse_allowed_ip_ranges(bad).is_err(),
                "{bad:?} must be refused"
            );
        }
    }
}
