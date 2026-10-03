// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

//! Tests for the CA-pinned TLS client used to reach bindcar sidecars.
//!
//! These exercise the verifier directly rather than over a socket: the
//! property under test is "which certificates are accepted", and asserting it
//! on the verifier is both faster and more precise than driving a handshake.

#[cfg(test)]
mod tests {
    use crate::bind9::tls_client::{build_root_store, CaPinnedVerifier};
    use rustls::client::danger::ServerCertVerifier;
    use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
    use std::sync::Arc;

    /// CA that issued the good leaf below.
    const CA_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDITCCAgmgAwIBAgIUQ+stMZZsv2mtePPpBq42mOmRx54wDQYJKoZIhvcNAQEL
BQAwGDEWMBQGA1UEAwwNYmluZHktdGVzdC1jYTAeFw0yNjA5MTkxNjI1MjJaFw0z
NjA5MTYxNjI1MjJaMBgxFjAUBgNVBAMMDWJpbmR5LXRlc3QtY2EwggEiMA0GCSqG
SIb3DQEBAQUAA4IBDwAwggEKAoIBAQDpl0SR4Wta8B+B4LEohwFUGPiDK+liXk8b
F695sr6GRRh63x55HGcoHuLg22UkOgSbtZ0d0IOpga/kxiF7Z8Bwcwr8pRWDNzpJ
IcGJ49ysTJdhRccH82Sc5w8OK5tfy7UC6qTPTSjkK/PTdNDTVRVcDdqXJoKCujLZ
zj1jCbzgrJONTBvIeCCfZrBxmQ6pIHXDmpmpDjnm01mmjUPT00ZJofAN2G8ulmmf
43wFsVpbpo/xmZq8WybQKoUrLh15NBNxTBGyftQpgrXHRYEcLhsa0fQTzJhSobw2
Gi4ko61rAlULN6edSULn1QkpRr4JTrxFjJYfa1+t8jVh9j560Ws5AgMBAAGjYzBh
MB0GA1UdDgQWBBS6oj53yFjw9vjNd5d6yWmnqTIxmDAfBgNVHSMEGDAWgBS6oj53
yFjw9vjNd5d6yWmnqTIxmDAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIB
BjANBgkqhkiG9w0BAQsFAAOCAQEAHGqtoUo8DYaR5esNU8aoVr54drqlbIGQsu5Q
8xxhJ0pBCw6VnFiBP0w8TQ4BnKbqp8HPIuCobYQDcyelHvLs3aZhQfMFI7CeEh2E
SuS/LKLApDYhJGjRPtPQbegletiObyWsyjyCLoaw2bPE3ajs3MlXqsqm33MPib/l
XXfLuOpkLdhUs58nRHEl8LYqdhbVnizTihrWAMfJPLBySFoNP3mCL+nXQu3NwU8m
GtbKjMGze8zqdlcBhdzMG2lZcJMaHtMx2xhHfigMv2yl8nVe1cp1EMmGfaxL2w0t
EX3UKFxaScUvdZd+re86RSsInPKOfcUQaZ64RIj8g7F7Q6em8Q==
-----END CERTIFICATE-----
"#;

