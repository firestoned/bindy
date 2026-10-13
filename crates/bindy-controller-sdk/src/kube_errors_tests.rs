// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `kube_errors.rs`

#[cfg(test)]
mod tests {
    use crate::kube_errors::{is_conflict, is_conflict_anyhow, is_not_found, is_not_found_anyhow};
    use anyhow::Context as _;
    use bindy_api::constants::{HTTP_CONFLICT, HTTP_NOT_FOUND};

    const HTTP_FORBIDDEN: u16 = 403;
    const HTTP_TOO_MANY_REQUESTS: u16 = 429;
    const HTTP_INTERNAL_SERVER_ERROR: u16 = 500;

    fn api_error(code: u16, reason: &str) -> kube::Error {
        kube::Error::Api(
            kube::core::Status::failure("mock failure", reason)
                .with_code(code)
                .boxed(),
        )
    }

    /// A non-API kube error (a request that could not be built).
    fn non_api_error() -> kube::Error {
        kube::Error::LinesCodecMaxLineLengthExceeded
    }

    #[test]
    fn constants_carry_the_http_codes() {
        assert_eq!(HTTP_NOT_FOUND, 404);
        assert_eq!(HTTP_CONFLICT, 409);
    }

    #[test]
    fn not_found_is_the_404_api_error_only() {
        assert!(is_not_found(&api_error(HTTP_NOT_FOUND, "NotFound")));
        for code in [
            HTTP_CONFLICT,
            HTTP_FORBIDDEN,
            HTTP_TOO_MANY_REQUESTS,
            HTTP_INTERNAL_SERVER_ERROR,
        ] {
            assert!(
                !is_not_found(&api_error(code, "Other")),
                "HTTP {code} is not not-found"
            );
        }
        assert!(!is_not_found(&non_api_error()));
    }

    #[test]
    fn not_found_goes_by_the_code_not_the_reason_text() {
        // A 404 whose reason is not "NotFound" is still not found, and a
        // "NotFound" reason on another code is not.
        assert!(is_not_found(&api_error(HTTP_NOT_FOUND, "Gone")));
        assert!(!is_not_found(&api_error(
            HTTP_INTERNAL_SERVER_ERROR,
            "NotFound"
        )));
    }

    #[test]
    fn conflict_is_the_409_api_error_including_already_exists() {
        assert!(is_conflict(&api_error(HTTP_CONFLICT, "AlreadyExists")));
        assert!(is_conflict(&api_error(HTTP_CONFLICT, "Conflict")));
        assert!(!is_conflict(&api_error(HTTP_NOT_FOUND, "NotFound")));
        assert!(!is_conflict(&non_api_error()));
    }

    #[test]
    fn anyhow_not_found_sees_a_converted_kube_error() {
        let err = anyhow::Error::from(api_error(HTTP_NOT_FOUND, "NotFound"));
        assert!(is_not_found_anyhow(&err));
        assert!(!is_conflict_anyhow(&err));
    }

    #[test]
    fn anyhow_helpers_see_through_context() {
        let err = Err::<(), _>(api_error(HTTP_NOT_FOUND, "NotFound"))
            .context("reading the parent cluster")
            .context("reconciling the instance")
            .unwrap_err();
        assert!(is_not_found_anyhow(&err));

        let err = Err::<(), _>(api_error(HTTP_CONFLICT, "AlreadyExists"))
            .context("creating the instance")
            .unwrap_err();
        assert!(is_conflict_anyhow(&err));
        assert!(!is_not_found_anyhow(&err));
    }

    #[test]
    fn anyhow_helpers_see_a_kube_error_that_is_another_errors_source() {
        #[derive(Debug, thiserror::Error)]
        #[error("wrapped")]
        struct Wrapper(#[source] kube::Error);

        let err = anyhow::Error::from(Wrapper(api_error(HTTP_NOT_FOUND, "NotFound")));
        assert!(is_not_found_anyhow(&err));
    }

    #[test]
    fn anyhow_helpers_reject_other_errors() {
        assert!(!is_not_found_anyhow(&anyhow::anyhow!("NotFound")));
        assert!(!is_conflict_anyhow(&anyhow::anyhow!("AlreadyExists")));
        let err = anyhow::Error::from(api_error(HTTP_INTERNAL_SERVER_ERROR, "InternalError"));
        assert!(!is_not_found_anyhow(&err));
        assert!(!is_conflict_anyhow(&err));
    }
}
