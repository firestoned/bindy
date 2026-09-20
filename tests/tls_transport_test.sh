#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# End-to-end proof for audit finding P2-4: the operator reaches the bindcar
# sidecar over TLS, so the ServiceAccount token it presents is never written to
# the pod network in cleartext.
#
# Unit tests already cover client construction and the refusal to fall back to
# plaintext. What they cannot show is a real certificate being issued, served
# and verified, which is the whole of the finding. This script closes that gap
# with cert-manager issuing into the sidecar Secret and the operator completing
# real zone work against it.
#
# What is actually asserted, and why each one earns its place:
#
#   1. cert-manager issued the sidecar certificate. Without this the rest of the
#      run proves nothing about a real PKI.
#   2. The operator wired the sidecar for TLS: BIND_TLS_CERT / BIND_TLS_KEY and a
#      read-only mount of the Secret.
#   3. The sidecar serves TLS and REFUSES plaintext on the same port. This is the
#      server half of P2-4: if plaintext still answered, a token could still be
#      sent in the clear by anything on the pod network.
#   4. The operator completed real zone work over HTTPS — a DNSZone reconciled
#      and BIND9 actually serves the zone. This is the client half: the operator
#      verified the presented certificate against the configured CA and used the
#      connection. A zone that never becomes ready would mean TLS was configured
#      but unusable.
#   5. Negative control: an instance with TLS enabled but no CA bundle must be
#      REFUSED, not quietly downgraded. ADR-0004 rests on there being no
#      insecureSkipVerify escape hatch, and a fallback to plaintext would make
#      every assertion above cosmetic.
#
# cert-manager is installed from its static release manifest, not Helm, and the
# version is resolved from the latest GitHub release at run time so the gate
# tracks upstream instead of rotting on a pin. Set CERT_MANAGER_VERSION to pin
# it explicitly.
#
# Usage:
#   ./tests/tls_transport_test.sh                    # build image, full setup
#   ./tests/tls_transport_test.sh --image REF        # use a prebuilt image
#   ./tests/tls_transport_test.sh --skip-deploy      # reuse a running setup
#   KEEP_CLUSTER=1 ./tests/tls_transport_test.sh     # leave the cluster up

set -euo pipefail

GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

CLUSTER_NAME="${CLUSTER_NAME:=bindy-tls}"
NAMESPACE="${NAMESPACE:=bindy-system}"
DNS_CLUSTER="${DNS_CLUSTER:=tlsdemo}"
ZONE_NAME="${ZONE_NAME:=tls.test}"
KEEP_CLUSTER="${KEEP_CLUSTER:=}"

# The Secret cert-manager issues into, and the CA the operator is pinned to.
TLS_SECRET="bindcar-tls"
CA_SECRET="bindy-ca"

# Known-good release used only when the GitHub API cannot be reached (rate
# limiting on unauthenticated CI runners is the usual cause).
CERT_MANAGER_FALLBACK="v1.16.2"

# bindcar serves its API here; the sidecar listens on the same port whether or
# not TLS is on, which is what makes assertion 3 meaningful.
BINDCAR_PORT=8080
CURL_IMAGE="curlimages/curl:8.11.1"
# How long a probe Pod gets to pull, run and terminate.
PROBE_TIMEOUT_SECS=120

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

cleanup() {
  local code=$?
  if [ $code -ne 0 ]; then
    echo
    warn "Failed — dumping diagnostics"
    ${KUBECTL} get certificate,secret -n "${NAMESPACE}" 2>/dev/null || true
    ${KUBECTL} get bind9cluster,bind9instance,dnszone -n "${NAMESPACE}" 2>/dev/null || true
    # Deep enough to reach the FIRST reconcile. A short tail here once hid the
    # start of a failure behind five minutes of retry noise, and the cluster is
    # torn down immediately below, so this is the only chance to capture it.
    ${KUBECTL} logs -n "${NAMESPACE}" -l app=bindy --tail=2000 --timestamps 2>/dev/null || true
    ${KUBECTL} logs -n "${NAMESPACE}" -l app=bind9 --tail=500 --timestamps --all-containers 2>/dev/null || true
  fi
  # Managed Bind9Instances carry finalizers; leaving them mid-deletion makes an
  # immediately-following run race its own leftovers.
  ${KUBECTL} delete bind9cluster "${DNS_CLUSTER}" -n "${NAMESPACE}" \
    --ignore-not-found --timeout=120s >/dev/null 2>&1 || true
  if [ "${SKIP_DEPLOY}" = false ] && [ -z "${KEEP_CLUSTER}" ]; then
    kind delete cluster --name "${CLUSTER_NAME}" >/dev/null 2>&1 || true
    # Verify rather than assume. A silently-failed delete leaves a multi-GB
    # cluster behind, and several of those in a row is what fills the disk.
    if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
      warn "kind cluster ${CLUSTER_NAME} is STILL PRESENT after teardown; \
remove it with: kind delete cluster --name ${CLUSTER_NAME}"
    fi
  fi
}
trap cleanup EXIT

