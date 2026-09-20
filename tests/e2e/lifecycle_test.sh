#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# E2E suite: zone and record lifecycle.
#
# The narrowest useful question: given a Bind9Cluster, a standalone
# Bind9Instance, a forward and a reverse DNSZone and one CR of every supported
# record type, does the operator bring all of it up and does BIND9 actually
# answer for it on every primary?
#
# This is the gate the other DNS suites build on -- if it fails, idempotency and
# restart-persistence results are meaningless -- so it is deliberately the
# cheapest and fastest of the three.
#
# Usage: tests/e2e/lifecycle_test.sh [--image REF] [--skip-deploy]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-lifecycle}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"
source "${LIB_DIR}/dns_fixtures.sh"

parse_common_args "$@"

info "🧪 E2E: zone/record lifecycle (cluster '${CLUSTER_NAME}')"

phase "Setting up cluster and operator"
bindy_setup deploy/kind-config-e2e.yaml

phase "Applying the fixture"
preclean_fixtures
apply_test_manifests

phase "Verifying every CR was created"
assert_fixture_crs_exist

phase "Operand readiness (${PRIMARY_REPLICAS} cluster primaries + 1 standalone)"
assert_operands_ready

phase "Verifying BIND9 serves every zone and record"
assert_dns_on_primaries "lifecycle"
assert_resource_counts "lifecycle"

dump_fixture_status

phase "Cleanup"
teardown_fixtures

summary_row "Every fixture CR created (cluster, instance, 2 zones, 10 records)" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"
summary_row "All 3 primaries reach Ready" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"
summary_row "BIND9 answers all ${#EXPECTED_DNS[@]} expected queries on every primary" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"

finish "E2E — zone/record lifecycle"
