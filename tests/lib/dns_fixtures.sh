#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# The zone/record fixture set shared by the tests/e2e/ DNS suites, plus the
# assertions that check it. Sourced, never run.
#
# One fixture set, three suites: lifecycle_test.sh proves it comes up and is
# actually served, idempotency_test.sh proves re-applying it changes nothing,
# restart_test.sh proves it survives an operator and an operand restart. They
# all need the same manifests and the same "is BIND9 really serving this?"
# check, so it lives here exactly once.

[ -n "${_BINDY_DNS_FIXTURES_SH:-}" ] && return 0
_BINDY_DNS_FIXTURES_SH=1

# shellcheck source=tests/lib/common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

# ── Fixture identity ─────────────────────────────────────────────────────────
# Names are constants so the pre-clean, the apply, the verification and the
# teardown all agree on them: a re-run must be able to find and remove exactly
# what the previous run left.
CLUSTER_CR="integration-test-cluster"
STANDALONE_PRIMARY="integration-test-primary"
ZONE_CR="integration-test-zone"
REVERSE_ZONE_CR="integration-test-reverse-zone"
ZONE_FQDN="integration.test"
REVERSE_ZONE_FQDN="0.168.192.in-addr.arpa"
BIND9_CONTAINER="bind9"
BINDCAR_IMAGE="${BINDCAR_IMAGE:-ghcr.io/firestoned/bindcar:v0.8.0}"

# The operand serves DNS on an unprivileged port so the Pod can drop
# NET_BIND_SERVICE; querying :53 gets connection refused. Matches
# tests/regression_test.sh's EXPECTED_DNS_PORT.
BIND9_DNS_PORT=5353

# Primaries created by the Bind9Cluster itself. A Bind9Cluster with
# primary.replicas: N does not create one N-Pod Deployment — each primary is an
# individually addressable nameserver, so the cluster controller creates N
# separate Bind9Instances. Two of them plus the standalone instance gives the
# suites three primaries to check every zone and record against.
PRIMARY_REPLICAS=2

RECORD_KINDS="arecords,aaaarecords,cnamerecords,mxrecords,txtrecords,nsrecords,srvrecords,caarecords,ptrrecords"
RECORD_NAMES=(integration-a integration-aaaa integration-cname integration-mx \
              integration-mail integration-txt integration-ns integration-srv \
              integration-caa integration-ptr)
# type:name pairs for the "the CR exists" check.
RECORD_TYPES=(arecord:integration-a aaaarecord:integration-aaaa \
              cnamerecord:integration-cname arecord:integration-mail \
              mxrecord:integration-mx txtrecord:integration-txt \
              nsrecord:integration-ns srvrecord:integration-srv \
              caarecord:integration-caa ptrrecord:integration-ptr)
EXPECTED_ZONE_COUNT=2
EXPECTED_RECORD_COUNT=10

# ── Time budgets ─────────────────────────────────────────────────────────────
# Restarting an operand is not just a Pod restart: the new Pod comes up with an
# empty zone directory and the operator has to push every zone and record back
# into it, so these are deliberately generous.
PRECLEAN_TIMEOUT=180
# A DNSZone finalizer has to reach every primary over the bindcar API, so it
# needs its own budget before the instances are torn down.
ZONE_DELETE_TIMEOUT=120
OPERAND_READY_TIMEOUT=300
# shellcheck disable=SC2034  # used by tests/e2e/restart_test.sh
OPERATOR_ROLLOUT_TIMEOUT=300
POD_APPEAR_TIMEOUT=180
# Measured: after every operand Pod is deleted, the operator takes about 100s to
# replay the zones and records into the replacements. 45 x 5s leaves real margin
# over that on a loaded machine.
DNS_SETTLE_RETRIES=45
DNS_SETTLE_SLEEP=5
POLL_INTERVAL=3
# Small pauses between dependent applies: the Bind9Cluster has to exist before
# the instance references it, and the zones before the records land in them.
APPLY_SETTLE_SECS=2
ZONE_SETTLE_SECS=3
RECONCILE_SETTLE_SECS=10

