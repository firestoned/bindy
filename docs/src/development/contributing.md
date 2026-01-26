# Contributing

Thank you for contributing to Bindy!

## Ways to Contribute

- Report bugs
- Suggest features
- Improve documentation
- Submit code changes
- Review pull requests

## Getting Started

1. [Set up development environment](./setup.md)
2. Read [Code Style](./code-style.md)
3. Check [Testing Guide](./testing-guide.md)
4. Follow [PR Process](./pr-process.md)

## Code of Conduct

Be respectful, inclusive, and professional.

## Reporting Issues

Use GitHub issues with:
- Clear description
- Steps to reproduce
- Expected vs actual behavior
- Environment details

## Feature Requests

Open an issue describing:
- Use case
- Proposed solution
- Alternatives considered

## Questions

Ask questions in:
- GitHub Discussions
- Issues (tagged as question)

## License

### Contributor License Agreement

By contributing to Bindy, you agree that:

1. **Your contributions will be licensed under the MIT License** - The same license that covers the project
2. **You have the right to submit the work** - You own the copyright or have permission from the copyright holder
3. **You grant a perpetual license** - The project maintainers receive a perpetual, worldwide, non-exclusive, royalty-free, irrevocable license to use, modify, and distribute your contributions

### What This Means

When you submit a pull request or contribution to Bindy:

- ✅ Your code will be licensed under the **MIT License**
- ✅ You retain copyright to your contributions
- ✅ Others can use your contributions under the MIT License terms
- ✅ Your contributions can be used in both open source and commercial projects
- ✅ You grant irrevocable permission for the project to use your work

### SPDX License Identifiers

All source code files in Bindy include SPDX license identifiers. When adding new files, please include the following header:

**For Rust files:**
```rust
// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT
```

**For shell scripts:**
```bash
#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
```

**For YAML/configuration files:**
```yaml
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
```

**For Makefiles and Dockerfiles:**
```makefile
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
```

### Why SPDX Identifiers?

SPDX (Software Package Data Exchange) identifiers provide:

- **Machine-readable license information** - Automated tools can scan and verify licenses
- **SBOM generation** - Software Bill of Materials can be automatically created
- **License compliance** - Makes it easier to track and verify licensing
- **Industry standard** - Widely adopted across open source projects

Learn more: [https://spdx.dev/](https://spdx.dev/)

### Third-Party Code

If you're adding code from another source:

1. **Ensure compatibility** - The license must be compatible with MIT
2. **Preserve original copyright** - Keep the original copyright notice
3. **Document the source** - Note where the code came from
4. **Check license requirements** - Some licenses require attribution or notices

Compatible licenses include:
- ✅ MIT License
- ✅ Apache License 2.0
- ✅ BSD licenses (2-clause, 3-clause)
- ✅ ISC License
- ✅ Public Domain (CC0, Unlicense)

### License Questions

If you have questions about:

- Whether your contribution is compatible
- License requirements for third-party code
- Copyright or attribution

Please ask in your pull request or open a discussion before submitting.

### Additional Resources

- [Full Project License](../../../LICENSE) - MIT License text
- [License Documentation](../license.md) - Comprehensive licensing information
- [SPDX License List](https://spdx.org/licenses/) - Standard license identifiers
- [Choose a License](https://choosealicense.com/) - Help choosing licenses for new projects
