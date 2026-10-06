// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod tests {
    use crate::bind9_acl::{
        parse_acl_entry, parse_acl_list, validate_acl_entry, AclError, MAX_ACL_ENTRY_LEN,
    };
    use hornet_bind9::named_conf::AddressMatchElement as Element;
    use std::net::IpAddr;

    #[test]
    fn accepts_keyword_any() {
        assert!(validate_acl_entry("any").is_ok());
    }

    #[test]
    fn accepts_all_reserved_keywords() {
        for kw in ["any", "none", "localhost", "localnets"] {
            assert!(validate_acl_entry(kw).is_ok(), "expected {kw} accepted");
        }
    }

    #[test]
    fn accepts_negated_keyword() {
        assert!(validate_acl_entry("!any").is_ok());
        assert!(validate_acl_entry("! localhost").is_ok());
    }

    #[test]
    fn accepts_ipv4_address() {
        assert!(validate_acl_entry("10.0.0.1").is_ok());
        assert!(validate_acl_entry("192.168.1.100").is_ok());
    }

    #[test]
    fn accepts_ipv4_cidr() {
        assert!(validate_acl_entry("10.0.0.0/8").is_ok());
        assert!(validate_acl_entry("0.0.0.0/0").is_ok());
        assert!(validate_acl_entry("172.16.0.0/12").is_ok());
    }

    #[test]
    fn rejects_ipv4_prefix_over_32() {
        assert!(matches!(
            validate_acl_entry("10.0.0.0/33"),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn accepts_ipv6_address_and_cidr() {
        assert!(validate_acl_entry("2001:db8::1").is_ok());
        assert!(validate_acl_entry("2001:db8::/32").is_ok());
        assert!(validate_acl_entry("::/0").is_ok());
    }

    #[test]
    fn rejects_ipv6_prefix_over_128() {
        assert!(matches!(
            validate_acl_entry("2001:db8::/129"),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn accepts_key_reference() {
        assert!(validate_acl_entry("key bindy-operator").is_ok());
        assert!(validate_acl_entry("key \"bindy-operator\"").is_ok());
        assert!(validate_acl_entry("!key bindy-operator").is_ok());
    }

    #[test]
    fn rejects_key_name_with_bad_chars() {
        assert!(matches!(
            validate_acl_entry("key bad name"),
            Err(AclError::InvalidToken(_))
        ));
        assert!(matches!(
            validate_acl_entry("key bad;name"),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn rejects_empty_entry() {
        assert_eq!(validate_acl_entry(""), Err(AclError::Empty));
        assert_eq!(validate_acl_entry("   "), Err(AclError::Empty));
    }

    #[test]
    fn rejects_injection_with_semicolon_and_brace() {
        // The H1 attack shape: close the ACL block and inject a zone directive.
        let payload =
            "any; }; zone \"evil.example\" { type master; file \"/etc/passwd\"; }; acl x { any";
        assert!(matches!(
            validate_acl_entry(payload),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn rejects_entry_with_bare_semicolon() {
        assert!(matches!(
            validate_acl_entry("10.0.0.0/8; any"),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn rejects_entry_with_brace() {
        assert!(matches!(
            validate_acl_entry("{ any; }"),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn rejects_entry_exceeding_max_length() {
        let long = "a".repeat(MAX_ACL_ENTRY_LEN + 1);
        assert!(matches!(
            validate_acl_entry(&long),
            Err(AclError::TooLong(_))
        ));
    }

    #[test]
    fn rejects_unknown_keyword() {
        assert!(matches!(
            validate_acl_entry("anyone"),
            Err(AclError::InvalidToken(_))
        ));
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("test address")
    }

    #[test]
    fn parse_acl_entry_maps_keywords() {
        assert_eq!(parse_acl_entry("any"), Ok(Element::Any));
        assert_eq!(parse_acl_entry("none"), Ok(Element::None));
        assert_eq!(parse_acl_entry("localhost"), Ok(Element::Localhost));
        assert_eq!(parse_acl_entry("localnets"), Ok(Element::Localnets));
    }

    #[test]
    fn parse_acl_entry_maps_addresses_and_prefixes() {
        assert_eq!(parse_acl_entry("10.0.0.1"), Ok(Element::Ip(ip("10.0.0.1"))));
        assert_eq!(
            parse_acl_entry("10.0.0.0/8"),
            Ok(Element::Cidr {
                addr: ip("10.0.0.0"),
                prefix_len: 8
            })
        );
        assert_eq!(
            parse_acl_entry("2001:db8::/32"),
            Ok(Element::Cidr {
                addr: ip("2001:db8::"),
                prefix_len: 32
            })
        );
    }

    #[test]
    fn parse_acl_entry_maps_keys_with_or_without_quotes() {
        let key = Ok(Element::Key("bindy-operator".to_string()));
        assert_eq!(parse_acl_entry("key bindy-operator"), key);
        assert_eq!(parse_acl_entry("key \"bindy-operator\""), key);
    }

    #[test]
    fn parse_acl_entry_maps_negation_and_trims() {
        assert_eq!(
            parse_acl_entry("  ! localhost "),
            Ok(Element::Negated(Box::new(Element::Localhost)))
        );
        assert_eq!(
            parse_acl_entry("!key k1"),
            Ok(Element::Negated(Box::new(Element::Key("k1".to_string()))))
        );
    }

    #[test]
    fn parse_acl_entry_rejects_what_validation_rejects() {
        assert_eq!(parse_acl_entry(""), Err(AclError::Empty));
        assert!(matches!(
            parse_acl_entry("any; }; zone \"evil\" {"),
            Err(AclError::InvalidToken(_))
        ));
        assert!(matches!(
            parse_acl_entry("10.0.0.0/33"),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn parse_acl_list_keeps_order() {
        let entries = vec![
            "10.0.0.0/8".to_string(),
            "localhost".to_string(),
            "any".to_string(),
        ];
        assert_eq!(
            parse_acl_list(&entries).unwrap(),
            vec![
                Element::Cidr {
                    addr: ip("10.0.0.0"),
                    prefix_len: 8
                },
                Element::Localhost,
                Element::Any,
            ]
        );
    }

    #[test]
    fn parse_acl_list_rejects_on_first_bad_entry() {
        let entries = vec![
            "10.0.0.0/8".to_string(),
            "}; exec;".to_string(),
            "any".to_string(),
        ];
        assert!(matches!(
            parse_acl_list(&entries),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn parse_acl_list_handles_empty_input() {
        assert!(parse_acl_list(&[]).unwrap().is_empty());
    }
}
