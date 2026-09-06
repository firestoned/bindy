#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# End-to-end verification of `spec.placement` zone spreading, against a kind
# cluster faking a three-zone region (deploy/kind-config-multizone.yaml).
#
# The interesting assertion is not "pods ended up in different zones". The
# scheduler's default scoring will often achieve that on its own, so a passing
# spread proves nothing by itself and is not a contract when the default rule
# is soft. What this script actually proves is:
#
#   1. The generated constraint's labelSelector matches the SIBLING primaries,
#      not just the Deployment's own single Pod. With a per-instance selector
#      the balanced set would have size 1 and the constraint would be a no-op.
#   2. A hard constraint is genuinely enforced: with two of three zones
#      cordoned, the extra primaries must go Pending with a topology-spread
#      reason. That can only happen if (1) holds.
#   3. spec.selector stays free of the cluster label, since it is immutable.
#   4. Changes to placement converge onto existing Deployments — including
#      REMOVALS, which is where strategic merge patch is easy to get wrong.
#   5. The automatic default covers primaries ONLY. Secondaries keep whatever
#      scheduling they had until they opt in, so upgrading the operator does
#      not silently move running secondary workloads.
#   6. The generated CRD schema rejects invalid rules on its own, with no
#      ValidatingAdmissionPolicy installed.
#
# Self-contained by default: creates its own kind cluster, installs the CRDs
# and RBAC, and deploys the operator. Requires only kind, kubectl and docker.
#
# Usage:
#   ./tests/zone_spread_test.sh                      # build image, full setup
#   ./tests/zone_spread_test.sh --image REF          # use a prebuilt image
#   ./tests/zone_spread_test.sh --skip-deploy        # reuse a running setup
#   KEEP_CLUSTER=1 ./tests/zone_spread_test.sh       # leave the cluster up

set -euo pipefail

GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

CLUSTER_NAME="${CLUSTER_NAME:=bindy-zonespread}"
NAMESPACE="${NAMESPACE:=bindy-system}"
DNS_CLUSTER="${DNS_CLUSTER:=zonespread}"
KEEP_CLUSTER="${KEEP_CLUSTER:=}"
PRIMARIES=3

# Always address the cluster explicitly. Relying on the ambient context makes
# the script destructive if someone's kubeconfig points at production.
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

IMAGE_REF=""
SKIP_DEPLOY=false
while [[ $# -gt 0 ]]; do
  case $1 in
    --image) IMAGE_REF="$2"; shift 2 ;;
    --skip-deploy) SKIP_DEPLOY=true; shift ;;
    *) echo -e "${RED}Unknown option: $1${NC}"; echo "Usage: $0 [--image REF] [--skip-deploy]"; exit 1 ;;
  esac
done

pass() { echo -e "${GREEN}✓${NC} $1"; }
fail() { echo -e "${RED}✗${NC} $1"; exit 1; }
info() { echo -e "${YELLOW}▸${NC} $1"; }
warn() { echo -e "${YELLOW}!${NC} $1"; }
step() { echo -e "${BLUE}==>${NC} $1"; }

# `repeat <char> <count>` — avoids a python3 dependency for the long-key cases.
repeat() { printf "%*s" "$2" '' | tr ' ' "$1"; }

# The Deployment metadata keeps its original label set; the cluster label lives
# on the POD TEMPLATE only (spec.selector is immutable). So Deployments are
# listed by name, not by selector.
primary_deployments() {
  ${KUBECTL} get deploy -n "${NAMESPACE}" -o name 2>/dev/null \
    | sed 's|^deployment.apps/||' \
    | grep "^${DNS_CLUSTER}-primary-" || true
}

primary_pods_jsonpath() {
  ${KUBECTL} get pods -n "${NAMESPACE}" \
    -l "bindy.firestoned.io/cluster=${DNS_CLUSTER},bindy.firestoned.io/role=primary" \
    "$@" 2>/dev/null || true
}

