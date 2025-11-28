#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

echo "Initializing Git repository..."

git init
git add .
git commit -m "Initial commit: BIND9 DNS Operator for Kubernetes

- Kopf-based operator implementation
- Support for all DNS record types (A, AAAA, CNAME, MX, TXT, NS, SRV, PTR, CAA, NAPTR)
- StatefulSet-based BIND9 deployment
- Comprehensive CRDs for DNS management
- Production-ready with error handling and monitoring
- Poetry-based project structure
- Docker containerization
- Kubernetes deployment manifests
- Test suite and examples"

echo ""
echo "✅ Git repository initialized!"
echo ""
echo "Next steps:"
echo "1. Create a GitHub repository at: https://github.com/new"
echo "2. Run these commands:"
echo "   git remote add origin git@github.com:$GITHUB_USER/$PROJECT_NAME.git"
echo "   git branch -M main"
echo "   git push -u origin main"
