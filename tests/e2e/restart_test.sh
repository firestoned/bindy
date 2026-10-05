#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
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

# How long a freshly started operator may take to repair drift made while it
# was down (the Bind9Instance watcher's InitApply enqueues every instance).
STARTUP_REPAIR_TIMEOUT=120
STARTUP_REPAIR_POLL=5

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

# The gate for deleting the operator's startup drift pass (roadmap 01 Phase F,
# ADR-0009 §5): drift that happens while no operator runs must be repaired from
# the watcher's own InitApply events when one starts, within a bounded time.
phase "Repairing drift made while the operator was down"
${KUBECTL} scale deployment/bindy -n "${NAMESPACE}" --replicas=0 >/dev/null
${KUBECTL} wait --for=delete pod -n "${NAMESPACE}" -l app=bindy \
    --timeout="${OPERATOR_ROLLOUT_TIMEOUT}s" >/dev/null 2>&1 || true
for instance in $(expected_primaries); do
    ${KUBECTL} delete service "${instance}" -n "${NAMESPACE}" --wait=true >/dev/null 2>&1 || true
done
${KUBECTL} scale deployment/bindy -n "${NAMESPACE}" --replicas=1 >/dev/null
${KUBECTL} rollout status deployment/bindy -n "${NAMESPACE}" \
    --timeout="${OPERATOR_ROLLOUT_TIMEOUT}s" >/dev/null 2>&1 || fail "operator did not come back"
started=$(date +%s)
for instance in $(expected_primaries); do
    until ${KUBECTL} get service "${instance}" -n "${NAMESPACE}" >/dev/null 2>&1; do
        if [ $(( $(date +%s) - started )) -ge "${STARTUP_REPAIR_TIMEOUT}" ]; then
            break
        fi
        sleep "${STARTUP_REPAIR_POLL}"
    done
    if ${KUBECTL} get service "${instance}" -n "${NAMESPACE}" >/dev/null 2>&1; then
        pass "Service ${instance} recreated $(( $(date +%s) - started ))s after operator start"
    else
        fail "Service ${instance} not recreated within ${STARTUP_REPAIR_TIMEOUT}s of operator start"
    fi
done
assert_dns_on_primaries "after drift repair on start"

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
summary_row "Drift made while the operator was down is repaired on start" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"
summary_row "Operator replays every zone/record into wiped operand Pods" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"
summary_row "Re-apply after restart is still idempotent" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"

finish "E2E — restart persistence"
