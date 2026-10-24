#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# E2E suite: Scout routes follow the addresses of their Gateway (ADR-0028,
# issue #569), proven on two kind clusters.
#
#   bindy cluster (QUEEN_CLUSTER): the operator, a Bind9Cluster with primaries
#     and a secondary, the zone gw.test and a registered Scout account.
#   edge cluster (EDGE_CLUSTER): the Gateway API CRDs (standard channel),
#     Scout with two --gateway-service targets, Gateways and their
#     LoadBalancer Services (kind has no load-balancer or gateway controller,
#     so the suite writes their status itself), and HTTPRoutes.
#
# Every DNS assertion digs every live primary AND secondary pod. Phases:
#   1. a label selector publishes every matching VIP, and a VIP that gets its
#      address later is published without touching the route;
#   2. VIPs that are deleted, readdressed or terminating leave DNS;
#   3. Services labelled gateway.networking.k8s.io/gateway-name (GEP-1762)
#      serve only their own Gateway's routes;
#   4. a Gateway's status.addresses take precedence, and clearing them falls
#      back to the Services;
#   5. a stray Service outside --allowed-ip-ranges is dropped with a warning
#      and the class keeps publishing;
#   6. a Scout restart rewrites nothing; Scout goes quiet;
#   7. removing the routes removes every record and name.
#
# Usage:
#   tests/e2e/scout_gateway_test.sh --image ghcr.io/firestoned/bindy:<build>

# Settings below are read by tests/lib/scout_clusters.sh.
# shellcheck disable=SC2034
set -uo pipefail

QUEEN_CLUSTER="${QUEEN_CLUSTER:-bindy-e2e-queen}"
EDGE_CLUSTER="${EDGE_CLUSTER:-bindy-e2e-edge}"
# cluster.sh and chaos.sh act on CLUSTER_NAME / KUBECTL: the bindy cluster.
# shellcheck disable=SC2034  # read by tests/lib/cluster.sh
CLUSTER_NAME="${QUEEN_CLUSTER}"
KUBECTL="kubectl --context kind-${QUEEN_CLUSTER}"
KEDGE="kubectl --context kind-${EDGE_CLUSTER}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"
source "${LIB_DIR}/chaos.sh"
source "${LIB_DIR}/scout_clusters.sh"

parse_common_args "$@"

[ -n "${IMAGE_REF}" ] || { echo "--image is required" >&2; exit 2; }
BINDY_BIN="${BINDY_BIN:-${PROJECT_ROOT}/target/x86_64-unknown-linux-gnu/debug/bindy}"
GATEWAY_API_VERSION="${GATEWAY_API_VERSION:-v1.3.0}"
GATEWAY_API_CRDS="https://github.com/kubernetes-sigs/gateway-api/releases/download/${GATEWAY_API_VERSION}/standard-install.yaml"

ZONE="gw.test"
ZONE_CR="gw-test"
EDGE_NAME="edge-a"
SCOUT_ACCOUNT="bindy-scout-remote"
APPS="team-a"
GW_NS="gateways"
SCOUT_CONVERGE_TIMEOUT="${SCOUT_CONVERGE_TIMEOUT:-240}"
SCOUT_QUIET_WINDOW="${SCOUT_QUIET_WINDOW:-60}"

# The traefik class: one Service per VIP, shared by every Gateway of the
# class. Only the Services labelled public are the gateway's VIPs; the
# internal one carries the chart's name label too.
TRAEFIK_LABEL="app.kubernetes.io/name"
PUBLIC_LABEL="example.com/dns-vip"
TRAEFIK_TARGET="traefik=${GW_NS}/${TRAEFIK_LABEL}=traefik,${PUBLIC_LABEL}=public"
# The envoy class: one Service per Gateway, labelled per GEP-1762.
GATEWAY_NAME_LABEL="gateway.networking.k8s.io/gateway-name"
ENVOY_TARGET="envoy=${GW_NS}/app=envoy"

