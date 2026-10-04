// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `labels.rs`

#[cfg(test)]
mod tests {
    use super::super::{BINDY_PART_OF_SELECTOR, K8S_PART_OF, PART_OF_BINDY};

    #[test]
    fn part_of_selector_matches_the_label_bindy_sets() {
        assert_eq!(
            BINDY_PART_OF_SELECTOR,
            format!("{K8S_PART_OF}={PART_OF_BINDY}")
        );
    }
}
