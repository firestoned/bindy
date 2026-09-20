#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# E2E suite: idempotent re-apply.
#
# Applying the identical spec a second time must be a no-op. The failure this
# catches is not "kubectl errored" -- it never does -- but the operator reacting
# to a no-change update by creating a second Bind9Instance, re-adding a zone
# that already exists, or storming the reconcile loop hard enough to drop a
# record that was already being served.
#
# So the assertion is a resource census plus a full DNS re-check after the
# second apply: same counts, same answers.
#
# Usage: tests/e2e/idempotency_test.sh [--image REF] [--skip-deploy]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-idempotency}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"
source "${LIB_DIR}/dns_fixtures.sh"

parse_common_args "$@"

# How many times to re-apply. Two extra applies is enough to distinguish a
# genuinely convergent controller from one that happens to tolerate one repeat.
REAPPLY_ROUNDS=2

info "🧪 E2E: idempotent re-apply (cluster '${CLUSTER_NAME}')"

phase "Setting up cluster and operator"
bindy_setup deploy/kind-config-e2e.yaml

phase "Establishing the baseline"
preclean_fixtures
apply_test_manifests
assert_fixture_healthy "baseline"

if [ "${ERRORS}" -ne 0 ]; then
    # Re-applying on top of a broken baseline cannot prove anything about
    # idempotency, and the failure belongs to the lifecycle suite.
    fail "baseline is not healthy; skipping the re-apply rounds"
    finish "E2E — idempotent re-apply" || true
    exit 1
fi

for ((round = 1; round <= REAPPLY_ROUNDS; round++)); do
    phase "Re-apply round ${round}/${REAPPLY_ROUNDS} — identical manifests"
    apply_test_manifests >/dev/null
    assert_fixture_healthy "after re-apply ${round}"
done

dump_fixture_status

phase "Cleanup"
teardown_fixtures

summary_row "Re-applying the identical spec ${REAPPLY_ROUNDS}x creates no duplicates" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"
summary_row "No zone or record is lost across re-applies" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"

finish "E2E — idempotent re-apply"