cleanup() {
  local rc=$?
  if [ "${rc}" -ne 0 ]; then
    echo
    warn "Failing — cluster diagnostics:"
    ${KUBECTL} get pods -n "${NAMESPACE}" -o wide 2>/dev/null | head -20 || true
    ${KUBECTL} get events -n "${NAMESPACE}" --field-selector reason=FailedScheduling \
      -o jsonpath='{range .items[*]}{.message}{"\n"}{end}' 2>/dev/null | sort -u | head -5 || true
  fi
  info "Cleaning up"
  ${KUBECTL} uncordon -l topology.kubernetes.io/zone >/dev/null 2>&1 || true
  # Wait for the delete: managed Bind9Instances carry finalizers, and leaving
  # them mid-deletion makes an immediately-following run race its own leftovers.
  ${KUBECTL} delete bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" \
    --ignore-not-found --timeout=120s >/dev/null 2>&1 || true
  if [ "${SKIP_DEPLOY}" = false ] && [ -z "${KEEP_CLUSTER}" ]; then
    kind delete cluster --name "${CLUSTER_NAME}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

# ==========================================================================
# Setup
# ==========================================================================

if [ "${SKIP_DEPLOY}" = false ]; then
  for bin in kind kubectl; do
    command -v "${bin}" >/dev/null 2>&1 || fail "${bin} not found on PATH"
  done

  if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    step "Reusing existing kind cluster ${CLUSTER_NAME}"
  else
    step "Creating three-zone kind cluster ${CLUSTER_NAME}"
    kind create cluster --name "${CLUSTER_NAME}" \
      --config "${PROJECT_ROOT}/deploy/kind-config-multizone.yaml" \
      || fail "failed to create kind cluster"
  fi

  ${KUBECTL} wait --for=condition=Ready node --all --timeout=180s >/dev/null \
    || fail "nodes did not become Ready"

  step "Installing CRDs"
  # --server-side: the cluster CRDs exceed the 256KB last-applied-configuration
  # annotation that client-side apply relies on.
  ${KUBECTL} apply --server-side --force-conflicts \
    -f "${PROJECT_ROOT}/deploy/operator/crds/" >/dev/null \
    || fail "failed to install CRDs"

  step "Creating namespace and RBAC"
  ${KUBECTL} create namespace "${NAMESPACE}" --dry-run=client -o yaml | ${KUBECTL} apply -f - >/dev/null
  ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/operator/rbac/" >/dev/null

  if [ -z "${IMAGE_REF}" ]; then
    command -v docker >/dev/null 2>&1 || fail "docker not found (pass --image to use a prebuilt operator image)"
    # Use the repo's own local build path rather than `docker build .`:
    # docker/Dockerfile consumes pre-built binaries from binaries/<arch>/ and
    # cannot build from source. `build-docker-fast.sh local` cross-compiles a
    # Linux binary with cargo and wraps it in docker/Dockerfile.local.
    step "Building operator image (scripts/build-docker-fast.sh local)"
    "${PROJECT_ROOT}/scripts/build-docker-fast.sh" local zonespread \
      || fail "image build failed. Cross-compiling to Linux needs the target toolchain \
(rustup target add ${LINUX_TARGET:-x86_64-unknown-linux-gnu} plus a linker); \
on a machine without it, pass --image REF with a prebuilt operator image."
    IMAGE_REF="${REGISTRY:-ghcr.io}/firestoned/bindy:zonespread"
  fi

  if docker image inspect "${IMAGE_REF}" >/dev/null 2>&1; then
    step "Loading ${IMAGE_REF} into kind"
    kind load docker-image "${IMAGE_REF}" --name "${CLUSTER_NAME}" >/dev/null
  else
    warn "${IMAGE_REF} not in local docker; assuming the kind node can pull it"
  fi

  step "Deploying operator (${IMAGE_REF})"
  sed "s|ghcr.io/firestoned/bindy:latest|${IMAGE_REF}|g" \
    "${PROJECT_ROOT}/deploy/operator/deployment.yaml" | ${KUBECTL} apply -f - >/dev/null

  ${KUBECTL} wait --for=condition=available --timeout=300s \
    deployment/bindy -n "${NAMESPACE}" || {
      ${KUBECTL} logs -n "${NAMESPACE}" -l app=bindy --tail=50 || true
      fail "operator failed to become available"
    }
  pass "operator running"
else
  info "Skipping setup; using the running cluster ${CLUSTER_NAME}"
  ${KUBECTL} get nodes >/dev/null 2>&1 || fail "cluster ${CLUSTER_NAME} is not reachable"
fi

echo

# Start from a clean slate: a previous (or interrupted) run may still be
# tearing down instances of the same name.
info "Waiting for any previous ${DNS_CLUSTER} resources to finish deleting"
${KUBECTL} delete bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" \
  --ignore-not-found --timeout=120s >/dev/null 2>&1 || true
leftover=0; stale_pods=0
for _ in $(seq 1 90); do
  leftover=$(${KUBECTL} get bind9instance -n "${NAMESPACE}" \
    -l "bindy.firestoned.io/cluster=${DNS_CLUSTER}" -o name 2>/dev/null | wc -l | tr -d ' ')
  stale_pods=$(primary_pods_jsonpath -o name | wc -l | tr -d ' ')
  [ "${leftover}" -eq 0 ] && [ "${stale_pods}" -eq 0 ] && break
  sleep 2
done
[ "${leftover}" -eq 0 ] && [ "${stale_pods}" -eq 0 ] \
  || fail "previous ${DNS_CLUSTER} resources still terminating (${leftover} instances, ${stale_pods} pods)"

# --------------------------------------------------------------------------
info "Checking the cluster really has three zones"
# --------------------------------------------------------------------------
ZONES=()
while IFS= read -r z; do
  [ -n "${z}" ] && ZONES+=("${z}")
done < <(${KUBECTL} get nodes \
  -o jsonpath='{range .items[*]}{.metadata.labels.topology\.kubernetes\.io/zone}{"\n"}{end}' \
  | tr ' ' '\n' | grep -v '^$' | sort -u || true)

if [ "${#ZONES[@]}" -lt 3 ]; then
  found="none"; [ "${#ZONES[@]}" -gt 0 ] && found="${ZONES[*]}"
  fail "need at least 3 zones, found ${#ZONES[@]}: ${found}. Use deploy/kind-config-multizone.yaml"
fi
pass "found ${#ZONES[@]} zones: ${ZONES[*]}"

# --------------------------------------------------------------------------
info "Creating a ${PRIMARIES}-primary Bind9Cluster with no placement block (tests the DEFAULT)"
# --------------------------------------------------------------------------
${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: ${DNS_CLUSTER}
  namespace: ${NAMESPACE}
spec:
  version: "9.18"
  primary:
    replicas: ${PRIMARIES}
EOF

running=0
for _ in $(seq 1 60); do
  # Distinct owning instances, not raw Pods: a Pod mid-rollout or
  # mid-termination would otherwise be double-counted.
  running=$(primary_pods_jsonpath --field-selector=status.phase=Running \
    -o jsonpath='{range .items[*]}{.metadata.labels.app\.kubernetes\.io/instance}{"\n"}{end}' \
    | grep -v '^$' | sort -u | wc -l | tr -d ' ' || true)
  [ "${running}" -ge "${PRIMARIES}" ] && break
  sleep 5
done
[ "${running}" -ge "${PRIMARIES}" ] || fail "only ${running}/${PRIMARIES} primary instances have a running pod"
pass "${running} primary instances have a running pod"

# --------------------------------------------------------------------------
info "TEST 1: the constraint's selector spans sibling Deployments"
# --------------------------------------------------------------------------
DEPLOY="${DNS_CLUSTER}-primary-0"
SELECTOR=$(${KUBECTL} get deploy "${DEPLOY}" -n "${NAMESPACE}" \
  -o jsonpath='{.spec.template.spec.topologySpreadConstraints[0].labelSelector.matchLabels}')
[ -n "${SELECTOR}" ] || fail "no topologySpreadConstraints on ${DEPLOY} (the default should have applied)"

echo "${SELECTOR}" | grep -q "bindy.firestoned.io/cluster" \
  || fail "constraint selector lacks the cluster label; it would balance a set of one: ${SELECTOR}"
echo "${SELECTOR}" | grep -q "bindy.firestoned.io/role" \
  || fail "constraint selector lacks the role label: ${SELECTOR}"
echo "${SELECTOR}" | grep -q "app.kubernetes.io/instance" \
  && fail "constraint selector is per-instance; it would spread nothing: ${SELECTOR}"
pass "selector is cross-instance: ${SELECTOR}"

MATCHED_INSTANCES=$(primary_pods_jsonpath \
  -o jsonpath='{range .items[*]}{.metadata.labels.app\.kubernetes\.io/instance}{"\n"}{end}' \
  | grep -v '^$' | sort -u | wc -l | tr -d ' ' || true)
[ "${MATCHED_INSTANCES}" -eq "${PRIMARIES}" ] \
  || fail "constraint selector spans ${MATCHED_INSTANCES} instance(s), expected ${PRIMARIES}"
pass "selector spans all ${MATCHED_INSTANCES} sibling primary Deployments"

# --------------------------------------------------------------------------
info "TEST 2: spec.selector does NOT carry the cluster label (it is immutable)"
# --------------------------------------------------------------------------
${KUBECTL} get deploy "${DEPLOY}" -n "${NAMESPACE}" -o jsonpath='{.spec.selector.matchLabels}' \
  | grep -q "bindy.firestoned.io/cluster" \
  && fail "cluster label leaked into the immutable spec.selector; upgrades of existing Deployments would fail"
pass "spec.selector unchanged; the new label lives on the Pod template only"

# --------------------------------------------------------------------------
info "TEST 3: zone occupancy under the SOFT default (informational)"
# --------------------------------------------------------------------------
# Deliberately not a hard assertion. The default rule is ScheduleAnyway, so
# even distribution is scheduler scoring, not a contract — asserting on it
# would make this suite flaky under unrelated cluster pressure. TEST 5 makes
# the contractual claim, using a hard constraint.
USED_ZONES=$(primary_pods_jsonpath --field-selector=status.phase=Running \
  -o jsonpath='{range .items[*]}{.spec.nodeName}{"\n"}{end}' \
  | grep -v '^$' | sort -u \
  | while read -r n; do
      ${KUBECTL} get node "${n}" -o jsonpath='{.metadata.labels.topology\.kubernetes\.io/zone}{"\n"}' 2>/dev/null
    done | sort -u | grep -c . || true)
if [ "${USED_ZONES}" -ge 3 ]; then
  pass "primaries occupy ${USED_ZONES} distinct zones"
else
  warn "primaries occupy only ${USED_ZONES} zone(s); the soft default does not guarantee more (not a failure)"
fi

# --------------------------------------------------------------------------
info "TEST 4: placement changes converge onto EXISTING Deployments"
# --------------------------------------------------------------------------
${KUBECTL} patch bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" --type=merge -p '{
  "spec": {"primary": {"placement": {"spread": [
    {"topologyKey": "topology.kubernetes.io/zone", "maxSkew": 1,
     "whenUnsatisfiable": "DoNotSchedule", "minDomains": 3}]}}}}' >/dev/null

# Wait for EVERY primary Deployment, not just the first. TEST 5 is only
# meaningful once all of them carry the hard constraint: a single primary still
# running the soft rule can fill the one schedulable zone, leaving nothing
# Pending and silently passing a test that proves nothing.
converged=0
for _ in $(seq 1 90); do
  converged=0
  for d in $(primary_deployments); do
    got=$(${KUBECTL} get deploy "${d}" -n "${NAMESPACE}" \
      -o jsonpath='{.spec.template.spec.topologySpreadConstraints[0].whenUnsatisfiable}' 2>/dev/null)
    [ "${got}" = "DoNotSchedule" ] && converged=$((converged + 1))
  done
  [ "${converged}" -eq "${PRIMARIES}" ] && break
  sleep 2
done
[ "${converged}" -eq "${PRIMARIES}" ] \
  || fail "only ${converged}/${PRIMARIES} primary Deployments converged to the hard constraint"

# Wait for the rollouts, so no Pod from a previous ReplicaSet (still carrying
# the old soft rule) is occupying a zone when TEST 5 runs. A rollout that never
# completes is a real failure, not something to shrug off.
for d in $(primary_deployments); do
  ${KUBECTL} rollout status "deploy/${d}" -n "${NAMESPACE}" --timeout=180s >/dev/null \
    || fail "rollout of ${d} did not complete after the placement change"
done
pass "all ${PRIMARIES} primary Deployments converged to whenUnsatisfiable=DoNotSchedule"

# --------------------------------------------------------------------------
info "TEST 5: the hard constraint is genuinely ENFORCED"
# --------------------------------------------------------------------------
# Cordon every zone but the first. With maxSkew 1 and one schedulable zone, at
# most ONE primary may run: the others must go Pending. With a per-instance
# selector every Pod would happily schedule instead.
for z in "${ZONES[@]:1}"; do
  ${KUBECTL} cordon -l "topology.kubernetes.io/zone=${z}" >/dev/null
done
primary_pods_jsonpath -o name | xargs -r -n1 ${KUBECTL} delete -n "${NAMESPACE}" --wait=false >/dev/null 2>&1 || true

PENDING=0
for _ in $(seq 1 30); do
  PENDING=$(primary_pods_jsonpath --field-selector=status.phase=Pending -o name | wc -l | tr -d ' ')
  [ "${PENDING}" -ge 2 ] && break
  sleep 5
done
if [ "${PENDING}" -lt 2 ]; then
  echo "--- diagnostic: primary pods ---"
  primary_pods_jsonpath -o wide | head
  echo "--- diagnostic: constraints per Deployment ---"
  for d in $(primary_deployments); do
    echo -n "${d}  "
    ${KUBECTL} get deploy "${d}" -n "${NAMESPACE}" \
      -o jsonpath='{.spec.template.spec.topologySpreadConstraints}{"\n"}' 2>&1
  done
  fail "expected >=2 Pending primaries with only one schedulable zone, got ${PENDING}"
fi

# Events lag behind the scheduling decision, so poll rather than sampling once.
spread_cited=""
for _ in $(seq 1 30); do
  if ${KUBECTL} get events -n "${NAMESPACE}" --field-selector reason=FailedScheduling \
       -o jsonpath='{range .items[*]}{.message}{"\n"}{end}' 2>/dev/null \
       | grep -q "topology spread constraints"; then
    spread_cited=yes; break
  fi
  sleep 3
done
if [ -z "${spread_cited}" ]; then
  echo "--- diagnostic: FailedScheduling messages ---"
  ${KUBECTL} get events -n "${NAMESPACE}" --field-selector reason=FailedScheduling \
    -o jsonpath='{range .items[*]}{.message}{"\n"}{end}' 2>&1 | sort -u | head -5
  fail "pods are Pending but not for a topology-spread reason"
fi
pass "${PENDING} primaries Pending, scheduler cites topology spread constraints"

for z in "${ZONES[@]:1}"; do
  ${KUBECTL} uncordon -l "topology.kubernetes.io/zone=${z}" >/dev/null
done

# --------------------------------------------------------------------------
info "TEST 6: removing placement removes the constraint (strategic-merge removal)"
# --------------------------------------------------------------------------
${KUBECTL} patch bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" --type=merge -p '{
  "spec": {"primary": {"placement": {"spread": []}}}}' >/dev/null

