# Changelog

All notable changes to Bindy will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **DNSZone tight reconciliation loop** - Added status change detection to prevent unnecessary status updates and reconciliation cycles (2025-12-01)

### Added
- Comprehensive documentation with mdBook and rustdoc
- GitHub Pages deployment workflow
- Status update optimization documentation in performance guide

## [0.1.0] - 2024-01-01

### Added
- Initial release of Bindy
- Bind9Instance CRD for managing BIND9 DNS server instances
- DNSZone CRD with label selector support
- DNS record CRDs: A, AAAA, CNAME, MX, TXT, NS, SRV, CAA
- Reconciliation operators for all resource types
- BIND9 zone file generation
- Status subresources for all CRDs
- RBAC configuration
- Docker container support
- Comprehensive test suite
- CI/CD with GitHub Actions
- Integration tests with Kind

### Features
- High-performance Rust implementation
- Async/await with Tokio runtime
- Label-based instance targeting
- Primary and secondary DNS support
- Multi-region deployment support
- Full status reporting
- Kubernetes 1.24+ support

## Links

- [GitHub Repository](https://github.com/firestoned/bindy)
- [Documentation](https://firestoned.github.io/bindy/)
- [Issue Tracker](https://github.com/firestoned/bindy/issues)
