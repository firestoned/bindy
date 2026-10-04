# License

Bindy is licensed under the Apache License, Version 2.0.

**SPDX-License-Identifier:** Apache-2.0

**Copyright 2025-2026 Erick Bourgeois, firestoned**

## Apache License 2.0

Licensed under the Apache License, Version 2.0 (the "License"); you may not
use this software except in compliance with the License. You may obtain a
copy of the License at

<http://www.apache.org/licenses/LICENSE-2.0>

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS, WITHOUT
WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the
License for the specific language governing permissions and limitations
under the License.

The full text is in [LICENSE](https://github.com/firestoned/bindy/blob/main/LICENSE)
at the repository root, together with the
[NOTICE](https://github.com/firestoned/bindy/blob/main/NOTICE) file that
section 4(d) of the License asks redistributors to preserve.

## What the License Allows

**Permissions:**

- ✅ Commercial use
- ✅ Modification
- ✅ Distribution
- ✅ Private use
- ✅ Patent use - contributors grant an express patent license (section 3)

**Conditions:**

- 📋 Include the license text and the NOTICE file when redistributing
- 📋 State significant changes made to the code
- 📋 Preserve copyright, patent, trademark and attribution notices

**Limitations:**

- ❌ No trademark rights
- ❌ No liability
- ❌ No warranty

Patent retaliation: the patent grant terminates for anyone who starts
patent litigation claiming the software infringes (section 3). This is the
main practical difference from the MIT license Bindy used before
2026-10-04, and a protection, not a restriction, for users.

## SPDX Headers

Every source file carries the project header, verified in CI on every pull
request:

```rust
// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
```

## Dependencies

Bindy depends only on permissively licensed libraries, enforced by
`cargo-deny` against the allow-list in `.cargo/deny.toml` (copyleft
licenses are refused). Representative core dependencies:

| Dependency | License | Purpose |
|------------|---------|---------|
| **kube-rs** | Apache-2.0 | Kubernetes client library |
| **tokio** | MIT | Async runtime |
| **serde** | MIT OR Apache-2.0 | Serialization framework |
| **tracing** | MIT | Structured logging |
| **anyhow** / **thiserror** | MIT OR Apache-2.0 | Error handling |
| **hickory-proto** | MIT OR Apache-2.0 | DNS protocol / TSIG |
| **rustls** / **ring** | Apache-2.0 / ISC-style | TLS and cryptography |

The authoritative per-release inventory is the SBOM shipped with every
release (see [Signed Releases](security/signed-releases.md)); the full
policy is in [License Policy](security/license-policy.md).

## Container Images

- **Bindy operator images** (Chainguard and Distroless variants) contain
  the bindy binary under Apache-2.0 on their respective minimal base
  images.
- **The BIND9 operand image** is upstream ISC BIND 9, licensed under the
  Mozilla Public License 2.0; bindy configures it but does not
  redistribute modified BIND sources.

## Contributions

The Apache License 2.0 carries its own contribution terms (section 5):
unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in Bindy is licensed under Apache-2.0, without
additional terms. You retain copyright to your contributions. See
[Contributing](development/contributing.md).

## License Compatibility

- ✅ Code under MIT, BSD (2- and 3-clause), ISC, CC0/Unlicense can be
  incorporated into Bindy
- ✅ Apache-2.0 code can be included in GPLv3 projects (one-way)
- ⚠️ Apache-2.0 is **not** compatible with GPLv2-only projects
- ✅ Bindy itself remains free to use in commercial and proprietary
  deployments

## Questions About Licensing

If you have questions about using Bindy in your project, license
compliance, contributing, or third-party dependencies, open a
[GitHub Discussion](https://github.com/firestoned/bindy/discussions) or
contact the maintainers.

## Additional Resources

- [Full License Text](https://github.com/firestoned/bindy/blob/main/LICENSE)
- [Apache License 2.0 (apache.org)](https://www.apache.org/licenses/LICENSE-2.0)
- [SPDX Apache-2.0](https://spdx.org/licenses/Apache-2.0.html)
- [Choose a License - Apache 2.0](https://choosealicense.com/licenses/apache-2.0/)
- [SPDX Specification](https://spdx.github.io/spdx-spec/)
