// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    use crate::bind9_acl::{build_acl_list, validate_acl_entry, AclError, MAX_ACL_ENTRY_LEN};

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

    #[test]
    fn build_acl_list_joins_valid_entries() {
        let entries = vec![
            "10.0.0.0/8".to_string(),
            "192.168.0.0/16".to_string(),
            "any".to_string(),
        ];
        assert_eq!(
            build_acl_list(&entries).unwrap(),
            "10.0.0.0/8; 192.168.0.0/16; any"
        );
    }

    #[test]
    fn build_acl_list_rejects_on_first_bad_entry() {
        let entries = vec![
            "10.0.0.0/8".to_string(),
            "}; exec;".to_string(),
            "any".to_string(),
        ];
        assert!(matches!(
            build_acl_list(&entries),
            Err(AclError::InvalidToken(_))
        ));
    }

    #[test]
    fn build_acl_list_handles_empty_input() {
        let entries: Vec<String> = Vec::new();
        assert_eq!(build_acl_list(&entries).unwrap(), "");
    }

    #[test]
    fn build_acl_list_trims_whitespace() {
        let entries = vec!["  10.0.0.0/8  ".to_string()];
        assert_eq!(build_acl_list(&entries).unwrap(), "10.0.0.0/8");
    }
}