    /// Leaf signed by CA_PEM. SANs: DNS:bindcar.bindy.internal, IP:10.1.2.3
    const SRV_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDWDCCAkCgAwIBAgIUXpC71aBA/XI0FWMoqNefGCj6r3EwDQYJKoZIhvcNAQEL
BQAwGDEWMBQGA1UEAwwNYmluZHktdGVzdC1jYTAeFw0yNjA5MTkxNjI1MjJaFw0z
NjA5MTYxNjI1MjJaMBIxEDAOBgNVBAMMB2JpbmRjYXIwggEiMA0GCSqGSIb3DQEB
AQUAA4IBDwAwggEKAoIBAQCkrerZMGlsZHPXYVZ3yOMV+5zoKtI0hj30EWM+M9RA
gJTy6VE5OnhTfEkSWdAQu5fJudmOsfH1aJKFhAu5Rj1Q/nE5L4AmrysvsCP46QvG
qKMvNFF2KAjLmgAWc7G4HXcq7tlHZ5iVqyVGbAtnaRdWmzN71aZoQUBbVDvQr0pX
3hiYCGQ7/ZEkBANWWZtROSIqIyXrZY7qtkviXYvFUrxUzSDYfMuXApCoJ4DphSP9
q7MdBM0Iu8dQCUBGlkfO6XCJ01RKjfaLAUQVytJBdWO+rj8WwoHXHC2MbAaHwTg9
TjIcety3TWTxCFPBiU+BkwqzST15KQRkE3M3Idk7UwwRAgMBAAGjgZ8wgZwwDAYD
VR0TAQH/BAIwADAOBgNVHQ8BAf8EBAMCBaAwEwYDVR0lBAwwCgYIKwYBBQUHAwEw
JwYDVR0RBCAwHoIWYmluZGNhci5iaW5keS5pbnRlcm5hbIcECgECAzAdBgNVHQ4E
FgQUqutZFiaPoCRbUyboXGZbHfflWScwHwYDVR0jBBgwFoAUuqI+d8hY8Pb4zXeX
eslpp6kyMZgwDQYJKoZIhvcNAQELBQADggEBAEh8kpfOjTkh7KZzYPJN+/cG7foE
eOFjDxHfUwBpK/PxMeWQQGZzZOWpeenNqM3W3vf2R4wJMsqSh2mLcVjfk1ppMuOp
iFfR50ZZ5FajLvqT8gGuKTpmjJ+YAWlN1WMMm3VjXdGAcDz7NaMMyaxDSQOEQ7k0
NjLiU2wKmGL6V5YAVgjMiMxdAJCvQvHiK4IKdFwrH9eHamimgeGtozA8SOhASp/5
9dRAUnh2g50Lpj/vWAZt1GE3sXYbmT7vDFvaaOD9xJqg4OuXHTOdOZC1DVezAfpD
go7jZYKzUiicFAGeiRZTJfQMOPRDKmr4lRrN114ZLdk2IEvljgQquzUMj2Y=
-----END CERTIFICATE-----
"#;

    /// Leaf with the same SANs but signed by a CA the operator does not trust.
    const ROGUE_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDUzCCAjugAwIBAgIUaB+iPH+t0wphvapKq71ToXNSchMwDQYJKoZIhvcNAQEL
BQAwEzERMA8GA1UEAwwIcm9ndWUtY2EwHhcNMjYwOTE5MTYyNTIyWhcNMzYwOTE2
MTYyNTIyWjASMRAwDgYDVQQDDAdiaW5kY2FyMIIBIjANBgkqhkiG9w0BAQEFAAOC
AQ8AMIIBCgKCAQEAwoNZCtsMbh88I5EbSqimifRglbGDNe/g1oaJMaIS0Zyps3am
QVEVeG7G9V8hHX9iRZDcfEQ3tlSjexwjyhMlYX+MxxbX8woAA8aQcr/mEi5KC6As
n4o4HLei4CXrU4AO2jMdRo0wicyOHGnRlYw/HBS3E0T90GFMyYwngL7DGObzU93N
a284o+Qwk/MKxAAZqXkuYbhIAY8KkB3BuozBzx8pQfsQYIyyYGgjunQ7riQua+XZ
Nix4Hw2P2Lpw8w+vnTI2rK6you3F8Jz2yRIm0rjJekrzlkzhQD76mEqKrkE2V8ev
r1RN3Pkrb4PRCubvqG+qVaNMiw7Iq/k9ia0IowIDAQABo4GfMIGcMAwGA1UdEwEB
/wQCMAAwDgYDVR0PAQH/BAQDAgWgMBMGA1UdJQQMMAoGCCsGAQUFBwMBMCcGA1Ud
EQQgMB6CFmJpbmRjYXIuYmluZHkuaW50ZXJuYWyHBAoBAgMwHQYDVR0OBBYEFC1d
IZ/NXdUEPAQWdRRRZJ5bcGaBMB8GA1UdIwQYMBaAFAV1FUIyIpIuB3N3autlBZCI
3izAMA0GCSqGSIb3DQEBCwUAA4IBAQBJMppO+spIUMzOVANOvMCyS0cUn1y+9B/S
nBeIuVEcKzlXtCe71Ovkt7hTBr5Mn+bvzRH+VvCBpnRRB5VQ/02YjewHaWvQZ/YD
7NMyN/9JZiLZOCR8gwS5twNCoHOLVpVIZeH64ZdnKu1cB5dbM6PkPi4dXjER822E
ByBw/jefWpNHwr3u57SED42GYxtLakZTL1ld17yiUtcPNfS5VKMd1m17RugYM8NP
imCBmhYRZQMiMZkgdK9gWcOlC61mKpR2ap2aqujIjoPMd1BSjIYwgTaRMrFt5TOS
o+JgjLIQP9MySCcv40UHZJHsi9lr/Xxg4+nI6jUBYWy/R+hotFLp
-----END CERTIFICATE-----
"#;

