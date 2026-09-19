#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# Runs every DNS e2e suite in tests/e2e/ against ONE kind cluster, in order.
#
# This used to be a 947-line monolith. It is now a thin orchestrator: the suites
# it calls are independent programs, each with its own Makefile target and its
# own CI job (see .github/workflows/e2e.yaml). Keep this script for the local
# "just run everything against one cluster" workflow and for `make
# kind-integration-test`; use the individual targets when you want one answer
# fast, or when you want them running in parallel.
#
#   make e2e-rust          tests/e2e/rust_api_test.sh
#   make e2e-lifecycle     tests/e2e/lifecycle_test.sh
#   make e2e-idempotency   tests/e2e/idempotency_test.sh
#   make e2e-restart       tests/e2e/restart_test.sh
#
# Usage: integration_test.sh [--image REF] [--skip-deploy] [--skip-restart]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-test}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/lib" && pwd)/cluster.sh"

SKIP_RESTART=false
ARGS=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-restart) SKIP_RESTART=true; shift ;;
        *)              ARGS+=("$1"); shift ;;
    esac
done
parse_common_args ${ARGS[@]+"${ARGS[@]}"}

# Suites to run, in dependency order: the Rust API tests and the lifecycle suite
# establish that the basics work before idempotency and restart-persistence
# claim anything about them.
SUITES=(rust_api lifecycle idempotency)
if [ "${SKIP_RESTART}" = true ]; then
    echo -e "${YELLOW}⏭️  --skip-restart: omitting the restart-persistence suite${NC}"
else
    SUITES+=(restart)
fi

info "🧪 Running every e2e suite against kind cluster '${CLUSTER_NAME}'"

# Bring the cluster up exactly once; the suites then run with --skip-deploy so
# they share it instead of each building their own.
bindy_setup deploy/kind-config.yaml

FAILED=()
for suite in "${SUITES[@]}"; do
    echo ""
    info "════════════════════════════════════════════════════════════"
    info " ${suite}"
    info "════════════════════════════════════════════════════════════"
    if CLUSTER_NAME="${CLUSTER_NAME}" "${TESTS_DIR}/e2e/${suite}_test.sh" --skip-deploy; then
        continue
    fi
    FAILED+=("${suite}")
done

echo ""
if [ ${#FAILED[@]} -eq 0 ]; then
    echo -e "${GREEN}✅ All e2e suites passed (${SUITES[*]})${NC}"
    exit 0
fi

echo -e "${RED}❌ Failed suites: ${FAILED[*]}${NC}"
exit 1
