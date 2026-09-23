#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# E2E suite: the Rust integration tests (tests/simple_integration.rs and
# tests/scout_integration.rs).
#
# These drive the Kubernetes API through the same kube-rs client the operator
# uses, in their own namespaces, so they exercise the client/CRD contract rather
# than the DNS data path. They are a separate suite because they are the only
# part of the old integration_test.sh that needs a Rust toolchain -- keeping
# them here means the DNS suites can run on a runner with nothing but kind,
# kubectl and docker.
#
# The tests use `Client::try_default()`, so the ambient kubeconfig context has
# to point at this suite's cluster.
#
# Usage: tests/e2e/rust_api_test.sh [--image REF] [--skip-deploy]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-rust}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"

parse_common_args "$@"

info "🧪 E2E: Rust API integration tests (cluster '${CLUSTER_NAME}')"

phase "Setting up cluster and operator"
bindy_setup deploy/kind-config-e2e.yaml

phase "Running the Rust integration tests -- --ignored"
# Client::try_default() reads the ambient context, so point it at this cluster.
kubectl config use-context "kind-${CLUSTER_NAME}" >/dev/null
export KUBECONFIG="${KUBECONFIG:-${HOME}/.kube/config}"

cd "${PROJECT_ROOT}"

# Each suite is its own test binary. `set -e` would abort before the summary,
# so capture each status instead and report them together.
#
#   simple_integration  the CRD/client contract across every record kind
#   scout_integration   Scout's stale-cleanup label selectors, evaluated by a
#                       REAL API server — the unit tests assert selector text
#                       and wiremock just echoes it back, so this is the only
#                       layer that can show the #474 zone scoping holds
for suite in simple_integration scout_integration; do
    step "cargo test --test ${suite} -- --ignored"
    if cargo test --test "${suite}" -- --ignored --test-threads=1 --nocapture; then
        pass "${suite} passed"
    else
        fail "${suite} failed"
    fi
done

summary_row "Rust integration tests (\`simple_integration\`, \`scout_integration\`)" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"

finish "E2E — Rust API integration tests"