tsc="unset"
for _ in $(seq 1 40); do
  tsc=$(${KUBECTL} get deploy "${DEPLOY}" -n "${NAMESPACE}" \
    -o jsonpath='{.spec.template.spec.topologySpreadConstraints}' 2>/dev/null)
  [ -z "${tsc}" ] && break
  sleep 2
done
[ -z "${tsc}" ] || fail "constraints survived the opt-out; strategic merge merged instead of replacing: ${tsc}"
pass "constraints removed by 'spread: []'"

# --------------------------------------------------------------------------
info "TEST 7: a Deployment predating this feature is upgraded in place"
# --------------------------------------------------------------------------
# Simulate the operator-upgrade case by stripping the Pod-template label and
# the constraints from a live Deployment, leaving the (immutable) spec.selector
# alone. The operator must repair it WITHOUT touching spec.selector — a widened
# selector would be rejected as immutable and wedge the reconciler.
${KUBECTL} patch bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" --type=merge \
  -p '{"spec": {"primary": {"placement": null}}}' >/dev/null
for _ in $(seq 1 40); do
  tsc=$(${KUBECTL} get deploy "${DEPLOY}" -n "${NAMESPACE}" \
    -o jsonpath='{.spec.template.spec.topologySpreadConstraints}' 2>/dev/null)
  [ -n "${tsc}" ] && break
  sleep 2
