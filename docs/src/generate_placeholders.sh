#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# Create placeholder files for all documentation pages

create_placeholder() {
    local file="$1"
    local title="$2"
    
    if [ ! -f "$file" ]; then
        mkdir -p "$(dirname "$file")"
        cat > "$file" << PLACEHOLDER
# ${title}

This page is under construction.

<!-- TODO: Add comprehensive documentation for ${title} -->

For now, please refer to:
- [README](../../../README.md)
- [Quick Start](../installation/quickstart.md)
- [GitHub Repository](https://github.com/firestoned/bindy)
PLACEHOLDER
        echo "Created $file"
    fi
}

# Guide pages
create_placeholder "guide/infrastructure.md" "Creating DNS Infrastructure"
create_placeholder "guide/primary-instance.md" "Primary DNS Instances"
create_placeholder "guide/secondary-instance.md" "Secondary DNS Instances"
create_placeholder "guide/multi-region.md" "Multi-Region Setup"
create_placeholder "guide/zones.md" "Managing DNS Zones"
create_placeholder "guide/creating-zones.md" "Creating Zones"
create_placeholder "guide/label-selectors.md" "Label Selectors"
create_placeholder "guide/zone-config.md" "Zone Configuration"
create_placeholder "guide/records-guide.md" "Managing DNS Records"
create_placeholder "guide/a-records.md" "A Records (IPv4)"
create_placeholder "guide/aaaa-records.md" "AAAA Records (IPv6)"
create_placeholder "guide/cname-records.md" "CNAME Records"
create_placeholder "guide/mx-records.md" "MX Records"
create_placeholder "guide/txt-records.md" "TXT Records"
create_placeholder "guide/ns-records.md" "NS Records"
create_placeholder "guide/srv-records.md" "SRV Records"
create_placeholder "guide/caa-records.md" "CAA Records"

# Operations pages
create_placeholder "operations/configuration.md" "Configuration"
create_placeholder "operations/env-vars.md" "Environment Variables"
create_placeholder "operations/rbac.md" "RBAC"
create_placeholder "operations/resources.md" "Resource Limits"
create_placeholder "operations/monitoring.md" "Monitoring"
create_placeholder "operations/status.md" "Status Conditions"
create_placeholder "operations/logging.md" "Logging"
create_placeholder "operations/metrics.md" "Metrics"
create_placeholder "operations/troubleshooting.md" "Troubleshooting"
create_placeholder "operations/common-issues.md" "Common Issues"
create_placeholder "operations/debugging.md" "Debugging"
create_placeholder "operations/faq.md" "FAQ"

# Advanced pages
create_placeholder "advanced/ha.md" "High Availability"
create_placeholder "advanced/zone-transfers.md" "Zone Transfers"
create_placeholder "advanced/replication.md" "Replication"
create_placeholder "advanced/security.md" "Security"
create_placeholder "advanced/dnssec.md" "DNSSEC"
create_placeholder "advanced/access-control.md" "Access Control"
create_placeholder "advanced/performance.md" "Performance"
create_placeholder "advanced/tuning.md" "Tuning"
create_placeholder "advanced/benchmarking.md" "Benchmarking"
create_placeholder "advanced/integration.md" "Integration"
create_placeholder "advanced/external-dns.md" "External DNS"
create_placeholder "advanced/service-discovery.md" "Service Discovery"

# Development pages
create_placeholder "development/setup.md" "Development Setup"
create_placeholder "development/building.md" "Building from Source"
create_placeholder "development/testing.md" "Running Tests"
create_placeholder "development/workflow.md" "Development Workflow"
create_placeholder "development/architecture-deep-dive.md" "Architecture Deep Dive"
create_placeholder "development/operator-design.md" "Operator Design"
create_placeholder "development/reconciliation.md" "Reconciliation Logic"
create_placeholder "development/bind9-integration.md" "BIND9 Integration"
create_placeholder "development/contributing.md" "Contributing"
create_placeholder "development/code-style.md" "Code Style"
create_placeholder "development/testing-guidelines.md" "Testing Guidelines"
create_placeholder "development/pr-process.md" "Pull Request Process"

# Reference pages
create_placeholder "reference/api.md" "API Reference"
create_placeholder "reference/bind9instance-spec.md" "Bind9Instance Spec"
create_placeholder "reference/dnszone-spec.md" "DNSZone Spec"
create_placeholder "reference/record-specs.md" "Record Specs"
create_placeholder "reference/examples.md" "Examples"
create_placeholder "reference/examples-simple.md" "Simple Setup"
create_placeholder "reference/examples-production.md" "Production Setup"
create_placeholder "reference/examples-multi-region.md" "Multi-Region Setup"
create_placeholder "reference/migration.md" "Migration Guide"
create_placeholder "reference/migration-python.md" "From Python Operator"
create_placeholder "reference/upgrades.md" "Version Upgrades"

# Root level pages
create_placeholder "changelog.md" "Changelog"
create_placeholder "license.md" "License"

echo "All placeholder files created!"
