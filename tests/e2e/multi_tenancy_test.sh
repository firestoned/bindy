#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# E2E suite: multi-tenancy.
#
# Wraps tests/run_multi_tenancy_tests.sh, which asserts that Bind9Clusters,
# Bind9Instances and DNSZones in different namespaces stay isolated from one
# another. That script drives the AMBIENT kubectl context (it predates the
# per-suite cluster convention), so this wrapper owns the cluster bring-up and
# points the context at it before handing over.
#
# Usage: tests/e2e/multi_tenancy_test.sh [--image REF] [--skip-deploy]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-multitenancy}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"

parse_common_args "$@"

info "🧪 E2E: multi-tenancy (cluster '${CLUSTER_NAME}')"

phase "Setting up cluster and operator"
bindy_setup deploy/kind-config-e2e.yaml

phase "Running the multi-tenancy suite"
kubectl config use-context "kind-${CLUSTER_NAME}" >/dev/null

chmod +x "${TESTS_DIR}/run_multi_tenancy_tests.sh"
if "${TESTS_DIR}/run_multi_tenancy_tests.sh"; then
    pass "multi-tenancy suite passed"
else
    fail "multi-tenancy suite failed"
fi

summary_row "Namespace isolation across Bind9Cluster / Bind9Instance / DNSZone" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"

finish "E2E — multi-tenancy"
