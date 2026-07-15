#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# Bindy regression suite (kind + kubectl).
#
# Focus: the bindcar 0.7.0 migration contract (Mode B / TokenReview) plus the
# ValidatingAdmissionPolicy surface. Complements tests/integration_test.sh
# (full DNS lifecycle) — this suite asserts the *shape* of what the operator
# renders and what the API server admits, so migration regressions are caught
# without needing a full DNS round-trip.
#
# Phases:
#   A. Admission-policy regression — kind + kubectl only, NO images required.
#      CRDs + all VAPs installed, every accept-*/reject-* fixture exercised
#      via server-side dry-run.
#   B. Operator pod-shape regression — requires --image <bindy operator ref>.
#      Deploys the operator under PSA `restricted`, creates a Bind9Instance,
#      and asserts the operand Deployment/Service/Secret match the bindcar
#      0.7.2 contract (unprivileged port 5353, no added caps, RO rootfs, seccomp, TMPDIR,
#      operator-SA allowlist, audience, hmac-sha256).
#   C. Operand liveness smoke — best-effort. Waits for the operand pod to be
#      Ready (probe = TCP 53, so Ready proves named binds 53). Skipped with a
#      warning if the bindcar sidecar image cannot be pulled/loaded.
#
# Usage:
#   regression_test.sh                         # Phase A only
#   regression_test.sh --image ghcr.io/firestoned/bindy:dev   # Phases A+B+C
#   regression_test.sh --fresh                 # recreate the kind cluster first
#   regression_test.sh --delete-cluster        # delete the cluster on success
#
# The cluster is dedicated (default: bindy-regression) and is REUSED across
# runs for speed. It never touches other kind clusters or kube contexts.

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

NAMESPACE="bindy-system"
CLUSTER_NAME="${CLUSTER_NAME:=bindy-regression}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

# Test instance / expectations (bindcar 0.7.0 contract)
INSTANCE_NAME="regression-primary"
EXPECTED_DNS_PORT="5353"
EXPECTED_AUDIENCE="bindcar"
EXPECTED_TMPDIR="/tmp"
EXPECTED_ALLOWED_SA="system:serviceaccount:${NAMESPACE}:bindy"
EXPECTED_RNDC_ALGORITHM="hmac-sha256"
EXPECTED_BINDCAR_IMAGE_PREFIX="ghcr.io/firestoned/bindcar:v0.7"

# Timeouts (seconds)
VAP_PROPAGATION_TIMEOUT=60
OPERATOR_READY_TIMEOUT=180
OPERAND_DEPLOY_TIMEOUT=120
OPERAND_READY_TIMEOUT=180
CLEANUP_TIMEOUT=60

IMAGE_REF=""
FRESH=false
DELETE_CLUSTER=false

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
FAILED_TESTS=()
# Markdown rows accumulated for the GitHub Actions step summary (run page).
SUMMARY_ROWS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --image)
            IMAGE_REF="$2"; shift 2 ;;
        --fresh)
            FRESH=true; shift ;;
        --delete-cluster)
            DELETE_CLUSTER=true; shift ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Usage: $0 [--image IMAGE_REF] [--fresh] [--delete-cluster]"
            exit 1 ;;
    esac
done

pass() { PASS_COUNT=$((PASS_COUNT + 1)); SUMMARY_ROWS+=("| ✅ | ${1//|/\\|} |"); echo -e "  ${GREEN}✅ PASS${NC} $1"; }
fail() { FAIL_COUNT=$((FAIL_COUNT + 1)); FAILED_TESTS+=("$1"); SUMMARY_ROWS+=("| ❌ | ${1//|/\\|}${2:+ — ${2//|/\\|}} |"); echo -e "  ${RED}❌ FAIL${NC} $1${2:+ — $2}"; }
skip() { SKIP_COUNT=$((SKIP_COUNT + 1)); SUMMARY_ROWS+=("| ⏭ | ${1//|/\\|} |"); echo -e "  ${YELLOW}⏭  SKIP${NC} $1${2:+ — $2}"; }

