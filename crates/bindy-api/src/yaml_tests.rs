// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `yaml.rs`

#[cfg(test)]
mod tests {
    use crate::yaml::to_string;
    use serde_json::json;

    /// Longer than any fold width a YAML emitter defaults to.
    const LONG_DESCRIPTION: &str = "ARecord maps a DNS hostname to an IPv4 address. Multiple A \
        records for the same name enable round-robin DNS load balancing across several servers.";

    #[test]
    fn no_line_ends_in_whitespace() {
        let value = json!({"spec": {"description": "first paragraph\n\nsecond paragraph\n"}});

        let yaml = to_string(&value).expect("serializes");

        for line in yaml.lines() {
            assert_eq!(line, line.trim_end(), "trailing whitespace in {line:?}");
        }
    }

    #[test]
    fn a_long_single_line_string_stays_on_one_line() {
        let value = json!({"description": LONG_DESCRIPTION});

        let yaml = to_string(&value).expect("serializes");

        assert!(!yaml.contains(">-"), "folded instead of plain: {yaml}");
        assert_eq!(yaml.lines().count(), 1, "wrapped across lines: {yaml}");
    }

    #[test]
    fn output_parses_back_to_the_same_value() {
        let value = json!({
            "description": LONG_DESCRIPTION,
            "multiline": "line one\n\n  indented line\nlast\n",
            "quoted": "Status of the condition: True, False, or Unknown.",
            "flag": "true",
            "empty_map": {},
            "empty_list": [],
            "nested": {"list": [{"name": "a"}, {"name": "b"}]},
        });

        let yaml = to_string(&value).expect("serializes");
        let back: serde_json::Value = serde_saphyr::from_str(&yaml).expect("parses");

        assert_eq!(back, value);
    }
}