done
[ -n "${tsc}" ] || fail "default spread did not come back after clearing placement"

SELECTOR_BEFORE=$(${KUBECTL} get deploy "${DEPLOY}" -n "${NAMESPACE}" -o jsonpath='{.spec.selector.matchLabels}')

${KUBECTL} patch deploy "${DEPLOY}" -n "${NAMESPACE}" --type=json -p '[
  {"op":"remove","path":"/spec/template/spec/topologySpreadConstraints"},
  {"op":"remove","path":"/spec/template/metadata/labels/bindy.firestoned.io~1cluster"}]' >/dev/null

for _ in $(seq 1 40); do
  tsc=$(${KUBECTL} get deploy "${DEPLOY}" -n "${NAMESPACE}" \
    -o jsonpath='{.spec.template.spec.topologySpreadConstraints}' 2>/dev/null)
  [ -n "${tsc}" ] && break
  sleep 2
done
[ -n "${tsc}" ] || fail "stale Deployment was never repaired; the reconcile short-circuit skipped it"

SELECTOR_AFTER=$(${KUBECTL} get deploy "${DEPLOY}" -n "${NAMESPACE}" -o jsonpath='{.spec.selector.matchLabels}')
[ "${SELECTOR_BEFORE}" = "${SELECTOR_AFTER}" ] \
  || fail "spec.selector changed during repair (immutable field): '${SELECTOR_BEFORE}' -> '${SELECTOR_AFTER}'"