# What every primary must answer once the zones and records have reconciled, as
# "TYPE|QNAME|EXPECTED SUBSTRING" triples. This is the real assertion: the CRs
# existing in Kubernetes proves nothing about BIND9 actually serving them, and
# after a Pod wipe it is the only thing that distinguishes a recovered server
# from an empty one.
EXPECTED_DNS=(
    "SOA|${ZONE_FQDN}.|ns1.integration.test."
    "A|www.${ZONE_FQDN}.|192.0.2.10"
    "AAAA|www.${ZONE_FQDN}.|2001:db8::1"
    "CNAME|blog.${ZONE_FQDN}.|www.integration.test."
    "A|mail.${ZONE_FQDN}.|192.0.2.20"
    "MX|${ZONE_FQDN}.|mail.integration.test."
    "TXT|${ZONE_FQDN}.|v=spf1 mx ~all"
    "NS|${ZONE_FQDN}.|ns2.integration.test."
    "SRV|_sip._tcp.${ZONE_FQDN}.|sip.integration.test."
    "CAA|${ZONE_FQDN}.|letsencrypt.org"
    "PTR|10.${REVERSE_ZONE_FQDN}.|www.integration.test."
)

# ── Discovery helpers ────────────────────────────────────────────────────────

# Every Bind9Instance that must exist once the cluster has reconciled: the ones
# the Bind9Cluster derives from primary.replicas, plus the standalone instance.
expected_primaries() {
    local i
    for ((i = 0; i < PRIMARY_REPLICAS; i++)); do
        echo "${CLUSTER_CR}-primary-${i}"
    done
    echo "${STANDALONE_PRIMARY}"
}

# Name of the Running Pod backing one Bind9Instance; empty when there is none.
instance_pod() {
    ${KUBECTL} get pods -n "${NAMESPACE}" \
        -l "app.kubernetes.io/instance=$1" \
        --field-selector=status.phase=Running \
        -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true
}

# ── Teardown ─────────────────────────────────────────────────────────────────

# Delete everything the fixture creates. Safe to call when nothing exists, which
# is what makes a re-run on a dirty cluster behave like a run on a clean one.
#
# Order matters. A DNSZone's finalizer removes the zone from each primary over
# the bindcar API, so the zones have to be gone before the instances that serve
# them are torn down -- delete the instances first and the finalizer has no
# endpoint left to talk to and wedges forever. Deletes are --wait=false and the
# waiting is bounded here, because kubectl's own blocking delete has no timeout.
delete_test_resources() {
    local name
    ${KUBECTL} delete ${RECORD_KINDS} -l test=integration -n "${NAMESPACE}" \
        --ignore-not-found=true --wait=false >/dev/null 2>&1 || true
    for name in "${RECORD_NAMES[@]}"; do
        ${KUBECTL} delete ${RECORD_KINDS} "${name}" -n "${NAMESPACE}" \
            --ignore-not-found=true --wait=false >/dev/null 2>&1 || true
    done

    ${KUBECTL} delete dnszone "${ZONE_CR}" "${REVERSE_ZONE_CR}" -n "${NAMESPACE}" \
        --ignore-not-found=true --wait=false >/dev/null 2>&1 || true
    wait_for_zones_gone || force_clear_zone_finalizers

    ${KUBECTL} delete bind9instance "${STANDALONE_PRIMARY}" -n "${NAMESPACE}" \
        --ignore-not-found=true --wait=false >/dev/null 2>&1 || true
    ${KUBECTL} delete bind9cluster "${CLUSTER_CR}" -n "${NAMESPACE}" \
        --ignore-not-found=true --wait=false >/dev/null 2>&1 || true
}

# Bounded wait for both DNSZones to finish finalizing.
wait_for_zones_gone() {
    local deadline=$((SECONDS + ZONE_DELETE_TIMEOUT)) left
    while [ "${SECONDS}" -lt "${deadline}" ]; do
        left=$(${KUBECTL} get dnszones -n "${NAMESPACE}" --no-headers 2>/dev/null \
                   | grep -c integration-test || true)
        if [ "${left:-0}" -eq 0 ]; then
            return 0
        fi
        sleep "${POLL_INTERVAL}"
    done
    return 1
}

