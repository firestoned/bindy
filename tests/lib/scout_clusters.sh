#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# Helpers shared by the multi-cluster Scout suites
# (tests/e2e/scout_multi_cluster_test.sh, tests/e2e/scout_guardrails_test.sh):
# a bindy cluster ("queen") with a Bind9Cluster from tests/lib/chaos.sh, and
# one or more edge clusters running Scout through `bindy bootstrap scout` and a
# `bindy bootstrap mc` account. Sourced after cluster.sh and chaos.sh, never run.
#
# The suite sets KUBECTL (the bindy cluster), KEDGE / EDGE_KUBECONFIG /
# EDGE_NAME / SCOUT_ACCOUNT (the edge being driven), ZONE (the zone names are
# relative to), STATE, BINDY_BIN, SUITE_TITLE and SCOUT_CONVERGE_TIMEOUT.
# Every DNS assertion digs every live primary and secondary pod.

# Many variables here are read by the suites, which the linter cannot follow.
# shellcheck disable=SC2034

[ -n "${_BINDY_SCOUT_CLUSTERS_SH:-}" ] && return 0
_BINDY_SCOUT_CLUSTERS_SH=1

NODE_RUNTIME="${KIND_EXPERIMENTAL_PROVIDER:-docker}"
PROBE_INTERVAL_SECS="${PROBE_INTERVAL_SECS:-1}"

tag_of() { echo "${1##*:}"; }

