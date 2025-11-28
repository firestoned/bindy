// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Unit tests for CRD documentation examples

#[cfg(test)]
mod tests {
    use crate::crd_docs::CRDExamples;

    #[test]
    fn test_crd_examples_struct_exists() {
        // Verify the struct is defined
        #[allow(clippy::no_effect_underscore_binding)]
        let _examples: CRDExamples = CRDExamples;
    }
}