# Escape hatch for a DNSZone whose finalizer cannot complete -- see the operator
# bug where a zone that never got created in BIND9 can never be deleted, because
# the deletion path freezes the zone first and freeze 500s when it is absent.
# Without this a single wedged zone makes every later run fail, so the suites
# would stop being idempotent. Logged loudly: it is a workaround, not a fix, and
# the operator still needs one.
force_clear_zone_finalizers() {
    local zone
    for zone in "${ZONE_CR}" "${REVERSE_ZONE_CR}"; do
        if ${KUBECTL} get dnszone "${zone}" -n "${NAMESPACE}" >/dev/null 2>&1; then
            warn "DNSZone ${zone} is wedged in Terminating; clearing its finalizer so the"
            echo -e "      suite stays re-runnable. This is an operator bug, not a test bug --"
            echo -e "      a zone whose create failed cannot be deleted."
            ${KUBECTL} patch dnszone "${zone}" -n "${NAMESPACE}" --type=merge \
                -p '{"metadata":{"finalizers":[]}}' >/dev/null 2>&1 || true
        fi
    done
}

# Wait for the operator's finalizers to actually release everything, so the run
# starts from a clean slate instead of racing a half-deleted zone.
#
# Deployments and Pods are in the list deliberately: the suites all reuse the
# same fixture names, and the CRs vanishing only means the finalizers ran — the
# operand Deployments/ReplicaSets/Pods they own are garbage-collected and
# terminated asynchronously after that. A suite that re-applies the fixtures
# one second later races the operator's fresh Deployment against the GC of the
# previous suite's identically-named one, and its readiness wait against the
# old Pod's preStop drain. Waiting for the operands to be gone removes that
# overlap entirely.
wait_for_test_resources_gone() {
    local deadline=$((SECONDS + PRECLEAN_TIMEOUT)) left
    while [ "${SECONDS}" -lt "${deadline}" ]; do
        left=$(${KUBECTL} get ${RECORD_KINDS},dnszones,bind9instances,bind9clusters,deployments,pods \
                   -n "${NAMESPACE}" --no-headers 2>/dev/null | grep -c integration || true)
        if [ "${left:-0}" -eq 0 ]; then
            return 0
        fi
        sleep "${POLL_INTERVAL}"
    done
    return 1
}

# Leftovers from an interrupted previous run would make every count wrong, so
# every suite starts here.
preclean_fixtures() {
    step "Pre-clean: removing anything a previous run left behind"
    delete_test_resources
    if wait_for_test_resources_gone; then
        pass "namespace is clean"
    else
        warn "some resources are still terminating; continuing anyway"
    fi
}

# Teardown to run at the end of a suite, so a run always ends where the next one
# expects to start.
teardown_fixtures() {
    step "Removing test resources"
    delete_test_resources
    wait_for_test_resources_gone || true
}

# ── Assertions ───────────────────────────────────────────────────────────────
# These record failures through fail() and always return 0, so callers can chain
# them without `set -e` aborting on the first problem.

# Everything the CI log needs to explain a missed readiness deadline: which
# container was unready and why, what the kubelet was doing to the Pod, and the
# namespace's recent events. Without this, a failure only says "never became
# ready" about a Pod that is often 2/2 Running by the time anyone looks.
dump_operand_diagnostics() {
    local inst=$1 pod
    warn "diagnostics for ${inst}:"
    ${KUBECTL} get pods -n "${NAMESPACE}" \
        -l "app.kubernetes.io/instance=${inst}" -o wide 2>/dev/null || true
    # Not instance_pod(): that filters to phase=Running, and the whole point
    # here is a Pod that may be stuck in ContainerCreating or Pending.
    pod=$(${KUBECTL} get pods -n "${NAMESPACE}" \
        -l "app.kubernetes.io/instance=${inst}" \
        -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)
    if [ -n "${pod}" ]; then
        echo "── container statuses (${pod}) ──"
        ${KUBECTL} get pod "${pod}" -n "${NAMESPACE}" -o jsonpath='{range .status.containerStatuses[*]}{.name}{" ready="}{.ready}{" restarts="}{.restartCount}{" state="}{.state}{"\n"}{end}' 2>/dev/null || true
        echo "── pod conditions ──"
        ${KUBECTL} get pod "${pod}" -n "${NAMESPACE}" -o jsonpath='{range .status.conditions[*]}{.type}{"="}{.status}{" reason="}{.reason}{" message="}{.message}{"\n"}{end}' 2>/dev/null || true
        echo "── describe (tail) ──"
        ${KUBECTL} describe pod "${pod}" -n "${NAMESPACE}" 2>/dev/null | tail -30 || true
    fi
    echo "── namespace events (most recent last) ──"
    ${KUBECTL} get events -n "${NAMESPACE}" --sort-by=.lastTimestamp 2>/dev/null | tail -25 || true
}

