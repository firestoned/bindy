//! Unit tests for CRD documentation examples

#[cfg(test)]
mod tests {
    use crate::crd_docs::CRDExamples;

    #[test]
    fn test_crd_examples_struct_exists() {
        // Verify the struct is defined
        let _examples: CRDExamples = CRDExamples;
    }
}