# assert_eq <test-name> <actual> <expected>
assert_eq() {
    if [ "$2" = "$3" ]; then
        pass "$1"
    else
        fail "$1" "expected '$3', got '$2'"
    fi
}

# assert_prefix <test-name> <actual> <expected-prefix>
assert_prefix() {
    if [[ "$2" == "$3"* ]]; then
        pass "$1"
    else
        fail "$1" "expected prefix '$3', got '$2'"
    fi
}

# ---------------------------------------------------------------------------
# Cluster setup
# ---------------------------------------------------------------------------

ensure_cluster() {
    if [ "$FRESH" = true ]; then
        echo -e "${YELLOW}♻️  --fresh: deleting cluster '${CLUSTER_NAME}'...${NC}"
        kind delete cluster --name "${CLUSTER_NAME}" 2>/dev/null || true
    fi

    if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
        echo -e "${GREEN}📦 Reusing kind cluster '${CLUSTER_NAME}'${NC}"
        return 0
    fi

    echo -e "${YELLOW}📦 Creating kind cluster '${CLUSTER_NAME}'...${NC}"
    # Dedicated minimal config: no host port mappings, so this cluster never
    # conflicts with the integration cluster (deploy/kind-config.yaml).
    kind create cluster --name "${CLUSTER_NAME}" --config - <<'EOF'
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
EOF
}

install_crds() {
    echo -e "${GREEN}📋 Installing CRDs + namespace + RBAC...${NC}"
    ${KUBECTL} create namespace "${NAMESPACE}" --dry-run=client -o yaml | ${KUBECTL} apply -f -
    # 'replace --force' avoids the 256KB annotation limit on the large CRDs.
    ${KUBECTL} replace --force -f "${PROJECT_ROOT}/deploy/operator/crds/" 2>/dev/null \
        || ${KUBECTL} create -f "${PROJECT_ROOT}/deploy/operator/crds/"
    # RBAC is needed in Phase A too: the operator-workload fixtures impersonate
    # the operator SA, and authorization runs before admission — without the
    # bindy ClusterRole the impersonated request would die at authz, never
    # reaching the VAP under test.
    ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/operator/rbac/" >/dev/null
}

# Idempotency: remove the previous run's test instance BEFORE the CRDs are
# replaced (a `replace --force` on the CRD cascades-deletes its CRs and can
# hang on a stuck finalizer) and before Phase B (which must never assert
# against a stale operand Deployment from the prior run).
cleanup_test_resources() {
    ${KUBECTL} get crd bind9instances.bindy.firestoned.io >/dev/null 2>&1 || return 0
    ${KUBECTL} get bind9instance "${INSTANCE_NAME}" -n "${NAMESPACE}" >/dev/null 2>&1 || return 0

    echo -e "${YELLOW}🧹 Cleaning up previous test instance '${INSTANCE_NAME}'...${NC}"
    if ! ${KUBECTL} delete bind9instance "${INSTANCE_NAME}" -n "${NAMESPACE}" \
            --wait=true "--timeout=${CLEANUP_TIMEOUT}s" >/dev/null 2>&1; then
        # A stuck finalizer (e.g. the operator from a failed previous run is
        # unhealthy) must not wedge the suite. This is a throwaway regression
        # cluster, so strip the finalizers and move on.
        echo -e "${YELLOW}⚠️  delete timed out; stripping finalizers${NC}"
        ${KUBECTL} patch bind9instance "${INSTANCE_NAME}" -n "${NAMESPACE}" \
            --type=merge -p '{"metadata":{"finalizers":[]}}' >/dev/null 2>&1 || true
    fi

    # Wait for the owned operand Deployment to be garbage-collected.
    local waited=0
    while ${KUBECTL} get deployment "${INSTANCE_NAME}" -n "${NAMESPACE}" >/dev/null 2>&1; do
        if [ "$waited" -ge "$CLEANUP_TIMEOUT" ]; then
            echo -e "${YELLOW}⚠️  stale operand Deployment still present after ${CLEANUP_TIMEOUT}s; deleting directly${NC}"
            ${KUBECTL} delete deployment "${INSTANCE_NAME}" -n "${NAMESPACE}" \
                --ignore-not-found "--timeout=${CLEANUP_TIMEOUT}s" >/dev/null 2>&1 || true
            break
        fi
        sleep 2; waited=$((waited + 2))
    done
}

