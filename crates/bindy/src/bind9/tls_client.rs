// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! CA-pinned TLS client for reaching bindcar sidecars.
//!
//! # Why this exists
//!
//! The operator reaches each sidecar at its **pod IP**, because a zone
//! operation must land on every replica of an instance and a Service ClusterIP
//! would load-balance it to one. A certificate cannot carry a SAN for an
//! ephemeral pod IP, so ordinary TLS hostname verification cannot succeed.
//!
//! [`CaPinnedVerifier`] therefore verifies that the presented certificate
//! **chains to a CA bundle the operator is configured with**, and by default
//! does not check the dialled address against the certificate's SANs.
//!
//! That encrypts the ServiceAccount token in transit and requires the peer to
//! hold a key signed by a CA the platform team controls. It does **not** bind
//! the certificate to an address: any certificate from that CA is accepted from
//! any pod, so the CA must be dedicated to issuing sidecar certificates.
//!
//! Setting `serverName` on the CRD restores full verification for deployments
//! whose certificates can cover a stable name.
//!
//! See ADR-0004.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::{DigitallySignedStruct, RootCertStore, SignatureScheme};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

/// Parse a PEM CA bundle into a rustls trust store.
///
/// # Arguments
/// * `pem` - PEM bytes, one or more CA certificates
///
/// # Errors
/// Returns an error when the bundle contains no usable certificate. An empty
/// trust store is treated as a configuration error rather than an empty set of
/// roots: silently trusting nothing would reject every connection and present
/// as a connectivity fault rather than as the misconfiguration it is.
pub fn build_root_store(pem: &[u8]) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    let mut added = 0usize;

    for cert in CertificateDer::pem_slice_iter(pem) {
        let cert = cert.context("failed to parse a certificate in the CA bundle")?;
        roots
            .add(cert)
            .context("failed to add a certificate from the CA bundle to the trust store")?;
        added += 1;
    }

    if added == 0 {
        return Err(anyhow!(
            "TLS CA bundle contained no certificates; refusing to build a trust store that \
             would reject every connection"
        ));
    }

    Ok(roots)
}

/// Verifies the sidecar certificate against a configured CA.
///
/// Chain verification is always delegated to rustls' own
/// [`WebPkiServerVerifier`], which is what keeps this type small: the only
/// thing it changes is *which name* the chain is verified against.
///
/// - With `server_name` set, that name is substituted for the dialled address,
///   giving full hostname verification against a name the operator chose.
/// - With `server_name` unset, the certificate's own subject is used, which in
///   practice means the SAN check cannot fail — the chain check still applies.
#[derive(Debug)]
pub struct CaPinnedVerifier {
    inner: Arc<WebPkiServerVerifier>,
    /// When set, the name the certificate must actually cover.
    server_name: Option<String>,
    /// Trust anchors, retained for the name-less chain-only path.
    roots: Arc<RootCertStore>,
}

impl CaPinnedVerifier {
    /// Build a verifier over `roots`.
    ///
    /// # Arguments
    /// * `roots` - trust anchors from [`build_root_store`]
    /// * `server_name` - optional name to demand in the certificate
    ///
    /// # Errors
    /// Returns an error if rustls rejects the trust anchors.
    pub fn new(roots: Arc<RootCertStore>, server_name: Option<String>) -> Result<Self> {
        let inner = WebPkiServerVerifier::builder(Arc::clone(&roots))
            .build()
            .context("failed to build the webpki server verifier")?;

        Ok(Self {
            inner,
            server_name,
            roots,
        })
    }

    /// The trust anchors this verifier pins to.
    #[must_use]
    pub fn roots(&self) -> &RootCertStore {
        &self.roots
    }
}

impl ServerCertVerifier for CaPinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // With an explicit name, verify against it rather than the dialled
        // address: full chain + hostname verification.
        if let Some(expected) = &self.server_name {
            let expected = ServerName::try_from(expected.as_str())
                .map_err(|_| rustls::Error::General("invalid configured serverName".into()))?
                .to_owned();

            return self.inner.verify_server_cert(
                end_entity,
                intermediates,
                &expected,
                ocsp_response,
                now,
            );
        }

        // No configured name: verify the chain, and derive the name to check
        // from the certificate itself so the SAN comparison is satisfied by
        // construction. The trust decision therefore rests entirely on the
        // chain reaching a configured root — which is the ADR-0004 contract.
        //
        // Deliberately NOT a blanket `Ok(...)`: that would accept any
        // self-signed certificate and make the transport encrypted but
        // unauthenticated.
        let parsed = webpki::EndEntityCert::try_from(end_entity)
            .map_err(|e| rustls::Error::General(format!("malformed server certificate: {e}")))?;

        let anchors: Vec<_> = self.roots.roots.clone();
        parsed
            .verify_for_usage(
                rustls::crypto::ring::default_provider()
                    .signature_verification_algorithms
                    .all,
                &anchors,
                intermediates,
                now,
                webpki::KeyUsage::server_auth(),
                None,
                None,
            )
            .map_err(|e| {
                rustls::Error::General(format!(
                    "server certificate does not chain to the configured CA bundle: {e}"
                ))
            })?;

        let _ = server_name;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Build an HTTP client that trusts only `ca_pem` when talking to sidecars.
///
/// # Arguments
/// * `ca_pem` - PEM CA bundle the sidecar certificate must chain to
/// * `server_name` - optional name to demand, enabling full hostname verification
/// * `connect_timeout` - TCP/TLS connect timeout
/// * `request_timeout` - whole-request timeout
///
/// # Errors
/// Returns an error if the CA bundle is unusable or the client cannot be built.
/// Callers must **not** fall back to a default client on error: doing so would
/// silently drop to unverified or plaintext transport. Fail the reconcile
/// instead.
pub fn build_tls_client(
    ca_pem: &[u8],
    server_name: Option<String>,
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
) -> Result<reqwest::Client> {
    let roots = Arc::new(build_root_store(ca_pem)?);
    let verifier = Arc::new(CaPinnedVerifier::new(roots, server_name)?);

    let tls_config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .context("failed to select TLS protocol versions")?
    .dangerous()
    .with_custom_certificate_verifier(verifier)
    .with_no_client_auth();

    reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .connect_timeout(connect_timeout)
        .timeout(request_timeout)
        .build()
        .context("failed to build the TLS HTTP client")
}

#[path = "tls_client_tests.rs"]
mod tls_client_tests;
