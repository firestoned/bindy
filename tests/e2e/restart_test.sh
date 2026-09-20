#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# E2E suite: restart persistence.
#
# Two different failure modes, both invisible to a steady-state test:
#
#   1. The operator holds no state of its own, so after a restart it must
#      rebuild its watches and reach the same conclusion about zones it did not
#      itself create in this process lifetime.
#   2. BIND9 keeps its zone files inside the Pod, so a deleted operand Pod comes
#      back EMPTY. Everything verified after the wipe was re-pushed by the
#      operator, not restored from disk -- this is the zone/record replay path
#      (#486), and a dig against a recovered server is the only thing that
#      distinguishes it from one that silently serves nothing.
#
# This is the slowest of the DNS suites (a full operand wipe costs ~100s of
# replay), which is exactly why it is its own target and its own CI job.
#
# Usage: tests/e2e/restart_test.sh [--image REF] [--skip-deploy]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-restart}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"
source "${LIB_DIR}/dns_fixtures.sh"

parse_common_args "$@"

info "🧪 E2E: restart persistence (cluster '${CLUSTER_NAME}')"

phase "Setting up cluster and operator"
bindy_setup deploy/kind-config-e2e.yaml

phase "Establishing the baseline"
preclean_fixtures
apply_test_manifests
assert_fixture_healthy "baseline"

if [ "${ERRORS}" -ne 0 ]; then
    # "Survives a restart" means nothing if it was not alive to begin with.
    fail "baseline is not healthy; skipping the restart phases"
    finish "E2E — restart persistence" || true
    exit 1
fi

phase "Restarting the operator"
${KUBECTL} rollout restart deployment/bindy -n "${NAMESPACE}" >/dev/null
if ${KUBECTL} rollout status deployment/bindy -n "${NAMESPACE}" \
       --timeout="${OPERATOR_ROLLOUT_TIMEOUT}s" >/dev/null 2>&1; then
    pass "operator rollout completed"
else
    fail "operator rollout did not complete"
fi
assert_dns_on_primaries "after operator restart"
assert_resource_counts "after operator restart"

phase "Wiping every BIND9 operand Pod"
${KUBECTL} delete pod -n "${NAMESPACE}" -l app=bind9 --wait=true >/dev/null
assert_fixture_healthy "after operand wipe"

phase "Re-applying every manifest after the restarts"
apply_test_manifests >/dev/null
assert_fixture_healthy "after restart + re-apply"

dump_fixture_status

phase "Cleanup"
teardown_fixtures

summary_row "Zones and records survive an operator restart" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"
summary_row "Operator replays every zone/record into wiped operand Pods" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"
summary_row "Re-apply after restart is still idempotent" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"

finish "E2E — restart persistence"
