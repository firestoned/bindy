#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# Kind cluster + operator bring-up shared by the tests/e2e/ suites. Sourced.
#
# Each suite owns its own cluster, so this has to be safe to call against a
# cluster that already exists (a local re-run) and against one that does not
# (CI). Everything here is therefore idempotent.

[ -n "${_BINDY_CLUSTER_SH:-}" ] && return 0
_BINDY_CLUSTER_SH=1

# shellcheck source=tests/lib/common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

# Tag used when no --image is supplied and the image is built locally.
# Deliberately not "latest": containerd keeps an existing tag when an image of
# that name is already present from a registry, so `kind load` of a "latest"
# build can silently leave the registry image in place. Matches
# scripts/build-docker-fast.sh.
LOCAL_BUILD_TAG="${LOCAL_BUILD_TAG:-local-integration}"

OPERATOR_AVAILABLE_TIMEOUT=300

# Create the kind cluster if it is missing, then install the CRDs and RBAC.
#
# $1 — path to the kind config, relative to the repo root.
bindy_ensure_cluster() {
    local kind_config="${PROJECT_ROOT}/$1"

    if ! kind get clusters 2>/dev/null | grep -qx "${CLUSTER_NAME}"; then
        step "Creating kind cluster '${CLUSTER_NAME}' (${1})"
        kind create cluster --name "${CLUSTER_NAME}" --config "${kind_config}"
    else
        pass "Reusing existing kind cluster '${CLUSTER_NAME}'"
    fi

    step "Installing CRDs and RBAC"
    ${KUBECTL} create namespace "${NAMESPACE}" --dry-run=client -o yaml \
        | ${KUBECTL} apply -f - >/dev/null
    # `replace --force` rather than `apply`: the Bind9Instance CRD exceeds the
    # 256KB last-applied-configuration annotation limit.
    ${KUBECTL} replace --force -f "${PROJECT_ROOT}/deploy/operator/crds/" >/dev/null 2>&1 \
        || ${KUBECTL} create -f "${PROJECT_ROOT}/deploy/operator/crds/" >/dev/null
    ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/operator/rbac/" >/dev/null
}

# Resolve IMAGE_REF, building the operator image locally when the caller did not
# supply one, and make it available to the kind nodes.
bindy_prepare_image() {
    if [ -z "${IMAGE_REF}" ]; then
        IMAGE_REF="ghcr.io/firestoned/bindy:${LOCAL_BUILD_TAG}"
        step "Building operator image ${IMAGE_REF}"
        TAG="${LOCAL_BUILD_TAG}" KIND_CLUSTER="${CLUSTER_NAME}" \
            "${PROJECT_ROOT}/scripts/build-docker-fast.sh" local "${LOCAL_BUILD_TAG}"
    fi

    # A locally built or `docker load`-ed image has to be pushed into the kind
    # node; anything else is assumed pullable from a registry.
    if docker image inspect "${IMAGE_REF}" >/dev/null 2>&1; then
        step "Loading ${IMAGE_REF} into kind cluster '${CLUSTER_NAME}'"
        kind load docker-image "${IMAGE_REF}" --name "${CLUSTER_NAME}" >/dev/null
    else
        warn "${IMAGE_REF} is not in the local Docker daemon; assuming the node can pull it"
    fi
}

# Deploy (or re-deploy) the operator at IMAGE_REF and wait for it to be
# available. Always applies the manifest so a suite re-run picks up a new image.
bindy_deploy_operator() {
    step "Deploying operator with image ${IMAGE_REF}"
    # Match both tag and digest pins so a digest-pinned deployment.yaml is
    # rewritten too.
    sed -E "s|ghcr.io/firestoned/bindy[:@][^\"[:space:]]*|${IMAGE_REF}|g" \
        "${PROJECT_ROOT}/deploy/operator/deployment.yaml" | ${KUBECTL} apply -f - >/dev/null

    if ${KUBECTL} rollout status deployment/bindy -n "${NAMESPACE}" \
           --timeout="${OPERATOR_AVAILABLE_TIMEOUT}s" >/dev/null 2>&1; then
        pass "Operator is available"
        return 0
    fi

    fail "Operator never became available"
    ${KUBECTL} get pods -n "${NAMESPACE}" -l app=bindy 2>/dev/null || true
    ${KUBECTL} logs -n "${NAMESPACE}" -l app=bindy --tail=50 2>/dev/null || true
    exit 1
}

# One-call bring-up: cluster, image, operator. Honours --skip-deploy, which
# asserts the cluster is already up and leaves it exactly as it is.
#
# $1 — path to the kind config, relative to the repo root.
bindy_setup() {
    if [ "${SKIP_DEPLOY}" = true ]; then
        step "--skip-deploy: reusing cluster '${CLUSTER_NAME}' as-is"
        ${KUBECTL} cluster-info >/dev/null 2>&1 || {
            echo -e "${RED}Cluster '${CLUSTER_NAME}' is not reachable${NC}" >&2
            exit 1
        }
        return 0
    fi

    bindy_ensure_cluster "$1"
    bindy_prepare_image
    bindy_deploy_operator
}