# Block until every expected primary has a Ready Pod. kubectl wait fails
# immediately when nothing matches its selector, so wait for the Pod to be
# created first, then for it to become Ready.
assert_operands_ready() {
    local inst deadline
    for inst in $(expected_primaries); do
        deadline=$((SECONDS + POD_APPEAR_TIMEOUT))
        while [ -z "$(instance_pod "${inst}")" ] && [ "${SECONDS}" -lt "${deadline}" ]; do
            sleep "${POLL_INTERVAL}"
        done
        if ${KUBECTL} wait --for=condition=ready pod \
               -l "app.kubernetes.io/instance=${inst}" -n "${NAMESPACE}" \
               --timeout="${OPERAND_READY_TIMEOUT}s" >/dev/null 2>&1; then
            pass "${inst}: Pod ready"
        else
            fail "${inst}: Pod never became ready"
            dump_operand_diagnostics "${inst}"
        fi
    done
    return 0
}

# Query one operand Pod's own BIND9 over the loopback. Failures echo nothing so
# callers can simply retry until the answer settles.
dig_in_pod() {
    local pod=$1 rrtype=$2 qname=$3
    ${KUBECTL} exec -n "${NAMESPACE}" "${pod}" -c "${BIND9_CONTAINER}" -- \
        dig @127.0.0.1 -p "${BIND9_DNS_PORT}" +short +time=3 +tries=1 "${rrtype}" "${qname}" 2>/dev/null || true
}

# Poll every primary until it serves the whole expected record set.
assert_dns_on_primaries() {
    local label=$1
    local inst pod attempt entry rrtype qname want got missing

    for inst in $(expected_primaries); do
        pod=$(instance_pod "${inst}")
        if [ -z "${pod}" ]; then
            fail "${label}: no running Pod for ${inst}"
            continue
        fi

        missing=""
        for ((attempt = 1; attempt <= DNS_SETTLE_RETRIES; attempt++)); do
            missing=""
            for entry in "${EXPECTED_DNS[@]}"; do
                IFS='|' read -r rrtype qname want <<< "${entry}"
                got=$(dig_in_pod "${pod}" "${rrtype}" "${qname}")
                case "${got}" in
                    *"${want}"*) ;;
                    *) missing="${missing} ${rrtype}/${qname}" ;;
                esac
            done
            if [ -z "${missing}" ]; then
                break
            fi
            sleep "${DNS_SETTLE_SLEEP}"
        done

        if [ -z "${missing}" ]; then
            pass "${label}: ${inst} serves all ${#EXPECTED_DNS[@]} expected records"
        else
            fail "${label}: ${inst} missing:${missing}"
        fi
    done
    return 0
}

# Re-applying the same manifests must not create a second copy of anything, and
# a restart must not lose one. Counts are matched on the shared "integration"
# name prefix so unrelated resources in the namespace are ignored.
assert_resource_counts() {
    local label=$1
    local want_instances got_instances got_zones got_records

    want_instances=$(expected_primaries | wc -l | tr -d ' ')
    got_instances=$(${KUBECTL} get bind9instances -n "${NAMESPACE}" -o name 2>/dev/null \
                        | grep -c integration-test || true)
    got_zones=$(${KUBECTL} get dnszones -n "${NAMESPACE}" -o name 2>/dev/null \
                    | grep -c integration-test || true)
    got_records=$(${KUBECTL} get ${RECORD_KINDS} -n "${NAMESPACE}" -o name 2>/dev/null \
                      | grep -c integration- || true)

    if [ "${got_instances:-0}" -eq "${want_instances}" ]; then
        pass "${label}: ${got_instances} Bind9Instances (expected ${want_instances})"
    else
        fail "${label}: ${got_instances} Bind9Instances, expected ${want_instances}"
    fi

    if [ "${got_zones:-0}" -eq "${EXPECTED_ZONE_COUNT}" ]; then
        pass "${label}: ${got_zones} DNSZones (expected ${EXPECTED_ZONE_COUNT})"
    else
        fail "${label}: ${got_zones} DNSZones, expected ${EXPECTED_ZONE_COUNT}"
    fi

    if [ "${got_records:-0}" -eq "${EXPECTED_RECORD_COUNT}" ]; then
        pass "${label}: ${got_records} record CRs (expected ${EXPECTED_RECORD_COUNT})"
    else
        fail "${label}: ${got_records} record CRs, expected ${EXPECTED_RECORD_COUNT}"
    fi
    return 0
}

