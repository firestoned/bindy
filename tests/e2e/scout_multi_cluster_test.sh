#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# E2E suite: Scout publishes one record per DNS name (ADR-0026, issue #556),
# proven on two kind clusters.
#
#   bindy cluster (QUEEN_CLUSTER): the operator, a Bind9Cluster with primaries
#     and a secondary, the zone scout.test, admission policies 21 and 22 and a
#     registered Scout account (`bindy bootstrap mc`, ADR-0025).
#   edge cluster (EDGE_CLUSTER): Scout, installed with `bindy bootstrap scout`,
#     writing to the bindy cluster through the registered account.
#
# Every DNS assertion digs every live primary AND secondary pod. Phases:
#   1. the Scout from BASE_IMAGE_REF (one record per source) reproduces #556;
#   2. upgrading Scout hands every record over without a DNS gap;
#   3. churn: delete, readdress, opt out, rename, multi-IP Service;
#   4. kinds, namespaces and TTLs sharing one name;
#   5. a burst of sources racing on one name, then a mass delete;
#   6. chaos: Scout, operator and BIND9 kills during churn, then convergence,
#      a quiet operator and a Scout that writes nothing more;
#   7. removing every source removes every record and name.
#
# Usage:
#   BASE_IMAGE_REF=ghcr.io/firestoned/bindy:<main-build> \
#     tests/e2e/scout_multi_cluster_test.sh --image ghcr.io/firestoned/bindy:<build>
# Both images must already be loadable (podman/docker) on this host.

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

parse_common_args "$@"

BASE_IMAGE_REF="${BASE_IMAGE_REF:?set BASE_IMAGE_REF to the image Scout is upgraded from}"
[ -n "${IMAGE_REF}" ] || { echo "--image is required" >&2; exit 2; }
BINDY_BIN="${BINDY_BIN:-${PROJECT_ROOT}/target/x86_64-unknown-linux-gnu/debug/bindy}"
NODE_RUNTIME="${KIND_EXPERIMENTAL_PROVIDER:-docker}"

ZONE="scout.test"
ZONE_CR="scout-test"
EDGE_NAME="edge-a"
SCOUT_ACCOUNT="bindy-scout-remote"
TEAM_A="team-a"
TEAM_B="team-b"
SCOUT_CONVERGE_TIMEOUT="${SCOUT_CONVERGE_TIMEOUT:-240}"
SCOUT_QUIET_WINDOW="${SCOUT_QUIET_WINDOW:-60}"
PROBE_INTERVAL_SECS=1
RACE_COUNT=10
RACE_KEEP=5
CHURN_FLIPS=10

VIP_A="10.0.0.246"
VIP_B="10.0.0.241"
VIP_C="10.0.0.240"
VIP_A2="10.0.0.250"
WWW_IP="10.0.1.10"
MULTI_IPS=("10.0.3.1" "10.0.3.2")
SHARED_INGRESS_IP="10.0.2.1"
SHARED_SERVICE_IP="10.0.2.2"
FLAP_IPS=("10.0.4.1" "10.0.4.2")

STATE="${CHAOS_STATE_DIR}"
QUEEN_KUBECONFIG="${STATE}/queen.kubeconfig"
EDGE_KUBECONFIG="${STATE}/edge.kubeconfig"
VIOLATIONS_FILE="${STATE}/violations"
: > "${VIOLATIONS_FILE}"

info "🧪 E2E: Scout one record per DNS name on two clusters (bindy '${QUEEN_CLUSTER}', edge '${EDGE_CLUSTER}')"
info "   base Scout ${BASE_IMAGE_REF}, new ${IMAGE_REF}, state ${STATE}"

# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────

tag_of() { echo "${1##*:}"; }

# Setup steps must not fail quietly: a missing policy or account would turn
# every later assertion into a test of something else.
must() {
    "$@" && return 0
    fail "setup step failed: $*"
    finish "E2E: Scout one record per DNS name, two clusters (ADR-0026)"
    exit 1
}

# Every live primary and secondary pod IP of the zone's cluster.
server_ips() { { live_ips_of_role primary; live_ips_of_role secondary; } | sort -u; }

