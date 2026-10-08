#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# Fixture, tools pods, invariant checker, DNS prober analysis and chaos steps
# for tests/e2e/chaos_test.sh. Sourced, never run.
#
# Everything here reads the cluster through ${KUBECTL} (always pinned to the
# kind context by the suite) and queries BIND9 from inside the cluster: the
# invariants dig each pod by its IP on the operand port, and rndc runs inside
# each pod's bind9 container.

# Many variables here are read by tests/e2e/chaos_test.sh, which the linter
# cannot follow from this file.
# shellcheck disable=SC2034

[ -n "${_BINDY_CHAOS_SH:-}" ] && return 0
_BINDY_CHAOS_SH=1


# shellcheck source=tests/lib/common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

# ── Fixture identity ─────────────────────────────────────────────────────────
CHAOS_CLUSTER_CR="chaos"
CHAOS_PRIMARY_REPLICAS=2
CHAOS_SECONDARY_REPLICAS=1
CHAOS_STANDALONE="chaos-standalone"
CHAOS_ZONE_CR="chaos-forward"
CHAOS_ZONE="chaos.test"
CHAOS_REVERSE_ZONE_CR="chaos-reverse"
CHAOS_REVERSE_ZONE="2.0.192.in-addr.arpa"
# The zone step 12 deletes and recreates under the other name, alternating.
CHAOS_EXTRA_ZONES=("chaos-a.test" "chaos-b.test")
CHAOS_EXTRA_ZONE_IP="192.0.2.99"
CHAOS_BINDCAR_IMAGE="${BINDCAR_IMAGE:-ghcr.io/firestoned/bindcar:v0.9.0}"
CHAOS_TOOLS_IMAGE="${CHAOS_TOOLS_IMAGE:-docker.io/internetsystemsconsortium/bind9:9.18}"
CHAOS_TOOLS_POD="chaos-tools"
CHAOS_PROBER_POD="chaos-prober"
CHAOS_DNS_PORT=5353
CHAOS_CHURN_RECORDS=5
# Env var step 10/13 add to the cluster's bindcar container to roll every
# instance through the operator, then remove again.
CHAOS_ROLL_ENV="BINDY_CHAOS_ROLL"
# How rndc is reached inside the operand container.
CHAOS_RNDC="rndc"

# ── Time budgets (seconds) ───────────────────────────────────────────────────
CONVERGE_TIMEOUT="${CONVERGE_TIMEOUT:-120}"
QUIET_WINDOW="${QUIET_WINDOW:-60}"
# A reconcile or two may legitimately land in the quiet window (a late
# Endpoints event); a hot loop produces dozens. Deployment PATCHes must be 0.
QUIET_RECONCILE_TOLERANCE="${QUIET_RECONCILE_TOLERANCE:-3}"
QUIET_DEPLOY_PATCH_TOLERANCE="${QUIET_DEPLOY_PATCH_TOLERANCE:-0}"
# How long a zone may have no answering nameserver when a step kills all of
# them at once (step 6). Measured recovery is reported either way.
ALL_DOWN_RECOVERY_BOUND="${ALL_DOWN_RECOVERY_BOUND:-120}"
# How long a Service may stop answering when its only pod is deleted, its
# container restarts, or it is the Service the step deleted. With one replica
# per instance there is no other pod behind the Service: an honest gap.
TARGET_GAP_BOUND="${TARGET_GAP_BOUND:-120}"
# Consecutive probe failures tolerated on a Service the step did not touch,
# and during a staggered rollout (old pod serves until the new one is Ready).
UNTOUCHED_GAP_TOLERANCE="${UNTOUCHED_GAP_TOLERANCE:-2}"
ROLLOUT_GAP_TOLERANCE="${ROLLOUT_GAP_TOLERANCE:-3}"
OPERATOR_DOWN_SECS="${OPERATOR_DOWN_SECS:-60}"
CHAOS_POLL=3
OPERATOR_REPLICAS=2
CHAOS_POD_TIMEOUT=300
CHAOS_ROLLOUT_TIMEOUT="${CHAOS_ROLLOUT_TIMEOUT:-480}"
EXEC_RETRIES=3
EXEC_RETRY_SLEEP=2

CHAOS_STATE_DIR="${CHAOS_STATE_DIR:-${TMPDIR:-/tmp}/bindy-chaos-$$}"
mkdir -p "${CHAOS_STATE_DIR}"

# Which zones exist right now (step 12 swaps the extra one).
CHAOS_CURRENT_EXTRA_ZONE="${CHAOS_EXTRA_ZONES[0]}"

# ── Small helpers ────────────────────────────────────────────────────────────

now() { date +%s; }

# kubectl exec with retries: an exec into a pod that is restarting a container
# fails transiently, which is a property of the step, not of the operator.
kexec() {
    local attempt out rc
    for ((attempt = 1; attempt <= EXEC_RETRIES; attempt++)); do
        rc=0
        out=$(${KUBECTL} exec -n "${NAMESPACE}" "$@" 2>/dev/null) || rc=$?
        if [ "${rc}" -eq 0 ]; then
            printf '%s' "${out}"
            return 0
        fi
        sleep "${EXEC_RETRY_SLEEP}"
    done
    printf '%s' "${out}"
    return "${rc}"
}

zone_names() {
    echo "${CHAOS_ZONE} ${CHAOS_REVERSE_ZONE} ${CHAOS_CURRENT_EXTRA_ZONE}"
}

extra_zone_cr() {
    # chaos-a.test -> chaos-extra-a
    local zone=$1
    echo "chaos-extra-${zone#chaos-}" | sed 's/\.test$//'
}

primary_instances() {
    local i
    for ((i = 0; i < CHAOS_PRIMARY_REPLICAS; i++)); do
        echo "${CHAOS_CLUSTER_CR}-primary-${i}"
    done
    echo "${CHAOS_STANDALONE}"
}

secondary_instances() {
    local i
    for ((i = 0; i < CHAOS_SECONDARY_REPLICAS; i++)); do
        echo "${CHAOS_CLUSTER_CR}-secondary-${i}"
    done
}

all_instances() { primary_instances; secondary_instances; }

# "name ip" of every live pod (has an IP, Running, not terminating) of one
# instance.
live_pods_of() {
    ${KUBECTL} get pods -n "${NAMESPACE}" -l "app.kubernetes.io/instance=$1" -o json 2>/dev/null \
        | jq -r '.items[] | select(.metadata.deletionTimestamp == null)
                 | select(.status.phase == "Running") | select(.status.podIP != null)
                 | "\(.metadata.name) \(.status.podIP)"'
}

live_ips_of_role() {
    local inst
    if [ "$1" = primary ]; then
        for inst in $(primary_instances); do live_pods_of "${inst}" | awk '{print $2}'; done
    else
        for inst in $(secondary_instances); do live_pods_of "${inst}" | awk '{print $2}'; done
    fi | sort -u
}

first_pod_of() { live_pods_of "$1" | head -1 | awk '{print $1}'; }

service_cluster_ip() {
    ${KUBECTL} get service "$1" -n "${NAMESPACE}" -o jsonpath='{.spec.clusterIP}' 2>/dev/null || true
}