    fn leaf(pem: &str) -> CertificateDer<'static> {
        rustls_pki_types::pem::PemObject::from_pem_slice(pem.as_bytes())
            .expect("test leaf must parse")
    }

    fn verifier(server_name: Option<&str>) -> CaPinnedVerifier {
        let roots = build_root_store(CA_PEM.as_bytes()).expect("CA bundle must parse");
        CaPinnedVerifier::new(Arc::new(roots), server_name.map(String::from))
            .expect("verifier must build")
    }

    fn any_name() -> ServerName<'static> {
        ServerName::try_from("10.1.2.3").expect("ip is a valid server name")
    }

    /// A CA bundle with no certificates is a configuration error, not an empty
    /// trust store — an empty store would reject everything and look like a
    /// connectivity fault.
    #[test]
    fn test_empty_ca_bundle_is_rejected() {
        assert!(build_root_store(b"not a certificate\n").is_err());
        assert!(build_root_store(b"").is_err());
    }

    /// The good leaf is accepted even though the address it is dialled at is
    /// not checked against its SANs. This is the ADR-0004 trade-off.
    #[test]
    fn test_accepts_certificate_signed_by_the_configured_ca() {
        let v = verifier(None);
        let r = v.verify_server_cert(&leaf(SRV_PEM), &[], &any_name(), &[], UnixTime::now());
        assert!(
            r.is_ok(),
            "leaf from the configured CA must be accepted: {r:?}"
        );
    }

    /// The property that actually matters: a certificate from any other CA is
    /// refused. Without this the transport would be encrypted but
    /// unauthenticated, which is worse than plaintext because it looks safe.
    #[test]
    fn test_rejects_certificate_from_an_untrusted_ca() {
        let v = verifier(None);
        let r = v.verify_server_cert(&leaf(ROGUE_PEM), &[], &any_name(), &[], UnixTime::now());
        assert!(
            r.is_err(),
            "a leaf signed by an untrusted CA must be rejected even with hostname checking off"
        );
    }

    /// Hostname verification is off by default, so dialling by pod IP works.
    #[test]
    fn test_hostname_is_not_checked_by_default() {
        let v = verifier(None);
        let unrelated = ServerName::try_from("192.0.2.99").expect("valid");
        assert!(v
            .verify_server_cert(&leaf(SRV_PEM), &[], &unrelated, &[], UnixTime::now())
            .is_ok());
    }

    /// With an explicit serverName, full verification applies: a name the
    /// certificate covers is accepted...
    #[test]
    fn test_explicit_server_name_accepts_a_matching_certificate() {
        let v = verifier(Some("bindcar.bindy.internal"));
        let r = v.verify_server_cert(&leaf(SRV_PEM), &[], &any_name(), &[], UnixTime::now());
        assert!(r.is_ok(), "SAN matches the configured serverName: {r:?}");
    }

    /// ...and one it does not cover is refused, so configuring serverName
    /// genuinely tightens verification rather than being decorative.
    #[test]
    fn test_explicit_server_name_rejects_a_non_matching_certificate() {
        let v = verifier(Some("not-in-the-cert.example"));
        let r = v.verify_server_cert(&leaf(SRV_PEM), &[], &any_name(), &[], UnixTime::now());
        assert!(r.is_err(), "serverName not in the SANs must be rejected");
    }

    /// Even with a matching serverName, the chain still has to be trusted.
    #[test]
    fn test_explicit_server_name_still_requires_a_trusted_chain() {
        let v = verifier(Some("bindcar.bindy.internal"));
        let r = v.verify_server_cert(&leaf(ROGUE_PEM), &[], &any_name(), &[], UnixTime::now());
        assert!(
            r.is_err(),
            "serverName must not substitute for chain verification"
        );
    }
}