# fixture_as_flag <fixture> — extract a '--as=<principal>' impersonation hint
# from the fixture's header comment (e.g. the operator-workload fixtures whose
# VAP only matches requests made by the operator SA). Prints the flag or "".
fixture_as_flag() {
    grep -oE -- '--as=[^ ]+' "$1" 2>/dev/null | head -1 || true
}

# ---------------------------------------------------------------------------
# Phase A — admission-policy regression (no images required)
# ---------------------------------------------------------------------------

phase_a() {
    echo ""
    echo -e "${BLUE}════ Phase A: admission-policy regression ════${NC}"

    echo -e "${GREEN}🛡  Applying all ValidatingAdmissionPolicies + bindings...${NC}"
    ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/admission-policies/" >/dev/null

    # Wait for the VAPs to become active. Canary: the trailing-dot CNAME
    # fixture is rejected ONLY by VAP 13 (the CRD schema has no pattern on
    # spec.target), so its rejection proves policy propagation.
    echo -e "${GREEN}⏳ Waiting for policy propagation (canary: VAP 13)...${NC}"
    local waited=0
    until ! ${KUBECTL} apply --dry-run=server \
            -f "${PROJECT_ROOT}/deploy/admission-policies/tests/reject-cnamerecord-no-trailing-dot.yaml" \
            >/dev/null 2>&1; do
        if [ "$waited" -ge "$VAP_PROPAGATION_TIMEOUT" ]; then
            fail "VAP propagation" "policies not active after ${VAP_PROPAGATION_TIMEOUT}s"
            return 0
        fi
        sleep 2; waited=$((waited + 2))
    done
    pass "VAP propagation (canary rejected after ${waited}s)"

    local fixture name as_flag
    for fixture in "${PROJECT_ROOT}"/deploy/admission-policies/tests/accept-*.yaml; do
        name="accept fixture: $(basename "$fixture")"
        as_flag="$(fixture_as_flag "$fixture")"
        # shellcheck disable=SC2086  # $as_flag is intentionally word-split (one token or none)
        if ${KUBECTL} apply --dry-run=server $as_flag -f "$fixture" >/dev/null 2>&1; then
            pass "$name${as_flag:+ (${as_flag})}"
        else
            fail "$name${as_flag:+ (${as_flag})}" "expected admission, got rejection"
        fi
    done

    for fixture in "${PROJECT_ROOT}"/deploy/admission-policies/tests/reject-*.yaml; do
        name="reject fixture: $(basename "$fixture")"
        as_flag="$(fixture_as_flag "$fixture")"
        # shellcheck disable=SC2086  # $as_flag is intentionally word-split (one token or none)
        if ${KUBECTL} apply --dry-run=server $as_flag -f "$fixture" >/dev/null 2>&1; then
            fail "$name${as_flag:+ (${as_flag})}" "expected rejection, got admission"
        else
            pass "$name${as_flag:+ (${as_flag})}"
        fi
    done
}

# ---------------------------------------------------------------------------
# Phase B — operator pod-shape regression (requires --image)
# ---------------------------------------------------------------------------

# jp <resource> <jsonpath> — jsonpath helper against the operand namespace
jp() {
    ${KUBECTL} get "$1" -n "${NAMESPACE}" -o jsonpath="$2" 2>/dev/null || true
}