# The also-notify targets every primary zone must carry, as "addr:port" lines.
# clusterip: each secondary instance's Service ClusterIP on port 53 (the
#            Service maps 53 to the operand's 5353);
# podip:     each live secondary pod IP on the operand port.
expected_also_notify() {
    local inst ip
    case "${ALSO_NOTIFY_MODE:-clusterip}" in
        podip)
            for ip in $(live_ips_of_role secondary); do echo "${ip}:${CHAOS_DNS_PORT}"; done
            ;;
        *)
            for inst in $(secondary_instances); do
                ip=$(service_cluster_ip "${inst}")
                [ -n "${ip}" ] && echo "${ip}:53"
            done
            ;;
    esac | sort -u
}

operator_pods() {
    ${KUBECTL} get pods -n "${NAMESPACE}" -l app=bindy -o json 2>/dev/null \
        | jq -r '.items[] | select(.metadata.deletionTimestamp == null) | .metadata.name'
}

# "pod restartCount" of every operator pod.
operator_restart_counts() {
    ${KUBECTL} get pods -n "${NAMESPACE}" -l app=bindy -o json 2>/dev/null \
        | jq -r '.items[] | "\(.metadata.name) \([.status.containerStatuses[]?.restartCount] | add // 0)"'
}

declare -A OPERATOR_RESTARTS_AT_STEP_START=()
remember_operator_restarts() {
    local pod count
    OPERATOR_RESTARTS_AT_STEP_START=()
    while read -r pod count; do
        [ -n "${pod}" ] && OPERATOR_RESTARTS_AT_STEP_START[${pod}]=${count}
    done <<< "$(operator_restart_counts)"
}

operator_leader() {
    ${KUBECTL} get lease bindy-leader -n "${NAMESPACE}" \
        -o jsonpath='{.spec.holderIdentity}' 2>/dev/null || true
}

wait_operator_ready() {
    local want=$1
    ${KUBECTL} rollout status deployment/bindy -n "${NAMESPACE}" \
        --timeout="${CHAOS_POD_TIMEOUT}s" >/dev/null 2>&1 || true
    local deadline=$(( $(now) + CHAOS_POD_TIMEOUT )) ready
    while [ "$(now)" -lt "${deadline}" ]; do
        ready=$(${KUBECTL} get deployment bindy -n "${NAMESPACE}" \
                    -o jsonpath='{.status.readyReplicas}' 2>/dev/null || echo 0)
        [ "${ready:-0}" -ge "${want}" ] && [ -n "$(operator_leader)" ] && return 0
        sleep "${CHAOS_POLL}"
    done
    return 1
}

# Wait until every instance has exactly its replica count of Ready,
# non-terminating pods.
wait_bind_pods_ready() {
    local deadline=$(( $(now) + ${1:-CHAOS_POD_TIMEOUT} )) inst ok n
    while [ "$(now)" -lt "${deadline}" ]; do
        ok=true
        for inst in $(all_instances); do
            n=$(${KUBECTL} get pods -n "${NAMESPACE}" -l "app.kubernetes.io/instance=${inst}" -o json 2>/dev/null \
                | jq '[.items[] | select(.metadata.deletionTimestamp == null)
                       | select(any(.status.conditions[]?; .type == "Ready" and .status == "True"))] | length')
            [ "${n:-0}" -ge 1 ] || { ok=false; break; }
        done
        ${ok} && return 0
        sleep "${CHAOS_POLL}"
    done
    return 1
}

# ── Fixture ──────────────────────────────────────────────────────────────────

apply_chaos_cluster() {
    ${KUBECTL} apply -f - >/dev/null <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: ${CHAOS_CLUSTER_CR}
  namespace: ${NAMESPACE}
  labels:
    test: chaos
spec:
  version: "9.18"
  primary:
    replicas: ${CHAOS_PRIMARY_REPLICAS}
  secondary:
    replicas: ${CHAOS_SECONDARY_REPLICAS}
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    bindcarConfig:
      image: "${CHAOS_BINDCAR_IMAGE}"
      imagePullPolicy: IfNotPresent
      logLevel: info
---
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: ${CHAOS_STANDALONE}
  namespace: ${NAMESPACE}
  labels:
    test: chaos
    bindy.firestoned.io/cluster: ${CHAOS_CLUSTER_CR}
    bindy.firestoned.io/role: primary
spec:
  clusterRef: ${CHAOS_CLUSTER_CR}
  role: primary
  replicas: 1
YAML
}

# $1 = DNSZone CR name, $2 = zone FQDN, $3 = in-zone glue (or empty)
apply_chaos_zone() {
    local cr=$1 zone=$2
    ${KUBECTL} apply -f - >/dev/null <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: ${cr}
  namespace: ${NAMESPACE}
  labels:
    test: chaos
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
          bindy.firestoned.io/zone: ${zone}
  nameServerIps:
    ns1.${CHAOS_ZONE}.: 192.0.2.53
  soaRecord:
    primaryNs: ns1.${CHAOS_ZONE}.
    adminEmail: admin.${CHAOS_ZONE}.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 300
  ttl: 300
YAML
}

apply_extra_zone() {
    local zone=$1 cr
    cr=$(extra_zone_cr "${zone}")
    apply_chaos_zone "${cr}" "${zone}"
    ${KUBECTL} apply -f - >/dev/null <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ${cr}-www
  namespace: ${NAMESPACE}
  labels:
    test: chaos
    bindy.firestoned.io/zone: ${zone}
spec:
  name: www
  ipv4Addresses:
    - "${CHAOS_EXTRA_ZONE_IP}"
  ttl: 300
YAML
}

delete_extra_zone() {
    local zone=$1 cr
    cr=$(extra_zone_cr "${zone}")
    ${KUBECTL} delete arecord "${cr}-www" -n "${NAMESPACE}" --ignore-not-found --wait=false >/dev/null 2>&1 || true
    ${KUBECTL} delete dnszone "${cr}" -n "${NAMESPACE}" --ignore-not-found --wait=false >/dev/null 2>&1 || true
}