pass "stale Deployment repaired in place, spec.selector untouched"

# --------------------------------------------------------------------------
info "TEST 8: secondaries get NO default, and can opt in"
# --------------------------------------------------------------------------
# Defaulting secondaries would silently change where already-running secondary
# Pods schedule the moment an operator is upgraded. They must stay untouched
# until asked.
${KUBECTL} patch bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" --type=merge \
  -p '{"spec": {"secondary": {"replicas": 3}}}' >/dev/null

SEC_DEPLOY="${DNS_CLUSTER}-secondary-0"
for _ in $(seq 1 60); do
  ${KUBECTL} get deploy "${SEC_DEPLOY}" -n "${NAMESPACE}" >/dev/null 2>&1 && break
  sleep 5
done
${KUBECTL} get deploy "${SEC_DEPLOY}" -n "${NAMESPACE}" >/dev/null 2>&1 \
  || fail "secondary Deployment ${SEC_DEPLOY} was never created"

# Wait for the operator to finish with the secondary rather than sleeping a
# fixed interval: the rollout completing means it has been fully reconciled, so
# a constraint appearing later would be a real regression, not a race.
${KUBECTL} rollout status "deploy/${SEC_DEPLOY}" -n "${NAMESPACE}" --timeout=180s >/dev/null \
  || fail "secondary Deployment never rolled out"