# Every CR the fixture creates must exist in the API.
assert_fixture_crs_exist() {
    local entry type name
    for entry in "bind9cluster:${CLUSTER_CR}" "bind9instance:${STANDALONE_PRIMARY}" \
                 "dnszone:${ZONE_CR}" "dnszone:${REVERSE_ZONE_CR}" "${RECORD_TYPES[@]}"; do
        IFS=':' read -r type name <<< "${entry}"
        if ${KUBECTL} get "${type}" "${name}" -n "${NAMESPACE}" >/dev/null 2>&1; then
            pass "${type}/${name} created"
        else
            fail "${type}/${name} not found"
        fi
    done
    return 0
}

# The full "is the fixture healthy right now?" check, used as a baseline by
# every suite and re-run after each perturbation.
assert_fixture_healthy() {
    local label=$1
    assert_operands_ready
    assert_dns_on_primaries "${label}"
    assert_resource_counts "${label}"
}

# ── Diagnostics ──────────────────────────────────────────────────────────────

dump_fixture_status() {
    echo ""
    info "📋 Resource status:"
    ${KUBECTL} get bind9clusters,bind9instances -n "${NAMESPACE}" 2>/dev/null || true
    ${KUBECTL} get dnszones -n "${NAMESPACE}" 2>/dev/null || true
    ${KUBECTL} get ${RECORD_KINDS} -n "${NAMESPACE}" 2>/dev/null || true
}

# ── The fixture itself ───────────────────────────────────────────────────────