VIP_A="10.0.0.11"
VIP_B="10.0.0.12"
VIP_C="10.0.0.13"
VIP_D="10.0.0.14"
VIP_A2="10.0.0.21"
INTERNAL_VIP="10.0.0.99"
GW_STATUS_IP="10.0.5.1"
ENVOY_1_IP="10.0.6.1"
ENVOY_2_IP="10.0.6.2"
STRAY_IP="192.0.2.50"

STATE="${CHAOS_STATE_DIR}"
QUEEN_KUBECONFIG="${STATE}/queen.kubeconfig"
EDGE_KUBECONFIG="${STATE}/edge.kubeconfig"
SUITE_TITLE="E2E: Scout routes follow their Gateway's addresses (ADR-0028)"

info "🧪 E2E: Scout routes follow their Gateway (bindy '${QUEEN_CLUSTER}', edge '${EDGE_CLUSTER}')"
info "   Scout ${IMAGE_REF}, Gateway API ${GATEWAY_API_VERSION}, state ${STATE}"

# ─────────────────────────────────────────────────────────────────────────────
# Edge objects
# ─────────────────────────────────────────────────────────────────────────────

# A gateway LoadBalancer Service in GW_NS (no Scout opt-in: it is not a
# source) with its load-balancer status set.
#
# $1 = name, $2 = labels as a flow map ("{a: b, c: d}"), rest = IPs (none:
# the address is still pending)
gw_service() {
    local name=$1 labels=$2; shift 2
    ${KEDGE} apply -f - >/dev/null <<YAML
apiVersion: v1
kind: Service
metadata:
  name: ${name}
  namespace: ${GW_NS}
  labels: ${labels}
spec:
  type: LoadBalancer
  selector: {app: ${name}}
  ports: [{port: 443, targetPort: 8443, protocol: TCP}]
YAML
    [ "$#" -gt 0 ] && lb_status "${GW_NS}" "${name}" "$@"
    return 0
}

# A Gateway in GW_NS of class $2.
gateway() {
    ${KEDGE} apply -f - >/dev/null <<YAML
apiVersion: gateway.networking.k8s.io/v1
kind: Gateway
metadata:
  name: $1
  namespace: ${GW_NS}
spec:
  gatewayClassName: $2
  listeners: [{name: http, port: 80, protocol: HTTP}]
YAML
}

# Sets a Gateway's status.addresses ($1 = Gateway, rest = IPs; none clears).
gateway_status() {
    local name=$1 addrs; shift
    addrs=$(printf '{"type":"IPAddress","value":"%s"},' "$@"); addrs="[${addrs%,}]"
    [ "$#" -eq 0 ] && addrs="[]"
    must ${KEDGE} patch gateway "${name}" -n "${GW_NS}" --subresource=status --type=merge \
        -p "{\"status\":{\"addresses\":${addrs}}}"
}

# An opted-in HTTPRoute in APPS publishing $2.<zone>, attached to Gateway $3
# in GW_NS. No IP annotation: its addresses come from the Gateway.
route() {
    ${KEDGE} apply -f - >/dev/null <<YAML
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: $1
  namespace: ${APPS}
  annotations:
    bindy.firestoned.io/scout-enabled: "true"
    bindy.firestoned.io/zone: "${ZONE}"
spec:
  hostnames: ["$2.${ZONE}"]
  parentRefs: [{name: $3, namespace: ${GW_NS}}]
  rules: [{backendRefs: [{name: app, port: 8080}]}]
YAML
}

route_version() { ${KEDGE} get httproute "$1" -n "${APPS}" -o jsonpath='{.metadata.resourceVersion}'; }

# "name generation" of every Scout record: a rewrite bumps the generation,
# a status write by the operator does not.
scout_record_generations() {
    scout_records_json | jq -r '.items[] | "\(.metadata.name) \(.metadata.generation)"' | sort
}

scout_dropped() { scout_logs | grep "Dropping a discovered gateway address" | grep -q "$1"; }

# ─────────────────────────────────────────────────────────────────────────────
# Setup
# ─────────────────────────────────────────────────────────────────────────────

SCOUT_IMAGES=("${IMAGE_REF}")
SCOUT_ALLOWED_ZONES="${ZONE}"
SCOUT_ALLOWED_RANGES="10.0.0.0/8"