# Then hold the negative for a few reconcile ticks, failing fast if one appears.
for _ in $(seq 1 10); do
  sec_tsc=$(${KUBECTL} get deploy "${SEC_DEPLOY}" -n "${NAMESPACE}" \
    -o jsonpath='{.spec.template.spec.topologySpreadConstraints}' 2>/dev/null)
  [ -z "${sec_tsc}" ] \
    || fail "secondaries received an automatic spread constraint; that is an unannounced scheduling change on upgrade: ${sec_tsc}"
  sleep 2
done
pass "secondaries have no constraint by default"

${KUBECTL} patch bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" --type=merge -p '{
  "spec": {"secondary": {"placement": {"spread": [
    {"topologyKey": "topology.kubernetes.io/zone", "maxSkew": 1,
     "whenUnsatisfiable": "ScheduleAnyway"}]}}}}' >/dev/null

sec_tsc=""
for _ in $(seq 1 40); do
  sec_tsc=$(${KUBECTL} get deploy "${SEC_DEPLOY}" -n "${NAMESPACE}" \
    -o jsonpath='{.spec.template.spec.topologySpreadConstraints}' 2>/dev/null)
  [ -n "${sec_tsc}" ] && break
  sleep 2
done
[ -n "${sec_tsc}" ] || fail "secondary opt-in did not take effect"
echo "${sec_tsc}" | grep -q '"bindy.firestoned.io/role":"secondary"' \
  || fail "secondary constraint selects the wrong role: ${sec_tsc}"