# Setup steps must not fail quietly: a missing policy or account would turn
# every later assertion into a test of something else.
must() {
    "$@" && return 0
    fail "setup step failed: $*"
    finish "${SUITE_TITLE}"
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
PROBE_LOG=""
PROBE_PID=""
start_prober() {
    local names=("$@") servers
    PROBE_LOG="${STATE}/probe.log"
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

# Installs (or upgrades) Scout at image $1 on the current edge with
# `bindy bootstrap scout`. SCOUT_ALLOWED_ZONES / SCOUT_ALLOWED_RANGES empty
# leave that allowlist unset (a Scout with no publishing limits).
bootstrap_scout() {
    local limits=()
    [ -n "${SCOUT_ALLOWED_ZONES:-}" ] && limits+=(--allowed-zones "${SCOUT_ALLOWED_ZONES}")
    [ -n "${SCOUT_ALLOWED_RANGES:-}" ] && limits+=(--allowed-ip-ranges "${SCOUT_ALLOWED_RANGES}")
    must env KUBECONFIG="${EDGE_KUBECONFIG}" "${BINDY_BIN}" bootstrap scout \
        --version "$(tag_of "$1")" \
        --cluster-name "${EDGE_NAME}" \
        --remote-secret "${SCOUT_ACCOUNT}-kubeconfig" \
        "${limits[@]}"
    must ${KEDGE} rollout status deployment/bindy-scout -n "${NAMESPACE}" --timeout=180s
}

record_phase() {
    summary_row "$1" "$([ "${ERRORS}" -eq "$2" ] && echo '✅ passed' || echo '❌ failed')"
}

# ─────────────────────────────────────────────────────────────────────────────
# Cluster setup
# ─────────────────────────────────────────────────────────────────────────────

# A DNSZone on the bindy cluster served by the chaos Bind9Cluster.
#
# $1 = DNSZone CR name, $2 = zone, $3 = namespaces granted (comma-separated),
# $4 = recordsFrom matchLabels as "key: value" (default: the zone label)
scout_apply_zone() {
    local cr=$1 zone=$2 grant=$3 selector=${4:-"bindy.firestoned.io/zone: $2"}
    must ${KUBECTL} apply -f - <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: ${cr}
  namespace: ${NAMESPACE}
  annotations:
    bindy.firestoned.io/allow-zone-namespaces: "${grant}"
spec:
  zoneName: ${zone}
  clusterRef: ${CHAOS_CLUSTER_CR}
  bind9InstancesFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/cluster: ${CHAOS_CLUSTER_CR}
  recordsFrom:
    - selector:
        matchLabels:
          ${selector}
  nameServerIps:
    ns1.${zone}.: 192.0.2.53
  soaRecord:
    primaryNs: ns1.${zone}.
    adminEmail: admin.${zone}.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 300
  ttl: 300
YAML
}

# The bindy cluster: kind cluster, images, operator, admission policies 21
# and 22, the chaos Bind9Cluster (primaries and a secondary) and the tools
# pods. Zones are added by the suite (scout_apply_zone).
#
# $@ = images to load
scout_setup_queen() {
    local img
    phase "Setup: bindy cluster '${QUEEN_CLUSTER}'"
    bindy_ensure_cluster deploy/kind-config-chaos.yaml
    for img in "$@"; do
        kind load docker-image "${img}" --name "${QUEEN_CLUSTER}" >/dev/null
    done
    bindy_deploy_operator
    kind get kubeconfig --name "${QUEEN_CLUSTER}" > "${QUEEN_KUBECONFIG}"
    QUEEN_IP=$(${NODE_RUNTIME} inspect -f '{{(index .NetworkSettings.Networks "kind").IPAddress}}' \
        "${QUEEN_CLUSTER}-control-plane")

    step "Installing the Scout source-binding admission policy (21, 22)"
    must ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/admission-policies/21-bindy-scout-source-binding-policy.yaml"
    must ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/admission-policies/22-bindy-scout-source-binding-binding.yaml"

    step "Bind9Cluster (primaries + secondary)"
    must apply_chaos_cluster
}

# Waits for BIND9 and starts the tools pods once the zones exist.
scout_queen_ready() {
    wait_bind_pods_ready || fail "BIND9 pods never became ready"
    apply_tools_pods
    pass "bindy cluster ready: primaries [$(live_ips_of_role primary | tr '\n' ' ')] secondaries [$(live_ips_of_role secondary | tr '\n' ' ')]"
}

# An edge cluster with the images loaded and namespaces created.
#
# $1 = kind cluster name, $2 = kubeconfig file to write, $3.. = namespaces;
# images from SCOUT_IMAGES (array)
scout_setup_edge() {
    local cluster=$1 kubeconfig=$2 img ns; shift 2
    phase "Setup: edge cluster '${cluster}'"
    if ! kind get clusters 2>/dev/null | grep -qx "${cluster}"; then
        kind create cluster --name "${cluster}" --config "${PROJECT_ROOT}/deploy/kind-scout-config.yaml"
    fi
    for img in "${SCOUT_IMAGES[@]}"; do
        kind load docker-image "${img}" --name "${cluster}" >/dev/null
    done
    kind get kubeconfig --name "${cluster}" > "${kubeconfig}"
    for ns in "${NAMESPACE}" "$@"; do
        kubectl --context "kind-${cluster}" create namespace "${ns}" --dry-run=client -o yaml \
            | kubectl --context "kind-${cluster}" apply -f - >/dev/null
    done
}

# Registers a Scout account on the bindy cluster (`bindy bootstrap mc`) and
# hands its kubeconfig Secret to the current edge (KEDGE).
#
# $1 = ServiceAccount, $2 = cluster names, $3 = zones (comma-separated)
scout_register() {
    local sa=$1 cluster_names=$2 zones=$3
    step "Registering ${sa} for clusters [${cluster_names}], zones [${zones}] (API https://${QUEEN_IP}:6443)"
    must env KUBECONFIG="${QUEEN_KUBECONFIG}" "${BINDY_BIN}" bootstrap mc \
        --service-account "${sa}" \
        --server "https://${QUEEN_IP}:6443" \
        --cluster-name "${cluster_names}" --allowed-zones "${zones}" \
        -o "${STATE}/mc-secret-${sa}.yaml"
    must ${KEDGE} apply -n "${NAMESPACE}" -f "${STATE}/mc-secret-${sa}.yaml"
    rm -f "${STATE}/mc-secret-${sa}.yaml"
}