scout_setup_queen "${SCOUT_IMAGES[@]}"
step "Zone ${ZONE}"
scout_apply_zone "${ZONE_CR}" "${ZONE}" "${APPS}"
scout_queen_ready

scout_setup_edge "${EDGE_CLUSTER}" "${EDGE_KUBECONFIG}" "${APPS}" "${GW_NS}"
step "Gateway API ${GATEWAY_API_VERSION} CRDs (standard channel) on the edge"
must ${KEDGE} apply --server-side -f "${GATEWAY_API_CRDS}"
must ${KEDGE} wait --for=condition=Established --timeout=60s \
    crd/gateways.gateway.networking.k8s.io crd/httproutes.gateway.networking.k8s.io

phase "Setup: registered Scout account and Scout"
scout_register "${SCOUT_ACCOUNT}" "${EDGE_NAME}" "${ZONE}"
bootstrap_scout "${IMAGE_REF}"
step "--gateway-service ${TRAEFIK_TARGET} and ${ENVOY_TARGET}"
must ${KEDGE} patch deployment bindy-scout -n "${NAMESPACE}" --type=json -p "[
  {\"op\":\"add\",\"path\":\"/spec/template/spec/containers/0/args/-\",\"value\":\"--gateway-service=${TRAEFIK_TARGET}\"},
  {\"op\":\"add\",\"path\":\"/spec/template/spec/containers/0/args/-\",\"value\":\"--gateway-service=${ENVOY_TARGET}\"}]"
must ${KEDGE} rollout status deployment/bindy-scout -n "${NAMESPACE}" --timeout=180s
wait_for "Scout watches Gateways" bash -c "${KEDGE} logs -n ${NAMESPACE} deployment/bindy-scout | grep -q 'watching.*HTTPRoutes'"

# ─────────────────────────────────────────────────────────────────────────────
# 1. Every matching VIP, including one that gets its address later
# ─────────────────────────────────────────────────────────────────────────────

phase "1. A label selector publishes every matching VIP (issue #569)"
P=${ERRORS}
PUBLIC="{${TRAEFIK_LABEL}: traefik, ${PUBLIC_LABEL}: public}"
gw_service vip-a "${PUBLIC}" "${VIP_A}"
gw_service vip-b "${PUBLIC}" "${VIP_B}"
gw_service vip-c "${PUBLIC}"
gw_service traefik-internal "{${TRAEFIK_LABEL}: traefik}" "${INTERNAL_VIP}"
gateway gw-t traefik
route api api gw-t
expect_a api "${VIP_A}" "${VIP_B}"
pass "the internal Service the chart labels alike is not published (VIP-specific selector)"
API_RV=$(route_version api)
lb_status "${GW_NS}" vip-c "${VIP_C}"
expect_a api "${VIP_A}" "${VIP_B}" "${VIP_C}"
if [ "$(route_version api)" = "${API_RV}" ]; then
    pass "the late VIP was published without the route changing"
else
    fail "the route changed while only a Service got its address"
fi
record_phase "Every matching VIP published, a late one without touching the route" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 2. VIPs leave DNS
# ─────────────────────────────────────────────────────────────────────────────

phase "2. Deleted, readdressed and terminating VIPs leave DNS"
P=${ERRORS}
must ${KEDGE} delete service vip-b -n "${GW_NS}"
expect_a api "${VIP_A}" "${VIP_C}"
lb_status "${GW_NS}" vip-a "${VIP_A2}"
expect_a api "${VIP_A2}" "${VIP_C}"
step "vip-c held in Terminating by a finalizer"
must ${KEDGE} patch service vip-c -n "${GW_NS}" --type=merge -p '{"metadata":{"finalizers":["example.com/hold"]}}'
must ${KEDGE} delete service vip-c -n "${GW_NS}" --wait=false
expect_a api "${VIP_A2}"
must ${KEDGE} patch service vip-c -n "${GW_NS}" --type=merge -p '{"metadata":{"finalizers":null}}'
record_phase "Deleted, readdressed and terminating VIPs leave DNS" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 3. GEP-1762: one Service per Gateway
# ─────────────────────────────────────────────────────────────────────────────

phase "3. Services labelled for a Gateway serve only its routes (GEP-1762)"
P=${ERRORS}
gw_service envoy-gw-e1 "{app: envoy, ${GATEWAY_NAME_LABEL}: gw-e1}" "${ENVOY_1_IP}"
gw_service envoy-gw-e2 "{app: envoy, ${GATEWAY_NAME_LABEL}: gw-e2}" "${ENVOY_2_IP}"
gateway gw-e1 envoy
gateway gw-e2 envoy
route e1 e1 gw-e1
route e2 e2 gw-e2
expect_a e1 "${ENVOY_1_IP}"
expect_a e2 "${ENVOY_2_IP}"
record_phase "Each Gateway's routes publish only its own Service" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 4. Gateway status.addresses
# ─────────────────────────────────────────────────────────────────────────────

phase "4. A Gateway's status.addresses come first; clearing them falls back"
P=${ERRORS}
gateway_status gw-t "${GW_STATUS_IP}"
expect_a api "${GW_STATUS_IP}"
gateway_status gw-t
expect_a api "${VIP_A2}"
record_phase "Gateway status changes re-resolve its routes" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 5. --allowed-ip-ranges drops a stray discovered address
# ─────────────────────────────────────────────────────────────────────────────

phase "5. A stray Service outside --allowed-ip-ranges is dropped, the class keeps publishing"
P=${ERRORS}
gw_service stray "${PUBLIC}" "${STRAY_IP}"
wait_for "Scout dropped ${STRAY_IP} with a warning" scout_dropped "${STRAY_IP}"
expect_a api "${VIP_A2}"
gw_service vip-d "${PUBLIC}" "${VIP_D}"
expect_a api "${VIP_A2}" "${VIP_D}"
record_phase "A stray address is dropped; the class keeps publishing" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 6. Restart and quiet
# ─────────────────────────────────────────────────────────────────────────────

phase "6. A Scout restart rewrites nothing; Scout goes quiet"
P=${ERRORS}
G0=$(scout_record_generations)
${KEDGE} delete pod -n "${NAMESPACE}" -l app.kubernetes.io/component=scout --wait=false >/dev/null
must ${KEDGE} rollout status deployment/bindy-scout -n "${NAMESPACE}" --timeout=180s
step "Scout quiet window (${SCOUT_QUIET_WINDOW}s)"
sleep "${SCOUT_QUIET_WINDOW}"
G1=$(scout_record_generations)
if [ "${G0}" = "${G1}" ]; then
    pass "no Scout record rewritten across the restart ($(echo "${G1}" | wc -l) records)"
else
    fail "records rewritten: $(diff <(echo "${G0}") <(echo "${G1}") | tr '\n' ' ')"
fi
expect_a api "${VIP_A2}" "${VIP_D}"
expect_a e1 "${ENVOY_1_IP}"
expect_a e2 "${ENVOY_2_IP}"
if [ "$(scout_restarts)" -eq 0 ]; then pass "Scout container never crashed"; else fail "Scout restarted $(scout_restarts) times"; fi
if scout_logs | grep -qE '(^|[[:space:]])ERROR[[:space:]]'; then
    warn "Scout logged errors: $(scout_logs | grep -E '(^|[[:space:]])ERROR[[:space:]]' | tail -3 | cut -c1-200 | tr '\n' '|')"
fi
scout_records_table
record_phase "A restart rewrites nothing" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 7. Removing the routes
# ─────────────────────────────────────────────────────────────────────────────

phase "7. Remove every route"
P=${ERRORS}
${KEDGE} delete httproutes -n "${APPS}" --all --wait=true >/dev/null
wait_for "no Scout record left on the bindy cluster" bash -c \
    "[ \$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/managed-by=scout -o name | wc -l) -eq 0 ]"
for n in api e1 e2; do expect_a "${n}"; done
record_phase "Removing every route removes every record and name" "${P}"

finish "${SUITE_TITLE}"