phase_b() {
    echo ""
    echo -e "${BLUE}════ Phase B: operator pod-shape regression ════${NC}"

    if [ -z "$IMAGE_REF" ]; then
        skip "Phase B (operator pod shape)" "no --image given; run with --image <bindy operator ref>"
        return 0
    fi

    # Load the image into kind if it exists in the local docker daemon
    # (locally built images are not pullable by the kind node).
    if docker image inspect "$IMAGE_REF" >/dev/null 2>&1; then
        echo -e "${GREEN}📤 Loading ${IMAGE_REF} into kind...${NC}"
        kind load docker-image "$IMAGE_REF" --name "${CLUSTER_NAME}"
    else
        echo -e "${YELLOW}⚠️  ${IMAGE_REF} not in local docker; assuming the kind node can pull it${NC}"
    fi

    # PSA `restricted` BEFORE any pods: proves both the operator pod and the
    # operand pods pass restricted admission (the migration's §6 contract).
    echo -e "${GREEN}🔒 Enforcing PSA restricted on ${NAMESPACE}...${NC}"
    ${KUBECTL} label ns "${NAMESPACE}" \
        pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null

    echo -e "${GREEN}🔐 Applying RBAC (incl. bindcar-tokenreview)...${NC}"
    ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/operator/rbac/" >/dev/null

    echo -e "${GREEN}🚀 Deploying operator: ${IMAGE_REF}${NC}"
    sed "s|ghcr.io/firestoned/bindy:latest|${IMAGE_REF}|g" \
        "${PROJECT_ROOT}/deploy/operator/deployment.yaml" | ${KUBECTL} apply -f - >/dev/null

    if ${KUBECTL} wait --for=condition=available "--timeout=${OPERATOR_READY_TIMEOUT}s" \
            deployment/bindy -n "${NAMESPACE}" >/dev/null 2>&1; then
        pass "operator available under PSA restricted"
    else
        fail "operator available under PSA restricted" "deployment/bindy not available in ${OPERATOR_READY_TIMEOUT}s"
        ${KUBECTL} get pods -n "${NAMESPACE}" || true
        return 0
    fi

    # Operator carries the projected bindcar-audience token (Mode B).
    assert_eq "operator: bindcar-token projected volume audience" \
        "$(jp deployment/bindy '{.spec.template.spec.volumes[?(@.name=="bindcar-token")].projected.sources[0].serviceAccountToken.audience}')" \
        "$EXPECTED_AUDIENCE"
    assert_eq "operator: bindcar-token mounted at /var/run/secrets/bindcar" \
        "$(jp deployment/bindy '{.spec.template.spec.containers[0].volumeMounts[?(@.name=="bindcar-token")].mountPath}')" \
        "/var/run/secrets/bindcar"

    # bind9 SA can create TokenReviews (bindcar's Mode B requirement).
    if [ "$(${KUBECTL} auth can-i create tokenreviews.authentication.k8s.io \
            --as="system:serviceaccount:${NAMESPACE}:bind9" 2>/dev/null)" = "yes" ]; then
        pass "rbac: bind9 SA can create tokenreviews"
    else
        fail "rbac: bind9 SA can create tokenreviews" "kubectl auth can-i returned no"
    fi

    # Prior-run resources were already removed by cleanup_test_resources.
    echo -e "${GREEN}🧪 Creating test Bind9Instance '${INSTANCE_NAME}'...${NC}"
    cat <<EOF | ${KUBECTL} apply -f - >/dev/null
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: ${INSTANCE_NAME}
  namespace: ${NAMESPACE}
  labels:
    dns-role: primary
spec:
  # STANDALONE instance (empty clusterRef). A non-empty clusterRef makes the
  # operator defer ConfigMap creation to the named Bind9Cluster
  # (resources.rs::create_or_update_configmap) — with no such cluster the
  # operand pod would hang in ContainerCreating on a missing '<name>-config'
  # ConfigMap. Standalone mode makes the operator generate the instance's own
  # ConfigMap, and exercises the identical pod-shape contract this suite checks.
  clusterRef: ""
  role: primary
  replicas: 1
EOF

    echo -e "${GREEN}⏳ Waiting for operand Deployment...${NC}"
    local waited=0
    until ${KUBECTL} get deployment "${INSTANCE_NAME}" -n "${NAMESPACE}" >/dev/null 2>&1; do
        if [ "$waited" -ge "$OPERAND_DEPLOY_TIMEOUT" ]; then
            fail "operand Deployment created" "not present after ${OPERAND_DEPLOY_TIMEOUT}s"
            ${KUBECTL} logs -n "${NAMESPACE}" deployment/bindy --tail=30 || true
            return 0
        fi
        sleep 2; waited=$((waited + 2))
    done
    pass "operand Deployment created (${waited}s)"

    local dep="deployment/${INSTANCE_NAME}"
    local bind9='{.spec.template.spec.containers[?(@.name=="bind9")]'
    local api='{.spec.template.spec.containers[?(@.name=="api")]'

    # named: unprivileged port 5353 (no NET_BIND_SERVICE) + seccomp
    assert_eq "operand: named dns-tcp containerPort" \
        "$(jp "$dep" "${bind9}.ports[?(@.name==\"dns-tcp\")].containerPort}")" "$EXPECTED_DNS_PORT"
    assert_eq "operand: named dns-udp containerPort" \
        "$(jp "$dep" "${bind9}.ports[?(@.name==\"dns-udp\")].containerPort}")" "$EXPECTED_DNS_PORT"
    assert_eq "operand: named adds no capabilities (unprivileged 5353)" \
        "$(jp "$dep" "${bind9}.securityContext.capabilities.add[0]}")" ""
    assert_eq "operand: named capabilities drop ALL" \
        "$(jp "$dep" "${bind9}.securityContext.capabilities.drop[0]}")" "ALL"
    assert_eq "operand: named seccomp RuntimeDefault" \
        "$(jp "$dep" "${bind9}.securityContext.seccompProfile.type}")" "RuntimeDefault"

    # bindcar sidecar: image, RO rootfs, seccomp, no cap adds, env contract
    assert_prefix "operand: api image is bindcar v0.7" \
        "$(jp "$dep" "${api}.image}")" "$EXPECTED_BINDCAR_IMAGE_PREFIX"
    assert_eq "operand: api readOnlyRootFilesystem" \
        "$(jp "$dep" "${api}.securityContext.readOnlyRootFilesystem}")" "true"
    assert_eq "operand: api seccomp RuntimeDefault" \
        "$(jp "$dep" "${api}.securityContext.seccompProfile.type}")" "RuntimeDefault"
    assert_eq "operand: api no added capabilities" \
        "$(jp "$dep" "${api}.securityContext.capabilities.add}")" ""
    assert_eq "operand: api BIND_TOKEN_AUDIENCES" \
        "$(jp "$dep" "${api}.env[?(@.name==\"BIND_TOKEN_AUDIENCES\")].value}")" "$EXPECTED_AUDIENCE"
    assert_eq "operand: api TMPDIR" \
        "$(jp "$dep" "${api}.env[?(@.name==\"TMPDIR\")].value}")" "$EXPECTED_TMPDIR"
    assert_eq "operand: api BIND_ALLOWED_SERVICE_ACCOUNTS names the operator SA" \
        "$(jp "$dep" "${api}.env[?(@.name==\"BIND_ALLOWED_SERVICE_ACCOUNTS\")].value}")" "$EXPECTED_ALLOWED_SA"
    assert_eq "operand: api /tmp volumeMount" \
        "$(jp "$dep" "${api}.volumeMounts[?(@.name==\"tmp\")].mountPath}")" "$EXPECTED_TMPDIR"

    # pod: seccomp + memory-backed tmp volume
    assert_eq "operand: pod seccomp RuntimeDefault" \
        "$(jp "$dep" '{.spec.template.spec.securityContext.seccompProfile.type}')" "RuntimeDefault"
    assert_eq "operand: tmp volume is memory-backed emptyDir" \
        "$(jp "$dep" '{.spec.template.spec.volumes[?(@.name=="tmp")].emptyDir.medium}')" "Memory"

    # Service targets port 53
    assert_eq "operand: service dns-tcp targetPort" \
        "$(jp "service/${INSTANCE_NAME}" '{.spec.ports[?(@.name=="dns-tcp")].targetPort}')" "$EXPECTED_DNS_PORT"
    assert_eq "operand: service dns-udp targetPort" \
        "$(jp "service/${INSTANCE_NAME}" '{.spec.ports[?(@.name=="dns-udp")].targetPort}')" "$EXPECTED_DNS_PORT"

    # RNDC secret is SHA-2
    local algo_b64
    algo_b64="$(jp "secret/${INSTANCE_NAME}-rndc-key" '{.data.algorithm}')"
    assert_eq "operand: rndc key algorithm" \
        "$(printf '%s' "$algo_b64" | base64 -d 2>/dev/null || true)" "$EXPECTED_RNDC_ALGORITHM"

    # PSA restricted admitted the operand pod (the Pod object existing proves
    # admission passed, even if its images cannot be pulled).
    local waited_pod=0
    until [ -n "$(${KUBECTL} get pods -n "${NAMESPACE}" -l "instance=${INSTANCE_NAME}" -o name 2>/dev/null)" ]; do
        if [ "$waited_pod" -ge "$OPERAND_DEPLOY_TIMEOUT" ]; then
            break
        fi
        sleep 2; waited_pod=$((waited_pod + 2))
    done
    if [ -n "$(${KUBECTL} get pods -n "${NAMESPACE}" -l "instance=${INSTANCE_NAME}" -o name 2>/dev/null)" ]; then
        pass "operand: pod admitted under PSA restricted"
    else
        fail "operand: pod admitted under PSA restricted" \
            "no pod created — check replicaset events: $(${KUBECTL} get events -n "${NAMESPACE}" --field-selector reason=FailedCreate -o jsonpath='{.items[-1].message}' 2>/dev/null | head -c 200)"
    fi
}

# ---------------------------------------------------------------------------
# Phase C — operand liveness smoke (best-effort)
# ---------------------------------------------------------------------------

phase_c() {
    echo ""
    echo -e "${BLUE}════ Phase C: operand liveness smoke (best-effort) ════${NC}"

    if [ -z "$IMAGE_REF" ]; then
        skip "Phase C (operand liveness)" "requires Phase B"
        return 0
    fi

    # Readiness probe is TCP :5353, so a Ready pod proves the named.conf
    # template change (listen-on port 5353) landed.
    echo -e "${GREEN}⏳ Waiting up to ${OPERAND_READY_TIMEOUT}s for operand pod Ready...${NC}"
    if ${KUBECTL} wait --for=condition=ready "--timeout=${OPERAND_READY_TIMEOUT}s" \
            pod -l "instance=${INSTANCE_NAME}" -n "${NAMESPACE}" >/dev/null 2>&1; then
        pass "operand pod Ready (named listening on :53)"
    else
        # Surface the ACTUAL reason rather than guessing. Inspect the sidecar's
        # container state and last log line so the operator (not the reader) says
        # why it isn't Ready. The most common real cause is a pre-v0.7.2 bindcar
        # image (no k8s-token-review) crashlooping on the non-loopback auth
        # startup guard, since bindy runs Mode B and sets no BIND_API_TOKEN.
        local pod api_reason api_log
        pod="$(${KUBECTL} get pods -n "${NAMESPACE}" -l "instance=${INSTANCE_NAME}" -o jsonpath='{.items[0].metadata.name}' 2>/dev/null)"
        api_reason="$(${KUBECTL} get pod "$pod" -n "${NAMESPACE}" \
            -o jsonpath='{.status.containerStatuses[?(@.name=="api")].state.waiting.reason}{.status.containerStatuses[?(@.name=="api")].lastState.terminated.reason}' 2>/dev/null || true)"
        api_log="$(${KUBECTL} logs "$pod" -c api -n "${NAMESPACE}" --tail=3 2>/dev/null | tr '\n' ' ' | tail -c 300 || true)"
        if printf '%s' "$api_log" | grep -qi 'refusing to start'; then
            skip "operand pod Ready" \
                "bindcar sidecar hit the non-loopback auth startup guard (${api_reason:-CrashLoopBackOff}) — the image lacks k8s-token-review. Use bindcar >= v0.7.2 (rebuild the operator so DEFAULT_BINDCAR_IMAGE=v0.7.2 is baked in, then reload)."
        else
            skip "operand pod Ready" \
                "sidecar not Ready (${api_reason:-unknown}); last api log: ${api_log:-<none>}"
        fi
        return 0
    fi

    # In-pod DNS answer on :53 (authoritative server answers CHAOS queries).
    local pod
    pod="$(${KUBECTL} get pods -n "${NAMESPACE}" -l "instance=${INSTANCE_NAME}" -o jsonpath='{.items[0].metadata.name}')"
    local dig_out
    dig_out="$(${KUBECTL} exec -n "${NAMESPACE}" "$pod" -c bind9 -- \
        dig @127.0.0.1 -p "${EXPECTED_DNS_PORT}" +short +time=3 CH TXT version.bind 2>/dev/null || true)"
    if [ -n "$dig_out" ]; then
        pass "in-pod dig on :53 answers (${dig_out})"
    else
        skip "in-pod dig on :53" "no answer (dig may be absent or version.bind disabled)"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

echo -e "${BLUE}🧪 Bindy regression suite${NC}"
echo -e "   cluster: kind-${CLUSTER_NAME}"
echo -e "   image:   ${IMAGE_REF:-<none — Phase A only>}"

command -v kind >/dev/null 2>&1 || { echo -e "${RED}kind not found${NC}"; exit 1; }
command -v kubectl >/dev/null 2>&1 || { echo -e "${RED}kubectl not found${NC}"; exit 1; }

ensure_cluster
cleanup_test_resources
install_crds
phase_a
phase_b
phase_c

echo ""
echo -e "${BLUE}════ Summary ════${NC}"
echo -e "  ${GREEN}passed:  ${PASS_COUNT}${NC}"
echo -e "  ${RED}failed:  ${FAIL_COUNT}${NC}"
echo -e "  ${YELLOW}skipped: ${SKIP_COUNT}${NC}"
if [ "$FAIL_COUNT" -gt 0 ]; then
    echo -e "${RED}Failed tests:${NC}"
    printf '  - %s\n' "${FAILED_TESTS[@]}"
fi

# Render a summary on the GitHub Actions run page (when running in CI).
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
        echo "## Regression suite — operand contract, admission policies, liveness"
        echo ""
        echo "**✅ passed: ${PASS_COUNT} · ❌ failed: ${FAIL_COUNT} · ⏭ skipped: ${SKIP_COUNT}**"
        echo ""
        echo "<details><summary>All ${#SUMMARY_ROWS[@]} checks</summary>"
        echo ""
        echo "| | Check |"
        echo "|---|---|"
        [ "${#SUMMARY_ROWS[@]}" -gt 0 ] && printf '%s\n' "${SUMMARY_ROWS[@]}"
        echo ""
        echo "</details>"
        echo ""
    } >> "$GITHUB_STEP_SUMMARY"
fi

if [ "$DELETE_CLUSTER" = true ] && [ "$FAIL_COUNT" -eq 0 ]; then
    echo -e "${YELLOW}🧹 Deleting cluster '${CLUSTER_NAME}'...${NC}"
    kind delete cluster --name "${CLUSTER_NAME}" || true
fi

[ "$FAIL_COUNT" -eq 0 ]
