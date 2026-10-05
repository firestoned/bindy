// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `config_check.rs`

#[cfg(test)]
mod tests {
    use crate::config_check::{check_named_conf_files, find_invalid_config, InvalidBind9Config};
    use std::collections::BTreeMap;

    fn files(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(name, text)| ((*name).to_string(), (*text).to_string()))
            .collect()
    }

    const VALID_OPTIONS: &str =
        "options {\n    directory \"/var/cache/bind\";\n    recursion no;\n};\n";

    #[test]
    fn a_valid_configuration_passes() {
        let data = files(&[
            ("named.conf", "include \"/etc/bind/named.conf.options\";\n"),
            ("named.conf.options", VALID_OPTIONS),
        ]);
        assert!(check_named_conf_files(&data).is_ok());
    }

    #[test]
    fn a_syntax_error_is_refused_and_names_the_file() {
        // bug-177's shape: a stray closing brace in the options file.
        let data = files(&[
            ("named.conf", "include \"/etc/bind/named.conf.options\";\n"),
            (
                "named.conf.options",
                "options {\n    recursion no;\n};\n};\n",
            ),
        ]);
        let error = check_named_conf_files(&data).expect_err("a stray brace must be refused");
        assert_eq!(error.file, "named.conf.options");
        assert!(error.to_string().contains("named.conf.options"), "{error}");
    }

    #[test]
    fn an_error_severity_finding_is_refused() {
        let data = files(&[(
            "named.conf.options",
            "options {\n    allow-query { undefined-acl; };\n};\n",
        )]);
        let error = check_named_conf_files(&data).expect_err("an undefined ACL must be refused");
        assert_eq!(error.file, "named.conf.options");
        assert!(error.detail.contains("undefined ACL"), "{}", error.detail);
    }

    #[test]
    fn warnings_do_not_block_and_are_returned() {
        // Authoritative-only shape: validation on, recursion off.
        let data = files(&[(
            "named.conf.options",
            "options {\n    recursion no;\n    dnssec-validation yes;\n};\n",
        )]);
        let warnings = check_named_conf_files(&data).expect("warnings never block");
        assert!(
            warnings.iter().any(|w| w.contains("named.conf.options")),
            "warnings name their file: {warnings:?}"
        );
    }

    #[test]
    fn files_that_are_not_named_conf_are_ignored() {
        // rndc.conf is the rndc client's grammar, not named.conf's.
        let data = files(&[
            (
                "rndc.conf",
                "# comment\noptions { default-server 127.0.0.1; };\n",
            ),
            ("named.conf.options", VALID_OPTIONS),
        ]);
        assert!(check_named_conf_files(&data).is_ok());
    }

    #[test]
    fn an_empty_configuration_has_nothing_to_refuse() {
        assert!(check_named_conf_files(&BTreeMap::new()).is_ok());
    }

    fn invalid() -> InvalidBind9Config {
        InvalidBind9Config {
            file: "named.conf.options".to_string(),
            detail: "'}' expected".to_string(),
        }
    }

    #[test]
    fn find_invalid_config_finds_it_directly_and_under_context() {
        let direct = anyhow::Error::from(invalid());
        assert_eq!(find_invalid_config(&direct), Some(&invalid()));

        let wrapped = anyhow::Error::from(invalid()).context("Failed to build ConfigMap");
        assert_eq!(find_invalid_config(&wrapped), Some(&invalid()));
    }

    #[test]
    fn find_invalid_config_ignores_other_errors() {
        assert_eq!(
            find_invalid_config(&anyhow::anyhow!("API server unreachable")),
            None
        );
    }
}