apply_chaos_records() {
    ${KUBECTL} apply -f - >/dev/null <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata: {name: chaos-www, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: www, ipv4Addresses: ["192.0.2.10"], ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata: {name: chaos-www6, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: www, ipv6Addresses: ["2001:db8::1"], ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata: {name: chaos-blog, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: blog, target: www.${CHAOS_ZONE}., ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata: {name: chaos-mail, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: mail, ipv4Addresses: ["192.0.2.20"], ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata: {name: chaos-mx, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: "@", priority: 10, mailServer: mail.${CHAOS_ZONE}., ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata: {name: chaos-txt, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: "@", text: ["v=spf1 mx ~all"], ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata: {name: chaos-srv, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: _sip._tcp, priority: 10, weight: 60, port: 5060, target: sip.${CHAOS_ZONE}., ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata: {name: chaos-caa, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: "@", flags: 0, tag: issue, value: letsencrypt.org, ttl: 300}
---
apiVersion: bindy.firestoned.io/v1beta1
kind: PTRRecord
metadata: {name: chaos-ptr, namespace: ${NAMESPACE}, labels: {test: chaos, bindy.firestoned.io/zone: ${CHAOS_REVERSE_ZONE}}}
spec: {name: "10", target: www.${CHAOS_ZONE}., ttl: 300}
YAML
}

# The tools pod (runs the invariant digs) and the prober (queries every
# Service once a second for the whole run). Both pinned to the control plane,
# which no chaos step touches, and neither carries the operand labels.
apply_tools_pods() {
    local svcs="" inst
    # Pod specs are immutable: recreate them so a re-run picks up changes.
    ${KUBECTL} delete pod -n "${NAMESPACE}" "${CHAOS_TOOLS_POD}" "${CHAOS_PROBER_POD}" \
        --ignore-not-found --wait=true --timeout=60s >/dev/null 2>&1 || true
    for inst in $(all_instances); do svcs="${svcs} ${inst}"; done
    ${KUBECTL} apply -f - >/dev/null <<YAML
apiVersion: v1
kind: Pod
metadata:
  name: ${CHAOS_TOOLS_POD}
  namespace: ${NAMESPACE}
  labels: {test: chaos-tools}
spec:
  nodeSelector: {node-role.kubernetes.io/control-plane: ""}
  tolerations: [{operator: Exists}]
  terminationGracePeriodSeconds: 1
  containers:
    - name: tools
      image: ${CHAOS_TOOLS_IMAGE}
      imagePullPolicy: IfNotPresent
      command: ["/bin/sh", "-c", "while true; do sleep 3600; done"]
---
apiVersion: v1
kind: Pod
metadata:
  name: ${CHAOS_PROBER_POD}
  namespace: ${NAMESPACE}
  labels: {test: chaos-tools}
spec:
  nodeSelector: {node-role.kubernetes.io/control-plane: ""}
  tolerations: [{operator: Exists}]
  terminationGracePeriodSeconds: 1
  containers:
    - name: prober
      image: ${CHAOS_TOOLS_IMAGE}
      imagePullPolicy: IfNotPresent
      env:
        - {name: SERVICES, value: "${svcs# }"}
        - {name: ZONES, value: "${CHAOS_ZONE} ${CHAOS_REVERSE_ZONE}"}
        - {name: NS, value: "${NAMESPACE}"}
      command:
        - /bin/sh
        - -c
        - |
          # One line per Service per zone per second: "<epoch> <svc> <zone> OK|FAIL".
          # Service ClusterIPs are cached and re-resolved every 5 s (and after a
          # failed resolution), so a CoreDNS blip cannot fail every Service at
          # once; a deleted Service keeps failing on its old ClusterIP until
          # the operator recreates it. Every query of a second runs in
          # parallel so a dead Service cannot delay the others' samples.
          mkdir -p /tmp/ip
          while true; do
            t=\$(date +%s)
            for s in \$SERVICES; do
              if [ ! -s /tmp/ip/\$s ] || [ \$((t % 5)) -eq 0 ]; then
                ip=\$(getent hosts \$s.\$NS.svc.cluster.local | awk '{print \$1}' | head -1)
                [ -n "\$ip" ] && echo "\$ip" > /tmp/ip/\$s
              fi
            done
            for s in \$SERVICES; do
              ip=\$(cat /tmp/ip/\$s 2>/dev/null)
              for z in \$ZONES; do
                ( r=FAIL
                  if [ -n "\$ip" ]; then
                    a=\$(dig +short +time=1 +tries=1 @\$ip -p 53 SOA \$z 2>/dev/null | head -1)
                    if [ -n "\$a" ] && [ "\${a#;}" = "\$a" ]; then r=OK; fi
                  fi
                  echo "\$t \$s \$z \$r" ) &
              done
            done
            wait
            while [ "\$(date +%s)" -le "\$t" ]; do sleep 0.1; done
          done
YAML
    ${KUBECTL} wait --for=condition=ready pod -n "${NAMESPACE}" \
        "${CHAOS_TOOLS_POD}" "${CHAOS_PROBER_POD}" --timeout="${CHAOS_POD_TIMEOUT}s" >/dev/null
}

# ── Record churn state ───────────────────────────────────────────────────────
# One line per churn record that should exist: "<index> <ipv4>". Records not
# listed must not exist (in Kubernetes or in DNS).
CHURN_STATE_FILE="${CHAOS_STATE_DIR}/churn-state"
: > "${CHURN_STATE_FILE}"
# Zones that were deleted and must be gone from every pod.
DELETED_ZONES_FILE="${CHAOS_STATE_DIR}/deleted-zones"
: > "${DELETED_ZONES_FILE}"

churn_apply() {
    local idx=$1 ip=$2
    # A record whose deletion is still pending (its finalizer waits until every
    # live pod has confirmed the delete, ADR-0015 decision 7) would accept the
    # apply as a patch to the terminating object and then disappear, leaving
    # the expected state wrong. Report failure so the caller skips it.
    if churn_terminating "${idx}"; then
        return 1
    fi
    ${KUBECTL} apply -f - >/dev/null 2>&1 <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata: {name: chaos-churn-${idx}, namespace: ${NAMESPACE}, labels: {test: chaos, chaos-churn: "true", bindy.firestoned.io/zone: ${CHAOS_ZONE}}}
spec: {name: churn-${idx}, ipv4Addresses: ["${ip}"], ttl: 60}
YAML
}

churn_delete() {
    ${KUBECTL} delete arecord "chaos-churn-$1" -n "${NAMESPACE}" --ignore-not-found --wait=false >/dev/null 2>&1
}

# True when the churn record's deletion is still pending (deletionTimestamp set).
churn_terminating() {
    local ts
    ts=$(${KUBECTL} get arecord "chaos-churn-$1" -n "${NAMESPACE}" \
        -o jsonpath='{.metadata.deletionTimestamp}' 2>/dev/null)
    [ -n "${ts}" ]
}

# ── Expected DNS answers ─────────────────────────────────────────────────────
# "TYPE|QNAME|ANSWERS" where ANSWERS is dig +short output, sorted, each line
# followed by ';'. An empty ANSWERS means the name must not resolve.
expected_answers() {
    local z=${CHAOS_ZONE} idx ip i present
    cat <<ROWS
A|www.${z}.|192.0.2.10;
AAAA|www.${z}.|2001:db8::1;
CNAME|blog.${z}.|www.${z}.;
A|mail.${z}.|192.0.2.20;
MX|${z}.|10 mail.${z}.;
TXT|${z}.|"v=spf1 mx ~all";
SRV|_sip._tcp.${z}.|10 60 5060 sip.${z}.;
CAA|${z}.|0 issue "letsencrypt.org";
PTR|10.${CHAOS_REVERSE_ZONE}.|www.${z}.;
A|www.${CHAOS_CURRENT_EXTRA_ZONE}.|${CHAOS_EXTRA_ZONE_IP};
ROWS
    for ((i = 0; i < CHAOS_CHURN_RECORDS; i++)); do
        present=$(awk -v i="${i}" '$1 == i {print $2}' "${CHURN_STATE_FILE}")
        echo "A|churn-${i}.${z}.|${present:+${present};}"
    done
}

# ── Invariant checker ────────────────────────────────────────────────────────
VIOLATIONS_FILE="${CHAOS_STATE_DIR}/violations"
violation() { echo "$*" >> "${VIOLATIONS_FILE}"; }

# Run every DNS query against every pod in one exec of the tools pod.
# stdin of the tools pod: lines "<ip> <TYPE> <QNAME>"; output lines
# "<ip>|<TYPE>|<QNAME>|<answers sorted, ';'-terminated>".
dig_all() {
    ${KUBECTL} exec -i -n "${NAMESPACE}" "${CHAOS_TOOLS_POD}" -- sh -c '
        while read -r ip t q; do
          ( a=$(dig +short +time=2 +tries=2 @"$ip" -p '"${CHAOS_DNS_PORT}"' "$t" "$q" 2>/dev/null \
                | grep -v "^;" | sort | tr "\n" ";")
            echo "$ip|$t|$q|$a" ) &
        done
        wait' 2>/dev/null
}

# One exec per pod: rndc zonestatus + showzone for every zone named.
# Output blocks: "@@ <zone>", status lines, "@@rc <rc>", "@@show <showzone>".
rndc_report() {
    local pod=$1; shift
    kexec "${pod}" -c bind9 -- sh -c '
        for z in "$@"; do
          echo "@@ $z"
          out=$('"${CHAOS_RNDC}"' zonestatus "$z" 2>&1); rc=$?
          echo "$out" | sed "s/^/   /"
          echo "@@rc $rc"
          echo "@@show $('"${CHAOS_RNDC}"' showzone "$z" 2>&1 | tr "\n" " ")"
        done' sh "$@"
}

# Addresses inside "<keyword> { ... };" of a showzone line, as "addr:port"
# (port 53 when none is given). allow-transfer drops any /32 suffix.
showzone_list() {
    local line=$1 keyword=$2
    echo "${line}" | sed -n "s/.*${keyword} *{\([^}]*\)}.*/\1/p" | tr ';' '\n' \
        | sed 's/^ *//; s/ *$//' | grep -v '^$' \
        | awk '{ addr=$1; sub(/\/[0-9]+$/, "", addr); port=53; if ($2 == "port") port=$3; print addr ":" port }' \
        | sort -u
}

check_dns() {
    local expected inst pod ip zone queries results row t q want got
    expected=$(expected_answers)
    queries=""
    for inst in $(all_instances); do
        while read -r pod ip; do
            [ -n "${ip}" ] || continue
            while IFS='|' read -r t q want; do
                queries+="${ip} ${t} ${q}"$'\n'
            done <<< "${expected}"
            for zone in $(zone_names); do queries+="${ip} SOA ${zone}."$'\n'; done
        done <<< "$(live_pods_of "${inst}")"
    done
    results=$(printf '%s' "${queries}" | dig_all) || { violation "dns: tools pod exec failed"; return; }
    echo "${results}" > "${CHAOS_STATE_DIR}/dig-results"

    # Exact record sets on every pod.
    for inst in $(all_instances); do
        while read -r pod ip; do
            [ -n "${ip}" ] || continue
            while IFS='|' read -r t q want; do
                got=$(awk -F'|' -v ip="${ip}" -v t="${t}" -v q="${q}" \
                      '$1 == ip && $2 == t && $3 == q {print $4; exit}' <<< "${results}")
                if [ "${got}" != "${want}" ]; then
                    violation "dns: ${inst} (${ip}) ${t} ${q}: got '${got}' want '${want}'"
                fi
            done <<< "${expected}"
        done <<< "$(live_pods_of "${inst}")"
    done

    # SOA serials: every pod serves the zone; each secondary's serial equals
    # the serial of at least one primary.
    local serials_p serial_s
    for zone in $(zone_names); do
        serials_p=""
        for inst in $(primary_instances); do
            while read -r pod ip; do
                [ -n "${ip}" ] || continue
                got=$(awk -F'|' -v ip="${ip}" -v q="${zone}." '$1 == ip && $2 == "SOA" && $3 == q {print $4; exit}' <<< "${results}" | awk '{print $3}')
                if [ -z "${got}" ]; then
                    violation "soa: primary ${inst} (${ip}) does not serve ${zone}"
                fi
                serials_p+=" ${got}"
            done <<< "$(live_pods_of "${inst}")"
        done
        for inst in $(secondary_instances); do
            while read -r pod ip; do
                [ -n "${ip}" ] || continue
                serial_s=$(awk -F'|' -v ip="${ip}" -v q="${zone}." '$1 == ip && $2 == "SOA" && $3 == q {print $4; exit}' <<< "${results}" | awk '{print $3}')
                if [ -z "${serial_s}" ]; then
                    violation "soa: secondary ${inst} (${ip}) does not serve ${zone}"
                elif ! grep -qw -- "${serial_s}" <<< "${serials_p}"; then
                    violation "soa: secondary ${inst} serial ${serial_s} for ${zone} matches no primary (primaries:${serials_p})"
                fi
            done <<< "$(live_pods_of "${inst}")"
        done
    done
}

check_rndc() {
    local primary_ips secondary_ips notify_want inst pod ip report zone block rc show deleted
    primary_ips=$(for ip in $(live_ips_of_role primary); do echo "${ip}:${CHAOS_DNS_PORT}"; done | sort -u)
    secondary_ips=$(for ip in $(live_ips_of_role secondary); do echo "${ip}:53"; done | sort -u)
    notify_want=$(expected_also_notify)
    deleted=$(sort -u "${DELETED_ZONES_FILE}" | grep -vxF "${CHAOS_CURRENT_EXTRA_ZONE}" || true)
    for inst in $(all_instances); do
        while read -r pod ip; do
            [ -n "${pod}" ] || continue
            # shellcheck disable=SC2046
            report=$(rndc_report "${pod}" $(zone_names) ${deleted}) || {
                violation "rndc: exec into ${inst} (${pod}) failed"; continue; }
            for zone in $(zone_names); do
                block=$(awk -v z="${zone}" '$1 == "@@" {on = ($2 == z)} on' <<< "${report}")
                rc=$(sed -n 's/^@@rc //p' <<< "${block}")
                show=$(sed -n 's/^@@show //p' <<< "${block}")
                if [ "${rc}" != 0 ] || ! grep -q 'serial:' <<< "${block}"; then
                    violation "rndc: ${inst} (${ip}) zonestatus ${zone} not loaded: $(grep -v '^@@' <<< "${block}" | tr -s ' \n' ' ' | cut -c1-160)"
                    continue
                fi
                if [[ " $(primary_instances | tr '\n' ' ') " == *" ${inst} "* ]]; then
                    local at an
                    at=$(showzone_list "${show}" "allow-transfer")
                    an=$(showzone_list "${show}" "also-notify")
                    [ "${at}" = "${secondary_ips}" ] || violation "acl: ${inst} (${ip}) ${zone} allow-transfer [$(echo ${at})] want [$(echo ${secondary_ips})]"
                    [ "${an}" = "${notify_want}" ] || violation "notify: ${inst} (${ip}) ${zone} also-notify [$(echo ${an})] want [$(echo ${notify_want})]"
                else
                    local pr
                    pr=$(showzone_list "${show}" "primaries")
                    [ "${pr}" = "${primary_ips}" ] || violation "primaries: ${inst} (${ip}) ${zone} primaries [$(echo ${pr})] want [$(echo ${primary_ips})]"
                fi
            done
            for zone in ${deleted}; do
                block=$(awk -v z="${zone}" '$1 == "@@" {on = ($2 == z)} on' <<< "${report}")
                grep -q 'serial:' <<< "${block}" \
                    && violation "rndc: deleted zone ${zone} still on ${inst} (${ip})"
            done
        done <<< "$(live_pods_of "${inst}")"
    done
}

# Ready / Degraded / RolloutQueued over every chaos CR.
check_status() {
    local json
    json=$(${KUBECTL} get dnszones,bind9instances,bind9clusters,arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,srvrecords,caarecords,ptrrecords \
               -n "${NAMESPACE}" -l test=chaos -o json 2>/dev/null) || { violation "status: list failed"; return; }
    echo "${json}" > "${CHAOS_STATE_DIR}/status.json"
    jq -r '.items[] | select(.metadata.deletionTimestamp == null)
        | . as $o
        | ([.status.conditions[]? | select(.type == "Ready")][0]) as $r
        | if ($r == null) then "status: \($o.kind)/\($o.metadata.name) has no Ready condition"
          elif $r.status != "True" then "status: \($o.kind)/\($o.metadata.name) Ready=\($r.status) \($r.reason): \($r.message | .[0:160])"
          else empty end,
          ( .status.conditions[]? | select(.reason == "RolloutQueued")
            | "status: \($o.kind)/\($o.metadata.name) still RolloutQueued: \(.message | .[0:120])" ),
          ( .status.conditions[]? | select(.type == "Degraded" and .status == "True")
            | "status: \($o.kind)/\($o.metadata.name) Degraded \(.reason): \(.message | .[0:160])" )' \
        <<< "${json}" >> "${VIOLATIONS_FILE}"
    # Expected objects all present.
    local zcount
    zcount=$(jq '[.items[] | select(.kind == "DNSZone" and .metadata.deletionTimestamp == null)] | length' <<< "${json}")
    [ "${zcount}" = 3 ] || violation "status: ${zcount} DNSZones, want 3"
    # Churn CRs exist exactly as the state says.
    local want_churn got_churn
    want_churn=$(awk '{print "chaos-churn-" $1}' "${CHURN_STATE_FILE}" | sort | tr '\n' ' ')
    got_churn=$(jq -r '.items[] | select(.kind == "ARecord") | .metadata.name | select(startswith("chaos-churn-"))' <<< "${json}" | sort | tr '\n' ' ')
    [ "${want_churn}" = "${got_churn}" ] || violation "status: churn CRs [${got_churn}] want [${want_churn}]"
}

check_pods() {
    local inst n
    for inst in $(all_instances); do
        n=$(${KUBECTL} get pods -n "${NAMESPACE}" -l "app.kubernetes.io/instance=${inst}" -o json 2>/dev/null \
            | jq '[.items[] | select(.metadata.deletionTimestamp == null)] as $p
                  | "\($p | length) \([$p[] | select(any(.status.conditions[]?; .type == "Ready" and .status == "True"))] | length)"' -r)
        [ "${n}" = "1 1" ] || violation "pods: ${inst} live/ready pods ${n}, want 1 1"
    done
    for inst in $(all_instances); do
        ${KUBECTL} get service "${inst}" -n "${NAMESPACE}" >/dev/null 2>&1 || violation "drift: Service ${inst} missing"
    done
    ${KUBECTL} get configmap "${CHAOS_CLUSTER_CR}-config" -n "${NAMESPACE}" >/dev/null 2>&1 \
        || violation "drift: ConfigMap ${CHAOS_CLUSTER_CR}-config missing"
    local ready
    ready=$(${KUBECTL} get deployment bindy -n "${NAMESPACE}" -o jsonpath='{.status.readyReplicas}' 2>/dev/null)
    [ "${ready:-0}" = "${OPERATOR_REPLICAS}" ] || violation "operator: ${ready:-0}/${OPERATOR_REPLICAS} replicas ready"
    local leader
    leader=$(operator_leader)
    grep -qxF -- "${leader:-none}" <<< "$(operator_pods)" \
        || violation "operator: lease holder '${leader}' is not a live operator pod"
    # Operator container restarts since the step began (a killed pod is
    # replaced by a new pod, which starts at 0; a restart is a crash).
    local pod count
    while read -r pod count; do
        [ -n "${pod}" ] || continue
        if [ "${count}" -gt "${OPERATOR_RESTARTS_AT_STEP_START[${pod}]:-0}" ]; then
            violation "operator: ${pod} container restarted (${OPERATOR_RESTARTS_AT_STEP_START[${pod}]:-0} -> ${count}); see 'kubectl logs --previous'"
        fi
    done <<< "$(operator_restart_counts)"
}

# One full pass of every invariant. Returns 0 when nothing is violated.
check_invariants_once() {
    : > "${VIOLATIONS_FILE}"
    check_pods
    check_status
    check_dns
    check_rndc
    [ ! -s "${VIOLATIONS_FILE}" ]
}

# Poll until every invariant holds, up to CONVERGE_TIMEOUT from $1 (epoch the
# step's chaos finished). Sets CONVERGED_SECS (or -1) and leaves the last
# violations in VIOLATIONS_FILE.
CONVERGED_SECS=-1
wait_converged() {
    local from=$1 deadline
    deadline=$(( from + CONVERGE_TIMEOUT ))
    CONVERGED_SECS=-1
    while true; do
        if check_invariants_once; then
            CONVERGED_SECS=$(( $(now) - from ))
            return 0
        fi
        [ "$(now)" -ge "${deadline}" ] && return 1
        sleep "${CHAOS_POLL}"
    done
}

# ── DNS prober analysis ──────────────────────────────────────────────────────
# Prints "svc <name> <max consecutive failed seconds>" for every Service and
# "zone <name> <max consecutive seconds with no answering Service>" for every
# probed zone, over prober samples in [$1, $2].
analyze_probe() {
    local from=$1 to=$2
    ${KUBECTL} logs -n "${NAMESPACE}" "${CHAOS_PROBER_POD}" --since-time="$(date -u -d "@$((from - 5))" +%Y-%m-%dT%H:%M:%SZ)" 2>/dev/null \
    | awk -v from="${from}" -v to="${to}" '
        NF == 4 && $1 >= from && $1 <= to {
            t = $1; s = $2; z = $3; r = $4
            secs[t] = 1; svcs[s] = 1; zones[z] = 1
            if (r != "OK") sfail[t, s] = 1; else zok[t, z] = 1
            seen[t, s] = 1; zseen[t, z] = 1
        }
        END {
            n = 0; for (t in secs) ts[++n] = t
            # insertion sort of epochs
            for (i = 2; i <= n; i++) { v = ts[i]; j = i - 1; while (j > 0 && ts[j] > v) { ts[j+1] = ts[j]; j-- } ts[j+1] = v }
            for (s in svcs) {
                run = 0; best = 0
                for (i = 1; i <= n; i++) { t = ts[i]; if (!((t, s) in seen)) continue
                    if ((t, s) in sfail) { run++; if (run > best) best = run } else run = 0 }
                print "svc", s, best
            }
            for (z in zones) {
                run = 0; best = 0
                for (i = 1; i <= n; i++) { t = ts[i]; if (!((t, z) in zseen)) continue
                    if (!((t, z) in zok)) { run++; if (run > best) best = run } else run = 0 }
                print "zone", z, best
            }
            print "samples", n
        }' | sort
}

# ── Quiet window ─────────────────────────────────────────────────────────────

operator_metric_sum() {
    local pod=$1 pattern=$2
    ${KUBECTL} get --raw "/api/v1/namespaces/${NAMESPACE}/pods/${pod}:8080/proxy/metrics" 2>/dev/null \
        | awk -v p="${pattern}" '$0 !~ /^#/ && $0 ~ p {s += $NF} END {printf "%d\n", s}'
}

# Per-kind reconcile counters of one operator pod, kept so a noisy quiet
# window can be attributed to a controller: quiet-<pod>-<label>.
quiet_snapshot() {
    ${KUBECTL} get --raw "/api/v1/namespaces/${NAMESPACE}/pods/$1:8080/proxy/metrics" 2>/dev/null \
        | grep -E '^bindy_firestoned_io_reconciliations_total' > "${CHAOS_STATE_DIR}/quiet-$1-$2" || true
}

# After convergence the operator must go quiet: no reconcile growth beyond a
# small tolerance, no Deployment PATCHes, no ERROR lines and no WARN repeated
# three times or more in the window. Appends to VIOLATIONS_FILE.
check_quiet() {
    local pods pod r0 r1 p0 p1 errs reps
    : > "${VIOLATIONS_FILE}"
    pods=$(operator_pods)
    declare -A rstart pstart
    for pod in ${pods}; do
        quiet_snapshot "${pod}" before
        rstart[${pod}]=$(operator_metric_sum "${pod}" '^bindy_firestoned_io_reconciliations_total')
        pstart[${pod}]=$(operator_metric_sum "${pod}" '^bindy_firestoned_io_kube_api_requests_total.*resource="deployments".*verb="patch"')
    done
    sleep "${QUIET_WINDOW}"
    for pod in ${pods}; do
        if ! ${KUBECTL} get pod "${pod}" -n "${NAMESPACE}" >/dev/null 2>&1; then
            violation "quiet: operator pod ${pod} vanished during the quiet window"
            continue
        fi
        quiet_snapshot "${pod}" after
        r1=$(operator_metric_sum "${pod}" '^bindy_firestoned_io_reconciliations_total')
        p1=$(operator_metric_sum "${pod}" '^bindy_firestoned_io_kube_api_requests_total.*resource="deployments".*verb="patch"')
        r0=${rstart[${pod}]}; p0=${pstart[${pod}]}
        QUIET_RECONCILES=$(( ${QUIET_RECONCILES:-0} + r1 - r0 ))
        [ $(( r1 - r0 )) -le "${QUIET_RECONCILE_TOLERANCE}" ] \
            || violation "quiet: ${pod} ran $(( r1 - r0 )) reconciles in a ${QUIET_WINDOW}s quiet window (hot loop?)"
        [ $(( p1 - p0 )) -le "${QUIET_DEPLOY_PATCH_TOLERANCE}" ] \
            || violation "quiet: ${pod} patched Deployments $(( p1 - p0 )) times in the quiet window"
        local logs
        logs=$(${KUBECTL} logs -n "${NAMESPACE}" "${pod}" --since="${QUIET_WINDOW}s" 2>/dev/null \
               | sed -E 's/\x1b\[[0-9;]*m//g')
        errs=$(grep -E '(^|[[:space:]])ERROR[[:space:]]' <<< "${logs}" | head -3 || true)
        [ -z "${errs}" ] || violation "quiet: ${pod} logged ERROR in the quiet window: $(echo "${errs}" | cut -c1-240 | tr '\n' '|')"
        reps=$(grep -E '(^|[[:space:]])WARN[[:space:]]' <<< "${logs}" \
               | sed -E 's/^[^ ]+ +//; s/[0-9]+(\.[0-9]+)?(ms|s)\b//g' | sort | uniq -c | awk '$1 >= 3' | head -3 || true)
        [ -z "${reps}" ] || violation "quiet: ${pod} repeated WARN lines: $(echo "${reps}" | cut -c1-240 | tr '\n' '|')"
    done
    [ ! -s "${VIOLATIONS_FILE}" ]
}

# ── Chaos primitives ─────────────────────────────────────────────────────────

NODE_RUNTIME="${KIND_EXPERIMENTAL_PROVIDER:-docker}"

delete_pod_of() {
    local pod
    pod=$(first_pod_of "$1")
    [ -n "${pod}" ] || { warn "no live pod for $1"; return 0; }
    ${KUBECTL} delete pod "${pod}" -n "${NAMESPACE}" --wait=false >/dev/null 2>&1 || true
    echo "    deleted pod ${pod} ($1)"
}

# Kill one container of an instance's pod by stopping it on its kind node
# (crictl stop with no grace): the kubelet restarts it in the same pod, same
# IP. Works for images without a shell or kill (bindcar).
kill_container_of() {
    local inst=$1 container=$2 pod json node cid before after deadline
    pod=$(first_pod_of "${inst}")
    [ -n "${pod}" ] || { warn "no live pod for ${inst}"; return 0; }
    json=$(${KUBECTL} get pod "${pod}" -n "${NAMESPACE}" -o json)
    node=$(jq -r '.spec.nodeName' <<< "${json}")
    cid=$(jq -r --arg c "${container}" '.status.containerStatuses[] | select(.name == $c) | .containerID | sub("^[a-z]+://"; "")' <<< "${json}")
    before=$(jq -r --arg c "${container}" '.status.containerStatuses[] | select(.name == $c) | .restartCount' <<< "${json}")
    ${NODE_RUNTIME} exec "${node}" crictl stop --timeout 0 "${cid}" >/dev/null 2>&1 \
        || warn "crictl stop ${container} in ${pod} failed"
    deadline=$(( $(now) + 60 ))
    while [ "$(now)" -lt "${deadline}" ]; do
        after=$(${KUBECTL} get pod "${pod}" -n "${NAMESPACE}" -o json 2>/dev/null \
                | jq -r --arg c "${container}" '.status.containerStatuses[] | select(.name == $c) | .restartCount')
        [ "${after:-0}" -gt "${before:-0}" ] && { echo "    killed ${container} in ${pod} (restarts ${before} -> ${after})"; return 0; }
        sleep 1
    done
    violation "chaos: ${container} in ${pod} did not restart after being killed"
}

# Deployments of the chaos instances that are mid-rollout right now.
rolling_deployments() {
    local names
    names=$(all_instances | tr '\n' ' ')
    # shellcheck disable=SC2086
    ${KUBECTL} get deployment ${names} -n "${NAMESPACE}" -o json 2>/dev/null \
        | jq -r '.items[] | select(
              (.status.observedGeneration // 0) < .metadata.generation
              or (.status.updatedReplicas // 0) < .spec.replicas
              or (.status.replicas // 0) > .spec.replicas
              or (.status.unavailableReplicas // 0) > 0) | .metadata.name'
}

# How many chaos instance Deployments carry the roll env var ($1 = present|absent).
roll_env_count() {
    local names
    names=$(all_instances | tr '\n' ' ')
    # shellcheck disable=SC2086
    ${KUBECTL} get deployment ${names} -n "${NAMESPACE}" -o json 2>/dev/null \
        | jq --arg e "${CHAOS_ROLL_ENV}" '[.items[] | select(any(.spec.template.spec.containers[] | select(.name == "api") | .env[]?; .name == $e))] | length'
}

set_roll_env() {
    if [ "$1" = add ]; then
        ${KUBECTL} patch bind9cluster "${CHAOS_CLUSTER_CR}" -n "${NAMESPACE}" --type=json \
            -p "[{\"op\":\"add\",\"path\":\"/spec/global/bindcarConfig/envVars\",\"value\":[{\"name\":\"${CHAOS_ROLL_ENV}\",\"value\":\"$(now)\"}]}]" >/dev/null
    else
        ${KUBECTL} patch bind9cluster "${CHAOS_CLUSTER_CR}" -n "${NAMESPACE}" --type=json \
            -p '[{"op":"remove","path":"/spec/global/bindcarConfig/envVars"}]' >/dev/null
    fi
}

# Background sampler: the most chaos Deployments seen rolling at once, written
# to $1 until $2 disappears.
stagger_sampler() {
    local out=$1 flag=$2 n max=0
    while [ -e "${flag}" ]; do
        n=$(rolling_deployments | grep -c . || true)
        [ "${n}" -gt "${max}" ] && max=${n} && echo "${max}" > "${out}"
        sleep 1
    done
}

# Roll every instance via the cluster spec ($1 = add|remove) and wait until
# every Deployment carries the change and none is rolling. $2 (optional) is a
# hook run once the first Deployment has started rolling (step 13 kills the
# operator there). Records stagger violations.
roll_cluster() {
    local action=$1 hook=${2:-} flag="${CHAOS_STATE_DIR}/sampling" max_file="${CHAOS_STATE_DIR}/max-rolling"
    local want deadline started=false total
    total=$(all_instances | grep -c .)
    [ "${action}" = add ] && want=${total} || want=0
    echo 0 > "${max_file}"; touch "${flag}"
    stagger_sampler "${max_file}" "${flag}" &
    local sampler=$!
    set_roll_env "${action}"
    deadline=$(( $(now) + CHAOS_ROLLOUT_TIMEOUT ))
    while [ "$(now)" -lt "${deadline}" ]; do
        if ! ${started} && [ -n "$(rolling_deployments)" ]; then
            started=true
            if [ -n "${hook}" ]; then "${hook}"; fi
        fi
        if [ "$(roll_env_count)" = "${want}" ] && [ -z "$(rolling_deployments)" ]; then
            break
        fi
        sleep 2
    done
    rm -f "${flag}"; wait "${sampler}" 2>/dev/null || true
    if [ "$(roll_env_count)" != "${want}" ] || [ -n "$(rolling_deployments)" ]; then
        STEP_NOTES+=("rollout (${action}) did not finish within ${CHAOS_ROLLOUT_TIMEOUT}s: $(roll_env_count)/${total} carry the change, rolling: $(rolling_deployments | tr '\n' ' ')")
        STEP_FAILED=true
    fi
    local max
    max=$(cat "${max_file}")
    echo "    rollout (${action}): at most ${max} instance(s) rolling at once"
    if [ "${max}" -gt 1 ]; then
        STEP_NOTES+=("staggering broken: ${max} instances of one cluster rolled at once (ADR-0018)")
        STEP_FAILED=true
    fi
}

# ── Record churn (step 11) ───────────────────────────────────────────────────

churn_loop() {
    local flag=$1 seed=$2 i ip op
    RANDOM=${seed}
    while [ -e "${flag}" ]; do
        i=$(( RANDOM % CHAOS_CHURN_RECORDS ))
        ip="198.51.100.$(( RANDOM % 250 + 1 ))"
        if grep -q "^${i} " "${CHURN_STATE_FILE}"; then
            op=$(( RANDOM % 2 ))
        else
            op=2
        fi
        case ${op} in
            0)  churn_delete "${i}" && sed -i "/^${i} /d" "${CHURN_STATE_FILE}" ;;
            *)  churn_apply "${i}" "${ip}" && { sed -i "/^${i} /d" "${CHURN_STATE_FILE}"; echo "${i} ${ip}" >> "${CHURN_STATE_FILE}"; } ;;
        esac
        sleep 2
    done
}

# ── The chaos steps ──────────────────────────────────────────────────────────
# Each sets STEP_TARGETS (Services allowed a bounded gap), STEP_ROLLOUT
# (rollout tolerance on every Service) or STEP_ALL_DOWN (every nameserver is
# killed on purpose), runs its chaos, and returns once the chaos is done.

CHAOS_STEP_IDS=(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
declare -A CHAOS_STEP_NAMES=(
    [1]="delete leader operator pod"
    [2]="delete both operator pods"
    [3]="operator down 60s, Service+ConfigMap deleted"
    [4]="delete one primary pod"
    [5]="delete the secondary pod"
    [6]="delete all BIND9 pods"
    [7]="delete a primary and the secondary"
    [8]="kill named (primary + secondary)"
    [9]="kill bindcar (primary + secondary)"
    [10]="cluster config change rolls every instance"
    [11]="record churn during pod chaos"
    [12]="zone churn while a pod is down"
    [13]="kill the operator mid-rollout"
    [14]="pod rescheduled to another node"
    [15]="delete records while a primary's bindcar, then named, is down"
)

# Sets PICKED to one of the cluster's primaries. Called in the suite's own
# shell (never in $(...)) so the seeded RANDOM sequence, and so the run, is
# reproducible from CHAOS_SEED.
PICKED=""
next_primary() {
    PICKED="${CHAOS_CLUSTER_CR}-primary-$(( RANDOM % CHAOS_PRIMARY_REPLICAS ))"
}
SECONDARY="${CHAOS_CLUSTER_CR}-secondary-0"

step_1() {
    local leader
    leader=$(operator_leader)
    ${KUBECTL} delete pod "${leader}" -n "${NAMESPACE}" --wait=false >/dev/null 2>&1 || true
    echo "    deleted leader ${leader}"
}

step_2() {
    ${KUBECTL} delete pod -n "${NAMESPACE}" -l app=bindy --wait=false >/dev/null 2>&1 || true
    echo "    deleted every operator pod"
}

step_3() {
    local t0 left
    t0=$(now)
    STEP_TARGETS="${SECONDARY}"
    ${KUBECTL} scale deployment/bindy -n "${NAMESPACE}" --replicas=0 >/dev/null
    ${KUBECTL} wait --for=delete pod -n "${NAMESPACE}" -l app=bindy --timeout=120s >/dev/null 2>&1 || true
    ${KUBECTL} delete service "${SECONDARY}" -n "${NAMESPACE}" --wait=true >/dev/null 2>&1 || true
    ${KUBECTL} delete configmap "${CHAOS_CLUSTER_CR}-config" -n "${NAMESPACE}" --wait=true >/dev/null 2>&1 || true
    echo "    operator scaled to 0; deleted Service ${SECONDARY} and ConfigMap ${CHAOS_CLUSTER_CR}-config"
    left=$(( OPERATOR_DOWN_SECS - ($(now) - t0) ))
    [ "${left}" -gt 0 ] && sleep "${left}"
    ${KUBECTL} scale deployment/bindy -n "${NAMESPACE}" --replicas="${OPERATOR_REPLICAS}" >/dev/null
    echo "    operator scaled back to ${OPERATOR_REPLICAS}"
}

step_4() { local p; next_primary; p=${PICKED}; STEP_TARGETS="${p}"; delete_pod_of "${p}"; }

step_5() { STEP_TARGETS="${SECONDARY}"; delete_pod_of "${SECONDARY}"; }

step_6() {
    STEP_ALL_DOWN=true
    ${KUBECTL} delete pod -n "${NAMESPACE}" -l app=bind9 --wait=false >/dev/null 2>&1 || true
    echo "    deleted every BIND9 pod"
}

step_7() {
    local p; next_primary; p=${PICKED}
    STEP_TARGETS="${p} ${SECONDARY}"
    delete_pod_of "${p}"; delete_pod_of "${SECONDARY}"
}

step_8() {
    local p; next_primary; p=${PICKED}
    STEP_TARGETS="${p} ${SECONDARY}"
    kill_container_of "${p}" bind9 & kill_container_of "${SECONDARY}" bind9 & wait
}

step_9() {
    local p; next_primary; p=${PICKED}
    STEP_TARGETS="${p} ${SECONDARY}"
    kill_container_of "${p}" api & kill_container_of "${SECONDARY}" api & wait
}

step_10() {
    STEP_ROLLOUT=true
    roll_cluster add
    roll_cluster remove
}

step_11() {
    local flag="${CHAOS_STATE_DIR}/churning" p
    STEP_TARGETS="$(all_instances | tr '\n' ' ')"
    touch "${flag}"
    churn_loop "${flag}" "${RANDOM}" &
    local churn=$!
    next_primary; p=${PICKED}
    delete_pod_of "${p}"; sleep 20
    delete_pod_of "${SECONDARY}"; sleep 20
    next_primary; p=${PICKED}
    delete_pod_of "${p}"; delete_pod_of "${SECONDARY}"; sleep 30
    wait_bind_pods_ready 120 || true
    next_primary; kill_container_of "${PICKED}" bind9; kill_container_of "${SECONDARY}" bind9; sleep 20
    next_primary; kill_container_of "${PICKED}" api; kill_container_of "${SECONDARY}" api; sleep 10
    rm -f "${flag}"; wait "${churn}" 2>/dev/null || true
    echo "    churn stopped; expected churn records: $(awk '{printf "%s=%s ", $1, $2}' "${CHURN_STATE_FILE}")"
}

step_12() {
    local old=${CHAOS_CURRENT_EXTRA_ZONE} new
    if [ "${old}" = "${CHAOS_EXTRA_ZONES[0]}" ]; then new=${CHAOS_EXTRA_ZONES[1]}; else new=${CHAOS_EXTRA_ZONES[0]}; fi
    STEP_TARGETS="${SECONDARY}"
    delete_pod_of "${SECONDARY}"
    delete_extra_zone "${old}"
    apply_extra_zone "${new}"
    echo "${old}" >> "${DELETED_ZONES_FILE}"
    CHAOS_CURRENT_EXTRA_ZONE=${new}
    echo "    deleted zone ${old}, created zone ${new}, while ${SECONDARY} is down"
}

# Wait until the pod of instance $1 reports ContainersReady=False (a killed
# container noticed by the kubelet), up to 20 s.
wait_containers_not_ready() {
    local pod deadline
    pod=$(first_pod_of "$1")
    deadline=$(( $(now) + 20 ))
    while [ "$(now)" -lt "${deadline}" ]; do
        if [ "$(${KUBECTL} get pod "${pod}" -n "${NAMESPACE}" \
                  -o jsonpath='{.status.conditions[?(@.type=="ContainersReady")].status}' 2>/dev/null)" = False ]; then
            return 0
        fi
        sleep 0.5
    done
    warn "pod ${pod} never reported ContainersReady=False"
    return 1
}

# Poll until every invariant holds (up to CONVERGE_TIMEOUT); 0 when it did.
wait_until_converged() {
    local deadline=$(( $(now) + CONVERGE_TIMEOUT ))
    while [ "$(now)" -lt "${deadline}" ]; do
        check_invariants_once && return 0
        sleep "${CHAOS_POLL}"
    done
    return 1
}

kill_leader_hook() {
    local leader; leader=$(operator_leader)
    ${KUBECTL} delete pod "${leader}" -n "${NAMESPACE}" --wait=false >/dev/null 2>&1 || true
    echo "    rollout started: deleted leader ${leader}"
}

kill_all_operators_hook() {
    ${KUBECTL} delete pod -n "${NAMESPACE}" -l app=bindy --wait=false >/dev/null 2>&1 || true
    echo "    rollout started: deleted every operator pod"
}

step_13() {
    STEP_ROLLOUT=true
    roll_cluster add kill_leader_hook
    roll_cluster remove kill_all_operators_hook
}

step_14() {
    local pod node newnode deadline
    STEP_TARGETS="${SECONDARY}"
    pod=$(first_pod_of "${SECONDARY}")
    node=$(${KUBECTL} get pod "${pod}" -n "${NAMESPACE}" -o jsonpath='{.spec.nodeName}')
    ${KUBECTL} cordon "${node}" >/dev/null
    ${KUBECTL} delete pod "${pod}" -n "${NAMESPACE}" --wait=false >/dev/null
    deadline=$(( $(now) + 120 ))
    newnode=""
    while [ "$(now)" -lt "${deadline}" ]; do
        newnode=$(${KUBECTL} get pods -n "${NAMESPACE}" -l "app.kubernetes.io/instance=${SECONDARY}" -o json \
                  | jq -r --arg p "${pod}" '.items[] | select(.metadata.name != $p) | .spec.nodeName // empty' | head -1)
        [ -n "${newnode}" ] && break
        sleep 1
    done
    ${KUBECTL} uncordon "${node}" >/dev/null
    echo "    ${SECONDARY}: ${node} -> ${newnode:-<not scheduled>}"
    if [ -z "${newnode}" ] || [ "${newnode}" = "${node}" ]; then
        STEP_NOTES+=("pod did not move off cordoned node ${node} (now on '${newnode}')")
        STEP_FAILED=true
    fi
}

# Deterministic regression for an orphaned record: a primary's pod keeps its
# zones on an emptyDir while one of its containers restarts, but it is not a
# writable endpoint meanwhile. A record deleted in that window must still
# disappear from the pod (and from the secondary, by transfer) once the
# container is back; the operator used to skip the pod, drop its finalizer
# and serve the record forever. Two records, one deleted while bindcar is
# down, one while named is down, each only after the pod reports
# ContainersReady=False.
CHAOS_ORPHAN_IDX_API=3
CHAOS_ORPHAN_IDX_NAMED=4
step_15() {
    local p idx container
    next_primary; p=${PICKED}
    STEP_TARGETS="${p} ${SECONDARY}"
    for idx in ${CHAOS_ORPHAN_IDX_API} ${CHAOS_ORPHAN_IDX_NAMED}; do
        # Step 11 may have left this name mid-deletion; this step needs the
        # record to exist, so let that deletion finish first.
        local waited=0
        while churn_terminating "${idx}" && [ "${waited}" -lt "${CONVERGE_TIMEOUT}" ]; do
            sleep 2; waited=$(( waited + 2 ))
        done
        churn_apply "${idx}" "198.51.100.$(( 200 + idx ))" \
            || fail "chaos-churn-${idx} still terminating after ${CONVERGE_TIMEOUT}s; cannot set up step 15"
        sed -i "/^${idx} /d" "${CHURN_STATE_FILE}"
        echo "${idx} 198.51.100.$(( 200 + idx ))" >> "${CHURN_STATE_FILE}"
    done
    if ! wait_until_converged; then
        STEP_NOTES+=("records to delete were never served everywhere before the chaos")
        STEP_FAILED=true
        return 0
    fi
    for container in api bind9; do
        if [ "${container}" = api ]; then idx=${CHAOS_ORPHAN_IDX_API}; else idx=${CHAOS_ORPHAN_IDX_NAMED}; fi
        kill_container_of "${p}" "${container}" &
        local killer=$!
        wait_containers_not_ready "${p}" || true
        churn_delete "${idx}"
        sed -i "/^${idx} /d" "${CHURN_STATE_FILE}"
        echo "    deleted ARecord chaos-churn-${idx} while ${container} of ${p} was down"
        wait "${killer}" 2>/dev/null || true
        wait_bind_pods_ready 120 || true
    done
}