pass "secondaries opt in explicitly, selecting sibling secondaries"

# --------------------------------------------------------------------------
info "TEST 9: the CRD schema rejects invalid rules WITHOUT an admission policy"
# --------------------------------------------------------------------------
# These limits are structural (maxItems / pattern / minimum /
# x-kubernetes-validations), so they hold on any cluster with the CRDs
# installed — no optional ValidatingAdmissionPolicy required.
${KUBECTL} get validatingadmissionpolicy 2>/dev/null | grep -q placement \
  && fail "a placement admission policy is installed; TEST 9 would not prove schema enforcement"

schema_reject() {
  local desc="$1" patch="$2"
  if ${KUBECTL} patch bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" \
       --type=merge -p "${patch}" >/dev/null 2>&1; then
    fail "schema accepted an invalid spread rule (${desc})"
  fi
  pass "schema rejected: ${desc}"
}

schema_reject "minDomains without DoNotSchedule" '{"spec":{"primary":{"placement":{"spread":[
  {"topologyKey":"topology.kubernetes.io/zone","minDomains":3,"whenUnsatisfiable":"ScheduleAnyway"}]}}}}'
schema_reject "malformed topologyKey" '{"spec":{"primary":{"placement":{"spread":[
  {"topologyKey":"not a valid key!"}]}}}}'
schema_reject "maxSkew of 0" '{"spec":{"primary":{"placement":{"spread":[
  {"topologyKey":"topology.kubernetes.io/zone","maxSkew":0}]}}}}'

# maxItems: 8.
NINE_RULES=$(for i in $(seq 1 9); do printf '{"topologyKey":"k%s.example.com/zone"},' "${i}"; done | sed 's/,$//')
schema_reject "9 spread rules (max is 8)" \
  "{\"spec\":{\"primary\":{\"placement\":{\"spread\":[${NINE_RULES}]}}}}"

# 253-character prefix cap, enforced by CEL because RE2 has no lookahead.
A63=$(repeat a 63)
LONG_PREFIX="${A63}.${A63}.${A63}.$(repeat a 62)"     # 254 chars
schema_reject "topologyKey prefix over 253 characters" \
  "{\"spec\":{\"primary\":{\"placement\":{\"spread\":[{\"topologyKey\":\"${LONG_PREFIX}/zone\"}]}}}}"

# ...and the maximal VALID key (253 + '/' + 63 = 317) must still be accepted,
# so the caps are not merely tight.
MAX_PREFIX="${A63}.${A63}.${A63}.$(repeat a 61)"      # 253 chars
MAX_NAME=$(repeat b 63)
${KUBECTL} patch bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" --type=merge \
  -p "{\"spec\":{\"primary\":{\"placement\":{\"spread\":[{\"topologyKey\":\"${MAX_PREFIX}/${MAX_NAME}\"}]}}}}" \
  >/dev/null 2>&1 \
  || fail "schema rejected a maximally-sized but VALID 317-character topologyKey"
pass "schema accepted the maximal valid topologyKey (317 chars)"

echo
echo -e "${GREEN}All zone-spreading checks passed.${NC}"