# "a;b;c;" for a list of addresses, sorted the way dig_all sorts answers.
answer_set() {
    [ $# -eq 0 ] && { echo ""; return; }
    printf '%s\n' "$@" | sort | tr '\n' ';'
}

# Waits until every server answers exactly the addresses given for the A
# query of $1 (a relative name, "@" for the apex). No addresses: an empty
# answer is expected.
#
# $1 = name, rest = addresses
expect_a() {
    local name=$1; shift
    local fqdn want deadline got ip bad
    fqdn=$([ "${name}" = "@" ] && echo "${ZONE}" || echo "${name}.${ZONE}")
    want=$(answer_set "$@")
    deadline=$(( $(now) + SCOUT_CONVERGE_TIMEOUT ))
    while [ "$(now)" -lt "${deadline}" ]; do
        bad=""
        got=$(for ip in $(server_ips); do echo "${ip} A ${fqdn}"; done | dig_all)
        [ -n "${got}" ] || { sleep "${CHAOS_POLL}"; continue; }
        while IFS='|' read -r ip _t _q ans; do
            [ "${ans}" = "${want}" ] || bad="${bad} ${ip}=[${ans}]"
        done <<< "${got}"
        if [ -z "${bad}" ]; then
            pass "${fqdn} A = [${want}] on $(server_ips | wc -l) servers"
            return 0
        fi
        sleep "${CHAOS_POLL}"
    done
    fail "${fqdn} A: wanted [${want}] on every server, got:${bad}"
    scout_records_table
    return 1
}

# The Scout records on the bindy cluster: "spec.name addresses contributors".
scout_records_table() {
    ${KUBECTL} get arecords -n "${NAMESPACE}" -l bindy.firestoned.io/managed-by=scout -o json 2>/dev/null \
        | jq -r '.items[] | "    \(.metadata.name) name=\(.spec.name) ips=\(.spec.ipv4Addresses|join(",")) ttl=\(.spec.ttl // "-") contributors=\(.metadata.annotations["bindy.firestoned.io/scout-contributors"] // "-") ready=\([.status.conditions[]?|select(.type=="Ready")|.status]|first // "-")"'
}

scout_records_json() {
    ${KUBECTL} get arecords -n "${NAMESPACE}" -l bindy.firestoned.io/managed-by=scout -o json 2>/dev/null
}

# Number of Scout records publishing name $1.
records_for_name() {
    scout_records_json | jq --arg n "$1" '[.items[] | select(.spec.name == $n)] | length'
}

# Contributor keys of the record publishing name $1, sorted, space-separated.
contributors_for_name() {
    scout_records_json | jq -r --arg n "$1" \
        '[.items[] | select(.spec.name == $n) | .metadata.annotations["bindy.firestoned.io/scout-contributors"] // "{}" | fromjson | keys[]] | sort | join(" ")'
}

per_source_record_count() {
    ${KUBECTL} get arecords -n "${NAMESPACE}" \
        -l bindy.firestoned.io/managed-by=scout,bindy.firestoned.io/source-kind -o name 2>/dev/null | wc -l
}

# Waits for a condition command to succeed.
wait_for() {
    local what=$1; shift
    local deadline=$(( $(now) + SCOUT_CONVERGE_TIMEOUT ))
    while [ "$(now)" -lt "${deadline}" ]; do
        "$@" && { pass "${what}"; return 0; }
        sleep "${CHAOS_POLL}"
    done
    fail "${what} (timed out after ${SCOUT_CONVERGE_TIMEOUT}s)"
    return 1
}

no_per_source_records() { [ "$(per_source_record_count)" -eq 0 ]; }
name_has_records() { [ "$(records_for_name "$1")" -eq "$2" ]; }
name_has_contributors() { [ "$(contributors_for_name "$1")" = "$2" ]; }

# A LoadBalancer Service on the edge cluster with its load-balancer status set
# (kind has no LB controller, so nothing else writes it).
#
# $1 = namespace, $2 = name, $3 = record-name ("" for the Service name),
# $4 = TTL ("" for none), rest = load-balancer IPs
lb_service() {
    local ns=$1 name=$2 record=$3 ttl=$4; shift 4
    local ann ingress
    ann="    bindy.firestoned.io/scout-enabled: \"true\"
    bindy.firestoned.io/zone: \"${ZONE}\""
    [ -n "${record}" ] && ann="${ann}
    bindy.firestoned.io/record-name: \"${record}\""
    [ -n "${ttl}" ] && ann="${ann}
    bindy.firestoned.io/ttl: \"${ttl}\""
    ${KEDGE} apply -f - >/dev/null <<YAML
apiVersion: v1
kind: Service
metadata:
  name: ${name}
  namespace: ${ns}
  annotations:
${ann}
spec:
  type: LoadBalancer
  selector: {app: ${name}}
  ports: [{port: 443, targetPort: 8443, protocol: TCP}]
YAML
    ingress=$(printf '{"ip":"%s"},' "$@"); ingress="[${ingress%,}]"
    ${KEDGE} patch service "${name}" -n "${ns}" --subresource=status --type=merge \
        -p "{\"status\":{\"loadBalancer\":{\"ingress\":${ingress}}}}" >/dev/null
}

# Only rewrite a Service's load-balancer addresses.
lb_status() {
    local ns=$1 name=$2; shift 2
    local ingress
    ingress=$(printf '{"ip":"%s"},' "$@"); ingress="[${ingress%,}]"
    ${KEDGE} patch service "${name}" -n "${ns}" --subresource=status --type=merge \
        -p "{\"status\":{\"loadBalancer\":{\"ingress\":${ingress}}}}" >/dev/null
}

# An Ingress publishing host $3.<zone> at $4 (bindy.firestoned.io/ip), TTL $5.
ingress_src() {
    local ns=$1 name=$2 host=$3 ip=$4 ttl=${5:-}
    local ttl_ann=""
    [ -n "${ttl}" ] && ttl_ann="
    bindy.firestoned.io/ttl: \"${ttl}\""
    ${KEDGE} apply -f - >/dev/null <<YAML
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: ${name}
  namespace: ${ns}
  annotations:
    bindy.firestoned.io/scout-enabled: "true"
    bindy.firestoned.io/zone: "${ZONE}"
    bindy.firestoned.io/ip: "${ip}"${ttl_ann}
spec:
  rules:
    - host: ${host}.${ZONE}
YAML
}

# TTL a primary serves for name $1.
served_ttl() {
    local ip
    ip=$(live_ips_of_role primary | head -1)
    kexec "${CHAOS_TOOLS_POD}" -- dig +noall +answer @"${ip}" -p "${CHAOS_DNS_PORT}" A "$1.${ZONE}" \
        | awk '{print $2}' | sort -u | head -1
}

scout_logs() {
    ${KEDGE} logs -n "${NAMESPACE}" deployment/bindy-scout --tail=-1 2>/dev/null || true
}

scout_restarts() {
    ${KEDGE} get pods -n "${NAMESPACE}" -l app.kubernetes.io/component=scout -o json 2>/dev/null \
        | jq '[.items[].status.containerStatuses[]?.restartCount] | add // 0'
}

# "name resourceVersion" of every Scout record, to prove Scout stopped writing.
scout_record_versions() {
    scout_records_json | jq -r '.items[] | "\(.metadata.name) \(.metadata.resourceVersion)"' | sort
}

# Background DNS prober: every PROBE_INTERVAL_SECS, the A answer of each name
# on every server, one line "<epoch> <server> <name> <answer>".
PROBE_LOG="${STATE}/probe.log"
PROBE_PID=""
start_prober() {
    local names=("$@") servers
    servers=$(server_ips | tr '\n' ' ')
    : > "${PROBE_LOG}"
    (
        while :; do
            t=$(now)
            for n in "${names[@]}"; do
                fqdn=$([ "${n}" = "@" ] && echo "${ZONE}" || echo "${n}.${ZONE}")
                for s in ${servers}; do echo "${s} A ${fqdn}"; done
            done | dig_all | sed "s/^/${t} /" >> "${PROBE_LOG}"
            sleep "${PROBE_INTERVAL_SECS}"
        done
    ) &
    PROBE_PID=$!
}
stop_prober() {
    [ -n "${PROBE_PID}" ] && kill "${PROBE_PID}" 2>/dev/null
    wait "${PROBE_PID}" 2>/dev/null || true
    PROBE_PID=""
}

# Probe samples of name $1 with an empty answer.
empty_samples() {
    local fqdn
    fqdn=$([ "$1" = "@" ] && echo "${ZONE}" || echo "$1.${ZONE}")
    awk -F'|' -v q="${fqdn}" '$3 == q && $4 == "" {n++} END {print n+0}' "${PROBE_LOG}"
}
total_samples() {
    local fqdn
    fqdn=$([ "$1" = "@" ] && echo "${ZONE}" || echo "$1.${ZONE}")
    awk -F'|' -v q="${fqdn}" '$3 == q {n++} END {print n+0}' "${PROBE_LOG}"
}

bootstrap_scout() {
    must env KUBECONFIG="${EDGE_KUBECONFIG}" "${BINDY_BIN}" bootstrap scout \
        --version "$(tag_of "$1")" \
        --cluster-name "${EDGE_NAME}" \
        --remote-secret "${SCOUT_ACCOUNT}-kubeconfig" \
        --allowed-zones "${ZONE}" \
        --allowed-ip-ranges 10.0.0.0/8
    must ${KEDGE} rollout status deployment/bindy-scout -n "${NAMESPACE}" --timeout=180s
}

record_phase() {
    summary_row "$1" "$([ "${ERRORS}" -eq "$2" ] && echo '✅ passed' || echo '❌ failed')"
}

# ─────────────────────────────────────────────────────────────────────────────
# Setup
# ─────────────────────────────────────────────────────────────────────────────

phase "Setup: bindy cluster '${QUEEN_CLUSTER}'"
bindy_ensure_cluster deploy/kind-config-chaos.yaml
for img in "${IMAGE_REF}" "${BASE_IMAGE_REF}"; do
    kind load docker-image "${img}" --name "${QUEEN_CLUSTER}" >/dev/null
done
bindy_deploy_operator
kind get kubeconfig --name "${QUEEN_CLUSTER}" > "${QUEEN_KUBECONFIG}"

step "Installing the Scout source-binding admission policy (21, 22)"
must ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/admission-policies/21-bindy-scout-source-binding-policy.yaml"
must ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/admission-policies/22-bindy-scout-source-binding-binding.yaml"

step "Bind9Cluster (primaries + secondary) and zone ${ZONE}"
must apply_chaos_cluster
must ${KUBECTL} apply -f - <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: ${ZONE_CR}
  namespace: ${NAMESPACE}
  annotations:
    bindy.firestoned.io/allow-zone-namespaces: "${TEAM_A},${TEAM_B}"
spec:
  zoneName: ${ZONE}
  clusterRef: ${CHAOS_CLUSTER_CR}
  bind9InstancesFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/cluster: ${CHAOS_CLUSTER_CR}
  recordsFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/zone: ${ZONE}
  nameServerIps:
    ns1.${ZONE}.: 192.0.2.53
  soaRecord:
    primaryNs: ns1.${ZONE}.
    adminEmail: admin.${ZONE}.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 300
  ttl: 300
YAML
wait_bind_pods_ready || fail "BIND9 pods never became ready"
apply_tools_pods
pass "bindy cluster ready: primaries [$(live_ips_of_role primary | tr '\n' ' ')] secondaries [$(live_ips_of_role secondary | tr '\n' ' ')]"

phase "Setup: edge cluster '${EDGE_CLUSTER}' and registered Scout account"
if ! kind get clusters 2>/dev/null | grep -qx "${EDGE_CLUSTER}"; then
    kind create cluster --name "${EDGE_CLUSTER}" --config "${PROJECT_ROOT}/deploy/kind-scout-config.yaml"
fi
for img in "${IMAGE_REF}" "${BASE_IMAGE_REF}"; do
    kind load docker-image "${img}" --name "${EDGE_CLUSTER}" >/dev/null
done
kind get kubeconfig --name "${EDGE_CLUSTER}" > "${EDGE_KUBECONFIG}"
for ns in "${NAMESPACE}" "${TEAM_A}" "${TEAM_B}"; do
    ${KEDGE} create namespace "${ns}" --dry-run=client -o yaml | ${KEDGE} apply -f - >/dev/null
done

QUEEN_IP=$(${NODE_RUNTIME} inspect -f '{{(index .NetworkSettings.Networks "kind").IPAddress}}' \
    "${QUEEN_CLUSTER}-control-plane")
step "Registering ${SCOUT_ACCOUNT} for cluster ${EDGE_NAME}, zone ${ZONE} (API https://${QUEEN_IP}:6443)"
must env KUBECONFIG="${QUEEN_KUBECONFIG}" "${BINDY_BIN}" bootstrap mc \
    --service-account "${SCOUT_ACCOUNT}" \
    --server "https://${QUEEN_IP}:6443" \
    --cluster-name "${EDGE_NAME}" --allowed-zones "${ZONE}" \
    -o "${STATE}/mc-secret.yaml"
must ${KEDGE} apply -n "${NAMESPACE}" -f "${STATE}/mc-secret.yaml"
rm -f "${STATE}/mc-secret.yaml"
pass "registered: $(${KUBECTL} get configmap bindy-scout-sources -n "${NAMESPACE}" -o jsonpath='{.data}')"

# ─────────────────────────────────────────────────────────────────────────────
# 1. The per-source Scout reproduces #556
# ─────────────────────────────────────────────────────────────────────────────

phase "1. Base Scout (${BASE_IMAGE_REF}): one record per source reproduces #556"
P=${ERRORS}
bootstrap_scout "${BASE_IMAGE_REF}"
pass "Scout ${BASE_IMAGE_REF} running on ${EDGE_CLUSTER}"
for pair in "svc-a ${VIP_A}" "svc-b ${VIP_B}" "svc-c ${VIP_C}"; do
    set -- ${pair}
    lb_service "${TEAM_A}" "$1" "@" "" "$2"
done
ingress_src "${TEAM_A}" web www "${WWW_IP}"
wait_for "base Scout wrote 4 per-source records" bash -c "[ \$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/managed-by=scout,bindy.firestoned.io/source-kind -o name | wc -l) -eq 4 ]"
expect_a www "${WWW_IP}"
# Three records claim the apex; whichever wrote last owns the RRset.
sleep "${SETTLE_SECS:-10}"
APEX_BEFORE=$(for ip in $(live_ips_of_role primary); do echo "${ip} A ${ZONE}"; done | dig_all | awk -F'|' '{print $4}' | sort -u)
APEX_COUNT=$(echo "${APEX_BEFORE}" | head -1 | tr ';' '\n' | grep -c . || true)
if [ "${APEX_COUNT}" -lt 3 ]; then
    pass "#556 reproduced: the apex serves [${APEX_BEFORE//$'\n'/ / }], not all three addresses"
else
    warn "the base Scout served all three apex addresses; #556 did not reproduce"
fi
record_phase "Base Scout reproduces #556 (one source's addresses at the apex)" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 2. Upgrade: per-source records handed over, no DNS gap
# ─────────────────────────────────────────────────────────────────────────────

phase "2. Upgrade Scout to ${IMAGE_REF}: handover without a gap"
P=${ERRORS}
start_prober www "@"
bootstrap_scout "${IMAGE_REF}"
pass "Scout upgraded"
expect_a "@" "${VIP_A}" "${VIP_B}" "${VIP_C}"
expect_a www "${WWW_IP}"
wait_for "every per-source record handed over (none left)" no_per_source_records
wait_for "one record publishes the apex" name_has_records "@" 1
wait_for "the apex record lists the three Services" name_has_contributors "@" \
    "Service/${TEAM_A}/svc-a Service/${TEAM_A}/svc-b Service/${TEAM_A}/svc-c"
sleep "${PROBE_INTERVAL_SECS}"; sleep "${PROBE_INTERVAL_SECS}"
stop_prober
WWW_EMPTY=$(empty_samples www); WWW_TOTAL=$(total_samples www)
APEX_EMPTY=$(empty_samples "@"); APEX_TOTAL=$(total_samples "@")
if [ "${WWW_EMPTY}" -eq 0 ] && [ "${WWW_TOTAL}" -gt 0 ]; then
    pass "www.${ZONE} answered in all ${WWW_TOTAL} probe samples during the handover"
else
    fail "www.${ZONE} had ${WWW_EMPTY} empty answers in ${WWW_TOTAL} samples during the handover"
fi
if [ "${APEX_EMPTY}" -eq 0 ] && [ "${APEX_TOTAL}" -gt 0 ]; then
    pass "${ZONE} answered in all ${APEX_TOTAL} probe samples during the handover"
else
    fail "${ZONE} had ${APEX_EMPTY} empty answers in ${APEX_TOTAL} samples during the handover"
fi
scout_records_table
record_phase "Upgrade hands over per-source records with no empty answer" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 3. Churn on a shared name
# ─────────────────────────────────────────────────────────────────────────────

phase "3. Churn: delete, readdress, opt out, rename, multi-IP"
P=${ERRORS}
step "Delete svc-b"
${KEDGE} delete service svc-b -n "${TEAM_A}" --wait=true >/dev/null
expect_a "@" "${VIP_A}" "${VIP_C}"
step "Readdress svc-a"
lb_status "${TEAM_A}" svc-a "${VIP_A2}"
expect_a "@" "${VIP_A2}" "${VIP_C}"
step "Opt svc-c out"
${KEDGE} annotate service svc-c -n "${TEAM_A}" bindy.firestoned.io/scout-enabled- >/dev/null
expect_a "@" "${VIP_A2}"
step "Rename svc-a from @ to api"
${KEDGE} annotate service svc-a -n "${TEAM_A}" bindy.firestoned.io/record-name=api --overwrite >/dev/null
expect_a api "${VIP_A2}"
expect_a "@"
wait_for "the apex record is gone with its last source" name_has_records "@" 0
step "A Service with two load-balancer addresses"
lb_service "${TEAM_A}" svc-multi multi "" "${MULTI_IPS[@]}"
expect_a multi "${MULTI_IPS[@]}"
step "Delete svc-a"
${KEDGE} delete service svc-a -n "${TEAM_A}" --wait=true >/dev/null
expect_a api
wait_for "the api record is gone" name_has_records api 0
record_phase "Churn on a shared name keeps exactly the live sources' addresses" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 4. Kinds, namespaces and TTLs sharing one name
# ─────────────────────────────────────────────────────────────────────────────

phase "4. An Ingress (${TEAM_A}) and a Service (${TEAM_B}) share 'shared'"
P=${ERRORS}
ingress_src "${TEAM_A}" shared-ing shared "${SHARED_INGRESS_IP}" 300
lb_service "${TEAM_B}" shared-svc shared 60 "${SHARED_SERVICE_IP}"
expect_a shared "${SHARED_INGRESS_IP}" "${SHARED_SERVICE_IP}"
wait_for "one record, two contributors of two kinds and namespaces" name_has_contributors shared \
    "Ingress/${TEAM_A}/shared-ing Service/${TEAM_B}/shared-svc"
TTL=$(served_ttl shared)
if [ "${TTL}" = "60" ]; then pass "served TTL is the lowest (60)"; else fail "served TTL was '${TTL}', expected 60"; fi
if scout_logs | grep -q "joins a DNS name"; then
    pass "Scout logged the join"
else
    fail "Scout did not log a source joining a shared name"
fi
if scout_logs | grep -q "different TTLs"; then
    pass "Scout warned about the different TTLs"
else
    fail "Scout did not warn about different TTLs"
fi
record_phase "Kinds, namespaces and TTLs share one record; lowest TTL served" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 5. A burst of sources racing on one name
# ─────────────────────────────────────────────────────────────────────────────

phase "5. ${RACE_COUNT} Services created at once on 'race', then ${RACE_KEEP} deleted at once"
P=${ERRORS}
RACE_IPS=()
for ((i = 1; i <= RACE_COUNT; i++)); do RACE_IPS+=("10.0.5.${i}"); done
for ((i = 1; i <= RACE_COUNT; i++)); do
    lb_service "${TEAM_B}" "race-${i}" race "" "10.0.5.${i}" &
done
wait
expect_a race "${RACE_IPS[@]}"
wait_for "the race record lists ${RACE_COUNT} contributors" bash -c \
    "[ \$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/managed-by=scout -o json | jq '[.items[]|select(.spec.name==\"race\")|.metadata.annotations[\"bindy.firestoned.io/scout-contributors\"]|fromjson|keys[]]|length') -eq ${RACE_COUNT} ]"
CONFLICTS=$(scout_logs | grep -c "changed since it was read" || true)
info "   Scout rebuilt ${CONFLICTS} writes after losing a race (debug level; 0 if not logged)"
DELETE_LIST=()
for ((i = RACE_KEEP + 1; i <= RACE_COUNT; i++)); do DELETE_LIST+=("race-${i}"); done
${KEDGE} delete service -n "${TEAM_B}" "${DELETE_LIST[@]}" --wait=true >/dev/null
expect_a race "${RACE_IPS[@]:0:${RACE_KEEP}}"
record_phase "Concurrent sources on one name: no address lost on create or delete" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 6. Chaos during churn
# ─────────────────────────────────────────────────────────────────────────────

phase "6. Chaos: Scout, operator and BIND9 kills during churn"
P=${ERRORS}
lb_service "${TEAM_B}" flap flap "" "${FLAP_IPS[0]}"
expect_a flap "${FLAP_IPS[0]}"
(
    for ((i = 0; i < CHURN_FLIPS; i++)); do
        lb_status "${TEAM_B}" flap "${FLAP_IPS[$(( i % 2 ))]}"
        sleep 2
    done
) &
CHURN_PID=$!
step "Kill Scout"
${KEDGE} delete pod -n "${NAMESPACE}" -l app.kubernetes.io/component=scout --wait=false >/dev/null
sleep 3
step "Kill every operator pod"
${KUBECTL} delete pod -n "${NAMESPACE}" -l app=bindy --wait=false >/dev/null
sleep 3
step "Kill a primary and the secondary"
delete_pod_of "$(primary_instances | head -1)" || true
delete_pod_of "$(secondary_instances | head -1)" || true
step "Opt a race Service out and rename another while everything restarts"
${KEDGE} annotate service race-1 -n "${TEAM_B}" bindy.firestoned.io/scout-enabled- >/dev/null
${KEDGE} annotate service race-2 -n "${TEAM_B}" bindy.firestoned.io/record-name=race2 --overwrite >/dev/null
wait "${CHURN_PID}"
# The churn loop's last write (CHURN_FLIPS even): FLAP_IPS[1].
FLAP_FINAL="${FLAP_IPS[$(( (CHURN_FLIPS - 1) % 2 ))]}"
${KEDGE} rollout status deployment/bindy-scout -n "${NAMESPACE}" --timeout=180s >/dev/null
wait_operator_ready 1 || fail "operator never came back"
wait_bind_pods_ready || fail "BIND9 pods never came back"
expect_a flap "${FLAP_FINAL}"
expect_a race "${RACE_IPS[@]:2:$(( RACE_KEEP - 2 ))}"
expect_a race2 "${RACE_IPS[1]}"
expect_a shared "${SHARED_INGRESS_IP}" "${SHARED_SERVICE_IP}"
expect_a multi "${MULTI_IPS[@]}"
expect_a www "${WWW_IP}"
expect_a "@"

step "Operator quiet window (${QUIET_WINDOW}s)"
if check_quiet; then
    pass "operator quiet after convergence"
else
    fail "operator not quiet: $(tr '\n' '|' < "${VIOLATIONS_FILE}")"
fi
step "Scout quiet window (${SCOUT_QUIET_WINDOW}s): no record rewritten"
V0=$(scout_record_versions); sleep "${SCOUT_QUIET_WINDOW}"; V1=$(scout_record_versions)
if [ "${V0}" = "${V1}" ]; then
    pass "no Scout record changed in ${SCOUT_QUIET_WINDOW}s ($(echo "${V1}" | wc -l) records)"
else
    fail "Scout records kept changing: $(diff <(echo "${V0}") <(echo "${V1}") | tr '\n' ' ')"
fi
if [ "$(scout_restarts)" -eq 0 ]; then pass "Scout container never crashed"; else fail "Scout restarted $(scout_restarts) times"; fi
if scout_logs | grep -E '(^|[[:space:]])ERROR[[:space:]]' | grep -qv "Reconcile failed.*conflict"; then
    warn "Scout logged errors: $(scout_logs | grep -E '(^|[[:space:]])ERROR[[:space:]]' | tail -3 | cut -c1-200 | tr '\n' '|')"
fi
scout_records_table
record_phase "Chaos during churn converges; operator and Scout go quiet" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# 7. Removing every source
# ─────────────────────────────────────────────────────────────────────────────

phase "7. Remove every source"
P=${ERRORS}
${KEDGE} delete services,ingresses -n "${TEAM_A}" --all --wait=true >/dev/null
${KEDGE} delete services,ingresses -n "${TEAM_B}" --all --wait=true >/dev/null
wait_for "no Scout record left on the bindy cluster" bash -c \
    "[ \$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/managed-by=scout -o name | wc -l) -eq 0 ]"
for n in www multi shared race race2 flap; do expect_a "${n}"; done
record_phase "Removing every source removes every record and name" "${P}"

finish "E2E: Scout one record per DNS name, two clusters (ADR-0026)"