# Resolve the newest cert-manager release, falling back to a known-good pin.
latest_cert_manager() {
  if [ -n "${CERT_MANAGER_VERSION:-}" ]; then
    echo "${CERT_MANAGER_VERSION}"
    return
  fi
  # Parsed with grep/cut rather than jq: jq is not guaranteed on every runner,
  # and one field does not justify the dependency.
  local tag
  tag="$(curl -fsSL --max-time 15 \
      https://api.github.com/repos/cert-manager/cert-manager/releases/latest 2>/dev/null \
    | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' \
    | head -1 | cut -d'"' -f4 || true)"
  if [ -z "${tag}" ]; then
    echo "${CERT_MANAGER_FALLBACK}"
  else
    echo "${tag}"
  fi
}

# Run curl inside the cluster. Returns curl's exit status; output on stdout.
# A throwaway pod is used rather than exec-ing into the operand, because the
# BIND9 image has no HTTP client.
# Runs curl inside the cluster and echoes its stdout; returns curl's own exit
# status so callers can distinguish "answered" from "refused".
#
# Deliberately does NOT use `kubectl run --rm -i`. That form streams over an
# interactive attach, which is fragile across kubectl/apiserver version skew and
# fails in ways indistinguishable from the probe target being down. Create the
# Pod, wait for it to terminate, read its logs: boring, and it behaves the same
# on every version.
cluster_curl() {
  local name="$1"; shift
  ${KUBECTL} delete pod "${name}" -n "${NAMESPACE}" --ignore-not-found >/dev/null 2>&1 || true

  if ! ${KUBECTL} run "${name}" -n "${NAMESPACE}" --image="${CURL_IMAGE}" \
      --image-pull-policy=IfNotPresent --restart=Never \
      --command -- "$@" >/dev/null 2>&1; then
    return 1
  fi

  local deadline=$((SECONDS + PROBE_TIMEOUT_SECS)) phase=""
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    phase="$(${KUBECTL} get pod "${name}" -n "${NAMESPACE}" \
      -o jsonpath='{.status.phase}' 2>/dev/null || true)"
    case "${phase}" in
      Succeeded|Failed) break ;;
    esac
    sleep 2
  done

  ${KUBECTL} logs "${name}" -n "${NAMESPACE}" 2>/dev/null || true

  local code
  code="$(${KUBECTL} get pod "${name}" -n "${NAMESPACE}" \
    -o jsonpath='{.status.containerStatuses[0].state.terminated.exitCode}' 2>/dev/null || true)"
  ${KUBECTL} delete pod "${name}" -n "${NAMESPACE}" --ignore-not-found --wait=false \
    >/dev/null 2>&1 || true

  return "${code:-1}"
}

# Make sure the probe itself works before using it to judge the sidecar.
#
# Without this, a Docker Hub hiccup or a rate-limited pull reports "sidecar did
# not answer over HTTPS" — a false security failure, which is far worse than an
# honest infrastructure error. Observed exactly once in practice, which is how
# this check came to exist.
probe_selftest() {
  if ! cluster_curl tls-probe-selftest curl --version | grep -q "^curl "; then
    fail "probe image ${CURL_IMAGE} could not run — cannot judge the sidecar. \
This is an infrastructure failure, NOT a TLS failure."
  fi
  pass "probe image usable"
}

# ==========================================================================
# Setup
# ==========================================================================

