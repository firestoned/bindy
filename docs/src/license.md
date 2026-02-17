# License

Bindy is licensed under the MIT License.

**SPDX-License-Identifier:** MIT

**Copyright (c) 2025 Erick Bourgeois, firestoned**

## MIT License

Copyright (c) 2025 Erick Bourgeois, firestoned

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## What This Means for You

The MIT License is one of the most permissive open source licenses. Here's what it allows:

### ✅ You Can

- **Use commercially** - Use Bindy in your commercial products and services
- **Modify** - Change the code to fit your needs
- **Distribute** - Share the original or your modified version
- **Sublicense** - Include Bindy in proprietary software
- **Private use** - Use Bindy for private/internal purposes without releasing your modifications

### ⚠️ Requirements

- **Include the license** - Include the copyright notice and license text in substantial portions of the software
- **State changes** - Document any modifications you make (recommended best practice)

### ❌ Limitations

- **No warranty** - The software is provided "as is" without warranty of any kind
- **No liability** - The authors are not liable for any damages arising from the use of the software

## SPDX License Identifiers

All source code files in this project include SPDX license identifiers for machine-readable license information:

```rust
// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT
```

This makes it easy for automated tools to:
- Scan the codebase for license compliance
- Generate Software Bill of Materials (SBOM)
- Verify license compatibility

Learn more about SPDX at [https://spdx.dev/](https://spdx.dev/)

## Software Bill of Materials (SBOM)

Bindy provides SBOM files in CycloneDX format with every release. These include:

- Binary SBOMs for each platform (Linux, macOS, Windows)
- Docker image SBOM
- Complete dependency tree with license information

SBOMs are available as release assets and can be used for:
- Supply chain security
- Vulnerability scanning
- License compliance auditing
- Dependency tracking

## Third-Party Licenses

Bindy depends on various open-source libraries. All dependencies are permissively licensed and compatible with the MIT License.

### Key Dependencies

| Library | License | Purpose |
|---------|---------|---------|
| **kube-rs** | Apache 2.0 / MIT | Kubernetes client library |
| **tokio** | MIT | Async runtime |
| **serde** | Apache 2.0 / MIT | Serialization framework |
| **tracing** | MIT | Structured logging |
| **anyhow** | Apache 2.0 / MIT | Error handling |
| **thiserror** | Apache 2.0 / MIT | Error derivation |

### Generating License Reports

For a complete list of all dependencies and their licenses:

```bash
# Install cargo-license tool
cargo install cargo-license

# Generate license report
cargo license

# Generate detailed license report with full license text
cargo license --json > licenses.json
```

You can also use [cargo-about](https://github.com/EmbarkStudios/cargo-about) for more detailed license auditing:

```bash
cargo install cargo-about
cargo about generate about.hbs > licenses.html
```

## Container Image Licenses

The Docker images for Bindy include:

- **Base Image**: Alpine Linux (MIT License)
- **BIND9**: ISC License (permissive, BSD-style)
- **Bindy Binary**: MIT License

All components are open source and permissively licensed.

## Contributing

By contributing to Bindy, you agree that:

1. Your contributions will be licensed under the MIT License
2. You have the right to submit the contributions
3. You grant the project maintainers a perpetual, worldwide, non-exclusive, royalty-free license to use your contributions

See the [Contributing Guidelines](./development/contributing.md) for more information on how to contribute.

## License Compatibility

The MIT License is compatible with most other open source licenses, including:

- ✅ Apache License 2.0
- ✅ BSD licenses (2-clause, 3-clause)
- ✅ GPL v2 and v3 (one-way compatible - MIT code can be included in GPL projects)
- ✅ ISC License
- ✅ Other MIT-licensed code

This makes Bindy easy to integrate into various projects and environments.

## Questions About Licensing

If you have questions about:

- Using Bindy in your project
- License compliance
- Contributing to Bindy
- Third-party dependencies

Please open a [GitHub Discussion](https://github.com/firestoned/bindy/discussions) or contact the maintainers.

## Additional Resources

- [Full License Text](https://github.com/firestoned/bindy/blob/main/LICENSE)
- [MIT License on OSI](https://opensource.org/licenses/MIT)
- [SPDX MIT License](https://spdx.org/licenses/MIT.html)
- [GitHub's Choose a License - MIT](https://choosealicense.com/licenses/mit/)
- [SPDX Specification](https://spdx.github.io/spdx-spec/)