# Everything the DNS suites create, in one re-runnable block. It is called more
# than once on purpose: applying the identical spec again must change nothing.
apply_test_manifests() {
    step "Applying Bind9Cluster (${PRIMARY_REPLICAS} primaries)"
    ${KUBECTL} apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Cluster
metadata:
  name: ${CLUSTER_CR}
  namespace: ${NAMESPACE}
  labels:
    test: integration
spec:
  version: "9.18"
  primary:
    replicas: ${PRIMARY_REPLICAS}
  global:
    recursion: false
    allowQuery:
      - "0.0.0.0/0"
    bindcarConfig:
      image: "${BINDCAR_IMAGE}"
      imagePullPolicy: IfNotPresent
      logLevel: debug
EOF
    sleep "${APPLY_SETTLE_SECS}"

    step "Applying standalone Bind9Instance"
    ${KUBECTL} apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: Bind9Instance
metadata:
  name: ${STANDALONE_PRIMARY}
  namespace: ${NAMESPACE}
  labels:
    test: integration
    role: primary
    # Same labels the Bind9Cluster puts on the primaries it creates, so the
    # zones' bind9InstancesFrom selector below matches all three primaries.
    bindy.firestoned.io/cluster: ${CLUSTER_CR}
    bindy.firestoned.io/role: primary
spec:
  clusterRef: ${CLUSTER_CR}
  role: primary
  replicas: 1
  bindcarConfig:
    image: "${BINDCAR_IMAGE}"
    imagePullPolicy: IfNotPresent
    logLevel: debug
EOF
    sleep "${APPLY_SETTLE_SECS}"

    step "Applying forward DNSZone (${ZONE_FQDN})"
    ${KUBECTL} apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: ${ZONE_CR}
  namespace: ${NAMESPACE}
spec:
  zoneName: ${ZONE_FQDN}
  clusterRef: ${CLUSTER_CR}
  # clusterRef alone is not enough: get_instances_from_zone() in
  # src/reconcilers/dnszone/validation.rs selects instances *only* through
  # bind9InstancesFrom and fails the zone outright when it is missing.
  bind9InstancesFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/cluster: ${CLUSTER_CR}
  recordsFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/zone: ${ZONE_FQDN}
  nameServerIps:
    ns1.example.com.: 192.168.0.60
    # The SOA primaryNs below is in-zone, so BIND needs a glue A record for it.
    # Without this, the generated zone fails to load and rndc addzone returns
    # "bad zone" — the reverse zone escapes this because ns1.integration.test.
    # is out-of-zone there.
    ns1.integration.test.: 192.0.2.53
    # The NSRecord below publishes ns2.integration.test. as a nameserver for
    # this zone. BIND runs a post-update name server sanity check on every
    # DDNS transaction and rejects it when an in-zone NS target has no
    # address, which took the batched MX update down with it:
    #   update rejected: post update name server sanity check failed
    ns2.integration.test.: 192.0.2.54
  soaRecord:
    primaryNs: ns1.integration.test.
    adminEmail: admin.integration.test.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
EOF
    sleep "${ZONE_SETTLE_SECS}"

    step "Applying reverse DNSZone (${REVERSE_ZONE_FQDN})"
    ${KUBECTL} apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: ${REVERSE_ZONE_CR}
  namespace: ${NAMESPACE}
spec:
  zoneName: ${REVERSE_ZONE_FQDN}
  clusterRef: ${CLUSTER_CR}
  bind9InstancesFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/cluster: ${CLUSTER_CR}
  recordsFrom:
    - selector:
        matchLabels:
          bindy.firestoned.io/zone: ${REVERSE_ZONE_FQDN}
  nameServerIps:
    ns1.example.com.: 192.168.0.60
  soaRecord:
    primaryNs: ns1.integration.test.
    adminEmail: admin.integration.test.
    serial: 2024010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
  ttl: 3600
EOF
    sleep "${ZONE_SETTLE_SECS}"

    step "Applying all nine record types"
    ${KUBECTL} apply -f - <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: integration-a
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: www
  ipv4Addresses:
    - "192.0.2.10"
  ttl: 300
---
apiVersion: bindy.firestoned.io/v1beta1
kind: AAAARecord
metadata:
  name: integration-aaaa
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: www
  ipv6Addresses:
    - "2001:db8::1"
  ttl: 300
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CNAMERecord
metadata:
  name: integration-cname
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: blog
  target: www.integration.test.
  ttl: 300
---
# Address record for the MX target. BIND refuses a dynamic MX update whose
# exchange lies inside the zone and has no A/AAAA record ("has no address
# records (A or AAAA)" -> REFUSED), so without this the MX below never lands.
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: integration-mail
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: mail
  ipv4Addresses:
    - "192.0.2.20"
  ttl: 300
---
apiVersion: bindy.firestoned.io/v1beta1
kind: MXRecord
metadata:
  name: integration-mx
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: "@"
  priority: 10
  mailServer: mail.integration.test.
  ttl: 3600
---
apiVersion: bindy.firestoned.io/v1beta1
kind: TXTRecord
metadata:
  name: integration-txt
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: "@"
  text:
    - "v=spf1 mx ~all"
  ttl: 3600
---
apiVersion: bindy.firestoned.io/v1beta1
kind: NSRecord
metadata:
  name: integration-ns
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: "@"
  nameserver: ns2.integration.test.
  ttl: 3600
---
apiVersion: bindy.firestoned.io/v1beta1
kind: SRVRecord
metadata:
  name: integration-srv
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: _sip._tcp
  priority: 10
  weight: 60
  port: 5060
  target: sip.integration.test.
  ttl: 3600
---
apiVersion: bindy.firestoned.io/v1beta1
kind: CAARecord
metadata:
  name: integration-caa
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: "@"
  flags: 0
  tag: issue
  value: letsencrypt.org
  ttl: 3600
---
apiVersion: bindy.firestoned.io/v1beta1
kind: PTRRecord
metadata:
  name: integration-ptr
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${REVERSE_ZONE_FQDN}
spec:
  name: "10"
  target: www.integration.test.
  ttl: 300
EOF

    step "Waiting ${RECONCILE_SETTLE_SECS}s for reconciliation"
    sleep "${RECONCILE_SETTLE_SECS}"
}