if [ "${SKIP_DEPLOY}" = false ]; then
  for bin in kind kubectl curl; do
    command -v "${bin}" >/dev/null 2>&1 || fail "${bin} not found on PATH"
  done

  if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    step "Reusing existing kind cluster ${CLUSTER_NAME}"
  else
    step "Creating kind cluster ${CLUSTER_NAME}"
    # kind-config-tls.yaml publishes no host ports on purpose: ci-e2e leaves the
    # earlier suites' clusters running, and deploy/kind-config.yaml would try to
    # bind 30053/30953 a second time.
    kind create cluster --name "${CLUSTER_NAME}" \
      --config "${PROJECT_ROOT}/deploy/kind-config-tls.yaml" \
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
    step "Building operator image (scripts/build-docker-fast.sh local)"
    "${PROJECT_ROOT}/scripts/build-docker-fast.sh" local tlstransport \
      || fail "image build failed; pass --image REF with a prebuilt operator image"
    IMAGE_REF="${REGISTRY:-ghcr.io}/firestoned/bindy:tlstransport"
  fi

  if docker image inspect "${IMAGE_REF}" >/dev/null 2>&1; then
    step "Loading ${IMAGE_REF} into kind"
    kind load docker-image "${IMAGE_REF}" --name "${CLUSTER_NAME}" >/dev/null
  else
    warn "${IMAGE_REF} not in local docker; assuming the kind node can pull it"
  fi

  # Same reasoning as probe_selftest: pre-load so the probe cannot fail on a
  # registry outage mid-run. Best effort — the kind node can still pull it.
  step "Pre-loading probe image ${CURL_IMAGE}"
  if docker pull --quiet "${CURL_IMAGE}" >/dev/null 2>&1; then
    kind load docker-image "${CURL_IMAGE}" --name "${CLUSTER_NAME}" >/dev/null 2>&1 \
      || warn "could not load ${CURL_IMAGE} into kind; the node will pull it"
  else
    warn "could not pull ${CURL_IMAGE}; the node will pull it"
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

# ==========================================================================
# cert-manager
# ==========================================================================

step "Installing cert-manager"
CM_VERSION="$(latest_cert_manager)"
info "cert-manager ${CM_VERSION} (static manifest, no Helm)"

# The static manifest carries the CRDs, so no separate --set installCRDs step.
${KUBECTL} apply --server-side --force-conflicts \
  -f "https://github.com/cert-manager/cert-manager/releases/download/${CM_VERSION}/cert-manager.yaml" \
  >/dev/null || fail "failed to apply cert-manager ${CM_VERSION}"

${KUBECTL} wait --for=condition=available --timeout=300s \
  deployment --all -n cert-manager >/dev/null || fail "cert-manager did not become available"
pass "cert-manager ${CM_VERSION} available"

# Available does not mean the webhook is answering yet, and an Issuer created a
# moment too early fails with a connection refused from the webhook service.
# Retry rather than sleep a guessed interval.
step "Waiting for the cert-manager webhook to admit resources"
issued=false
for _ in $(seq 1 60); do
  if ${KUBECTL} apply -f - >/dev/null 2>&1 <<EOF
apiVersion: cert-manager.io/v1
kind: Issuer
metadata:
  name: selfsigned
  namespace: ${NAMESPACE}
spec:
  selfSigned: {}
EOF
  then
    issued=true
    break
  fi
  sleep 5
done
[ "${issued}" = true ] || fail "cert-manager webhook never admitted an Issuer"
pass "webhook admitting resources"

# ==========================================================================
# PKI: self-signed root -> dedicated CA -> sidecar certificate
# ==========================================================================
#
# The CA is dedicated to sidecar certificates on purpose. The operator's default
# verifier checks that the presented certificate chains to this bundle and does
# NOT check the dialled address against its SANs, because a certificate cannot
# carry a SAN for an ephemeral pod IP (see src/bind9/tls_client.rs). Any
# certificate from this CA is therefore accepted from any pod, which is only
# safe while the CA issues nothing else.

