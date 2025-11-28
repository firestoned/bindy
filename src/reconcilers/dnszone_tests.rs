//! Unit tests for dnszone reconciler

#[cfg(test)]
mod tests {
    use crate::crd::*;
    use crate::reconcilers::dnszone::build_label_selector;
    use std::collections::BTreeMap;

    #[test]
    fn test_build_label_selector_with_match_labels() {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "bind9".to_string());
        labels.insert("env".to_string(), "prod".to_string());

        let selector = LabelSelector {
            match_labels: Some(labels),
            match_expressions: None,
        };

        let result = build_label_selector(&selector);
        assert!(result.is_some());

        let selector_str = result.unwrap();
        assert!(selector_str.contains("app=bind9"));
        assert!(selector_str.contains("env=prod"));
    }

    #[test]
    fn test_build_label_selector_empty() {
        let selector = LabelSelector {
            match_labels: None,
            match_expressions: None,
        };

        let result = build_label_selector(&selector);
        assert!(result.is_none());
    }

    #[test]
    fn test_build_label_selector_with_one_label() {
        let mut labels = BTreeMap::new();
        labels.insert("instance".to_string(), "dns-primary".to_string());

        let selector = LabelSelector {
            match_labels: Some(labels),
            match_expressions: None,
        };

        let result = build_label_selector(&selector);
        assert_eq!(result, Some("instance=dns-primary".to_string()));
    }

    #[test]
    fn test_build_label_selector_with_empty_labels() {
        let selector = LabelSelector {
            match_labels: Some(BTreeMap::new()),
            match_expressions: None,
        };

        let result = build_label_selector(&selector);
        assert!(result.is_none());
    }

    #[test]
    fn test_build_label_selector_with_special_characters() {
        let mut labels = BTreeMap::new();
        labels.insert("app.kubernetes.io/name".to_string(), "bind9".to_string());

        let selector = LabelSelector {
            match_labels: Some(labels),
            match_expressions: None,
        };

        let result = build_label_selector(&selector);
        assert_eq!(result, Some("app.kubernetes.io/name=bind9".to_string()));
    }

    #[test]
    fn test_build_label_selector_multiple_labels_order() {
        let mut labels = BTreeMap::new();
        labels.insert("a".to_string(), "1".to_string());
        labels.insert("b".to_string(), "2".to_string());
        labels.insert("c".to_string(), "3".to_string());

        let selector = LabelSelector {
            match_labels: Some(labels),
            match_expressions: None,
        };

        let result = build_label_selector(&selector).unwrap();
        // BTreeMap maintains sorted order
        assert!(result.contains("a=1"));
        assert!(result.contains("b=2"));
        assert!(result.contains("c=3"));
    }
}