step "Issuing the sidecar certificate"
${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: ${CA_SECRET}
  namespace: ${NAMESPACE}
spec:
  isCA: true
  commonName: bindy-sidecar-ca
  secretName: ${CA_SECRET}
  privateKey:
    algorithm: ECDSA
    size: 256
  issuerRef:
    name: selfsigned
    kind: Issuer
    group: cert-manager.io
---
apiVersion: cert-manager.io/v1
kind: Issuer
metadata:
  name: bindy-ca-issuer
  namespace: ${NAMESPACE}
spec:
  ca:
    secretName: ${CA_SECRET}
---
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: ${TLS_SECRET}
  namespace: ${NAMESPACE}
spec:
  secretName: ${TLS_SECRET}
  commonName: bindcar
  dnsNames:
    - bindcar
    - bindcar.${NAMESPACE}.svc
  privateKey:
    algorithm: ECDSA
    size: 256
  issuerRef:
    name: bindy-ca-issuer
    kind: Issuer
    group: cert-manager.io
EOF

${KUBECTL} wait --for=condition=Ready --timeout=180s \
  certificate/"${CA_SECRET}" -n "${NAMESPACE}" >/dev/null || fail "CA certificate never became Ready"
${KUBECTL} wait --for=condition=Ready --timeout=180s \
  certificate/"${TLS_SECRET}" -n "${NAMESPACE}" >/dev/null || fail "sidecar certificate never became Ready"

# Assertion 1: a real certificate exists, issued by cert-manager.
for key in tls.crt tls.key; do
  ${KUBECTL} get secret "${TLS_SECRET}" -n "${NAMESPACE}" \
    -o "jsonpath={.data.${key//./\\.}}" 2>/dev/null | grep -q . \
    || fail "Secret ${TLS_SECRET} has no ${key}"
done
pass "cert-manager issued ${TLS_SECRET} (tls.crt + tls.key)"

# ==========================================================================
# Bind9Cluster with TLS enabled
# ==========================================================================

step "Creating Bind9Cluster with bindcar TLS enabled"
${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: ${DNS_CLUSTER}
  namespace: ${NAMESPACE}
spec:
  version: "9.18"
  primary:
    replicas: 1
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    bindcarConfig:
      imagePullPolicy: IfNotPresent
      logLevel: debug
      tls:
        enabled: true
        secretName: ${TLS_SECRET}
        caBundle:
          secretRef:
            name: ${CA_SECRET}
            key: ca.crt
EOF

INSTANCE="${DNS_CLUSTER}-primary-0"
step "Waiting for ${INSTANCE} to come up"
for _ in $(seq 1 60); do
  ${KUBECTL} get pods -n "${NAMESPACE}" -l "app.kubernetes.io/instance=${INSTANCE}" \
    --no-headers 2>/dev/null | grep -q . && break
  sleep 5
done
${KUBECTL} wait --for=condition=ready pod -l "app.kubernetes.io/instance=${INSTANCE}" \
  -n "${NAMESPACE}" --timeout=300s >/dev/null || fail "${INSTANCE} pod never became ready"
POD="$(${KUBECTL} get pods -n "${NAMESPACE}" -l "app.kubernetes.io/instance=${INSTANCE}" \
  --field-selector=status.phase=Running -o jsonpath='{.items[0].metadata.name}')"
pass "${INSTANCE} running (${POD})"

# Assertion 2: the operator wired the sidecar for TLS.
step "Checking the sidecar's TLS wiring"
sidecar_env="$(${KUBECTL} get pod "${POD}" -n "${NAMESPACE}" \
  -o jsonpath='{range .spec.containers[?(@.name=="api")].env[*]}{.name}={.value}{"\n"}{end}')"
grep -q '^BIND_TLS_CERT=' <<<"${sidecar_env}" || fail "sidecar has no BIND_TLS_CERT"
grep -q '^BIND_TLS_KEY=' <<<"${sidecar_env}" || fail "sidecar has no BIND_TLS_KEY"
pass "sidecar env carries BIND_TLS_CERT and BIND_TLS_KEY"

mount_ro="$(${KUBECTL} get pod "${POD}" -n "${NAMESPACE}" \
  -o jsonpath='{.spec.containers[?(@.name=="api")].volumeMounts[?(@.name=="bindcar-tls")].readOnly}')"
[ "${mount_ro}" = "true" ] || fail "bindcar-tls volume is not mounted read-only (got: '${mount_ro}')"
pass "certificate mounted read-only"

POD_IP="$(${KUBECTL} get pod "${POD}" -n "${NAMESPACE}" -o jsonpath='{.status.podIP}')"

# Assertion 3: the sidecar serves TLS and refuses plaintext on the same port.
# --insecure is deliberate here: this probe asks "is the wire encrypted at all",
# not "does the chain verify". Chain verification is the operator's job and is
# what assertion 4 exercises.
step "Probing the sidecar on ${POD_IP}:${BINDCAR_PORT}"
probe_selftest
if cluster_curl tls-probe-https curl -sS -k --max-time 10 \
     "https://${POD_IP}:${BINDCAR_PORT}/api/v1/health" | grep -qi "ok\|healthy\|status"; then
  pass "sidecar answers over HTTPS"
else
  fail "sidecar did not answer over HTTPS"
fi

if cluster_curl tls-probe-http curl -sS --max-time 10 \
     "http://${POD_IP}:${BINDCAR_PORT}/api/v1/health" >/dev/null 2>&1; then
  fail "sidecar still answers PLAINTEXT http:// — a token could be sent in the clear"
fi
pass "sidecar refuses plaintext on the same port"

# Assertion 4: the operator completed real zone work over the verified channel.
step "Creating a DNSZone and an A record"
${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: tls-zone
  namespace: ${NAMESPACE}
spec:
  zoneName: ${ZONE_NAME}
  clusterRef: ${DNS_CLUSTER}
  bind9InstancesFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/cluster: ${DNS_CLUSTER}
  recordsFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/zone: ${ZONE_NAME}
  nameServerIps:
    ns1.${ZONE_NAME}.: 192.0.2.53
  soaRecord:
    primaryNs: ns1.${ZONE_NAME}.
    adminEmail: admin.${ZONE_NAME}.
    serial: 2026010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: tls-a
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_NAME}
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.10"
  ttl: 300
EOF

# BIND9 answers on an unprivileged port so the Pod can drop NET_BIND_SERVICE.
step "Waiting for BIND9 to serve ${ZONE_NAME} (pushed over TLS)"
served=false
for _ in $(seq 1 60); do
  if ${KUBECTL} exec -n "${NAMESPACE}" "${POD}" -c bind9 -- \
       dig @127.0.0.1 -p 5353 +short A "www.${ZONE_NAME}." 2>/dev/null | grep -q "192.0.2.10"; then
    served=true
    break
  fi
  sleep 5
done
[ "${served}" = true ] || fail "zone never reached BIND9 — TLS was configured but unusable"
pass "BIND9 serves www.${ZONE_NAME} — the operator pushed it over TLS"

# Direct evidence of the scheme, so a passing zone cannot be explained by some
# other path having done the work.
if ${KUBECTL} logs -n "${NAMESPACE}" -l app=bindy --tail=2000 2>/dev/null \
   | grep -q "https://${POD_IP}:${BINDCAR_PORT}"; then
  pass "operator logged https:// calls to the sidecar"
else
  warn "no https:// URL found in operator logs (log level may hide it)"
fi

if ${KUBECTL} logs -n "${NAMESPACE}" -l app=bindy --tail=2000 2>/dev/null \
   | grep -q "http://${POD_IP}:${BINDCAR_PORT}"; then
  fail "operator made PLAINTEXT calls to a TLS-enabled sidecar"
fi
pass "operator made no plaintext calls to the sidecar"

# Assertion 5: half-configured TLS is refused, never downgraded.
step "Negative control: TLS enabled with no CA bundle must be refused"
${KUBECTL} apply -f - >/dev/null 2>&1 <<EOF || true
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: ${DNS_CLUSTER}-nocabundle
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/cluster: ${DNS_CLUSTER}
spec:
  clusterRef: ${DNS_CLUSTER}
  role: primary
  replicas: 1
  bindcarConfig:
    tls:
      enabled: true
      secretName: ${TLS_SECRET}
EOF

# Either the CRD/admission rejects it outright, or the operator refuses to
# reconcile it. Both are correct; silently running it in plaintext is not.
sleep 30
if ${KUBECTL} get bind9instance "${DNS_CLUSTER}-nocabundle" -n "${NAMESPACE}" >/dev/null 2>&1; then
  nocabundle_pod="$(${KUBECTL} get pods -n "${NAMESPACE}" \
    -l "app.kubernetes.io/instance=${DNS_CLUSTER}-nocabundle" \
    --field-selector=status.phase=Running -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)"
  if [ -n "${nocabundle_pod}" ]; then
    env_has_tls="$(${KUBECTL} get pod "${nocabundle_pod}" -n "${NAMESPACE}" \
      -o jsonpath='{range .spec.containers[?(@.name=="api")].env[*]}{.name}{"\n"}{end}' 2>/dev/null \
      | grep -c '^BIND_TLS_CERT$' || true)"
    [ "${env_has_tls:-0}" -eq 0 ] \
      && fail "half-configured instance is RUNNING IN PLAINTEXT — silent downgrade"
  fi
  pass "half-configured instance was not brought up in plaintext"
  ${KUBECTL} delete bind9instance "${DNS_CLUSTER}-nocabundle" -n "${NAMESPACE}" \
    --ignore-not-found --timeout=60s >/dev/null 2>&1 || true
else
  pass "half-configured instance rejected at admission"
fi

echo
echo -e "${GREEN}✅ P2-4 verified end to end${NC}"
echo "  cert-manager:  ${CM_VERSION}"
echo "  certificate:   ${TLS_SECRET} (CA: ${CA_SECRET})"
echo "  sidecar:       TLS only, plaintext refused"
echo "  operator:      zone pushed over a CA-verified HTTPS connection"
