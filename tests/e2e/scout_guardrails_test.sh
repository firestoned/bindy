#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# E2E suite: Scout records bound to their cluster and zones (ADR-0025),
# proven with two edge clusters against one bindy cluster (roadmap 31
# phase 5).
#
#   bindy cluster (QUEEN_CLUSTER): operator, primaries + secondary, admission
#     policies 21 and 22; zones foo.test, bar.test (with a loose
#     `managed-by: scout` selector) and shared.test, each granting team-a.
#   edge foo (FOO_CLUSTER): Scout --cluster-name foo, account scout-foo
#     registered for [foo] / [foo.test, shared.test], and NO Scout-side
#     allowlist: it stands in for a misconfigured or compromised Scout, so
#     only the bindy cluster's admission stands between it and bar.test.
#   edge bar (BAR_CLUSTER): Scout --cluster-name bar, account scout-bar
#     registered for [bar] / [bar.test, shared.test], with --allowed-zones
#     and --allowed-ip-ranges.
#
# Both edges have a namespace team-a and a Service team-a/web: namespace
# names repeat across clusters, which is the vector ADR-0025 closes.
#
#   G1. each cluster publishes into its own zone; same-named sources in two
#       zones never clean each other up (#474);
#   G2. a tenant on foo targeting bar.test is refused at admission, and
#       bar.test is untouched (Decision 4);
#   G3. bar's own allowlists refuse another zone and an outside address
#       before any write (Decisions 1 and 2);
#   G4. foo's stolen token cannot create, rewrite or delete bar's records or
#       stamp bar's cluster name; it can still write its own zone (Decision 4);
#   G5. a loose DNSZone selector does not take foo's records into bar.test
#       (Decision 5);
#   G6. a refused withdrawal from a record left under another cluster name is
#       skipped and does not hold the source's finalizer (Decision 6);
#   G7. a zone registered for both clusters: the first cluster to publish a
#       name keeps it, the other's record is held back (RRsetConflict), a
#       primary wipe's replay keeps the holder, and the name passes to the
#       other cluster when the holder leaves (ADR-0027);
#   G8. a cluster renamed with both names registered: no conflict, the new
#       name takes the records over (ADR-0027 Decision 1).
#
# Usage: tests/e2e/scout_guardrails_test.sh --image ghcr.io/firestoned/bindy:<build>

# Settings below are read by tests/lib/scout_clusters.sh.
# shellcheck disable=SC2034
set -uo pipefail

QUEEN_CLUSTER="${QUEEN_CLUSTER:-bindy-e2e-queen}"
FOO_CLUSTER="${FOO_CLUSTER:-bindy-e2e-edge}"
BAR_CLUSTER="${BAR_CLUSTER:-bindy-e2e-edge-b}"
CLUSTER_NAME="${QUEEN_CLUSTER}"
KUBECTL="kubectl --context kind-${QUEEN_CLUSTER}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"
source "${LIB_DIR}/chaos.sh"
source "${LIB_DIR}/scout_clusters.sh"

parse_common_args "$@"

[ -n "${IMAGE_REF}" ] || { echo "--image is required" >&2; exit 2; }
BINDY_BIN="${BINDY_BIN:-${PROJECT_ROOT}/target/x86_64-unknown-linux-gnu/debug/bindy}"
SCOUT_CONVERGE_TIMEOUT="${SCOUT_CONVERGE_TIMEOUT:-240}"
# How long a refused source is given to (not) produce a record.
REFUSAL_WINDOW="${REFUSAL_WINDOW:-45}"
SUITE_TITLE="E2E: Scout records bound to their cluster and zones, two edge clusters (ADR-0025)"

FOO_ZONE="foo.test"
BAR_ZONE="bar.test"
SHARED_ZONE="shared.test"
TEAM="team-a"
FOO_WEB_IP="10.10.1.1"
BAR_WEB_IP="10.20.1.1"
FOO_OWN_IP="10.10.4.4"
EVIL_IP="10.66.6.6"
OUTSIDE_IP="172.16.0.9"
FOO_SHARED_IP="10.10.7.7"
BAR_SHARED_IP="10.20.7.7"
STALE_CLUSTER="foo-old"

STATE="${CHAOS_STATE_DIR}"
QUEEN_KUBECONFIG="${STATE}/queen.kubeconfig"
FOO_KUBECONFIG="${STATE}/foo.kubeconfig"
BAR_KUBECONFIG="${STATE}/bar.kubeconfig"
VIOLATIONS_FILE="${STATE}/violations"
: > "${VIOLATIONS_FILE}"
SCOUT_IMAGES=("${IMAGE_REF}")

info "🧪 ${SUITE_TITLE}"
info "   bindy '${QUEEN_CLUSTER}', edges '${FOO_CLUSTER}' (foo) and '${BAR_CLUSTER}' (bar), image ${IMAGE_REF}"

# Points the edge helpers at one edge cluster.
use_edge() {
    case "$1" in
        foo)
            KEDGE="kubectl --context kind-${FOO_CLUSTER}"
            EDGE_KUBECONFIG="${FOO_KUBECONFIG}"
            EDGE_NAME="foo"
            SCOUT_ACCOUNT="scout-foo"
            SCOUT_ALLOWED_ZONES=""
            SCOUT_ALLOWED_RANGES=""
            ;;
        bar)
            KEDGE="kubectl --context kind-${BAR_CLUSTER}"
            EDGE_KUBECONFIG="${BAR_KUBECONFIG}"
            EDGE_NAME="bar"
            SCOUT_ACCOUNT="scout-bar"
            SCOUT_ALLOWED_ZONES="${BAR_ZONE},${SHARED_ZONE}"
            SCOUT_ALLOWED_RANGES="10.0.0.0/8"
            ;;
    esac
}

# Name of the Scout record cluster $1 wrote for DNS name $3 in zone $2.
record_of() {
    scout_records_json | jq -r --arg c "$1" --arg z "$2" --arg n "$3" \
        '.items[] | select(.metadata.labels["bindy.firestoned.io/source-cluster"] == $c
                           and .metadata.labels["bindy.firestoned.io/zone"] == $z
                           and .spec.name == $n) | .metadata.name' | head -1
}

# Scout records cluster $1 has in zone $2.
records_in() {
    ${KUBECTL} get arecords -n "${NAMESPACE}" \
        -l "bindy.firestoned.io/managed-by=scout,bindy.firestoned.io/source-cluster=$1,bindy.firestoned.io/zone=$2" \
        -o name 2>/dev/null | wc -l
}

has_record() { [ -n "$(record_of "$1" "$2" "$3")" ]; }

# Fails unless "$@" fails and its stderr names the source-binding policy.
expect_refused() {
    local what=$1; shift
    local err
    if err=$("$@" 2>&1 >/dev/null); then
        fail "${what}: the bindy cluster accepted it"
        return 1
    fi
    if grep -qE "bindy-scout-source-binding|denied the request" <<< "${err}"; then
        pass "${what}: refused ($(grep -oE "denied (the )?request: .{0,140}" <<< "${err}" | head -1))"
        return 0
    fi
    fail "${what}: failed, but not by the admission policy: $(echo "${err}" | tail -1 | cut -c1-200)"
    return 1
}

# An ARecord manifest on stdin for the stolen-token checks.
# $1 = name, $2 = source-cluster label ("" for none), $3 = zone, $4 = DNS name,
# $5 = address
arecord_manifest() {
    local labels="    bindy.firestoned.io/zone: \"$3\""
    [ -n "$2" ] && labels="    bindy.firestoned.io/managed-by: scout
    bindy.firestoned.io/source-cluster: \"$2\"
${labels}"
    cat <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: $1
  namespace: ${NAMESPACE}
  labels:
${labels}
spec:
  name: "$4"
  ipv4Addresses: ["$5"]
YAML
}

# ─────────────────────────────────────────────────────────────────────────────
# Setup
# ─────────────────────────────────────────────────────────────────────────────

scout_setup_queen "${SCOUT_IMAGES[@]}"
step "Zones ${FOO_ZONE}, ${BAR_ZONE} (loose selector), ${SHARED_ZONE}"
scout_apply_zone foo-test "${FOO_ZONE}" "${TEAM}"
scout_apply_zone bar-test "${BAR_ZONE}" "${TEAM}" "bindy.firestoned.io/managed-by: scout"
scout_apply_zone shared-test "${SHARED_ZONE}" "${TEAM}"
scout_queen_ready
QUEEN_SERVER=$(kubectl --kubeconfig "${QUEEN_KUBECONFIG}" config view -o jsonpath='{.clusters[0].cluster.server}')

scout_setup_edge "${FOO_CLUSTER}" "${FOO_KUBECONFIG}" "${TEAM}"
scout_setup_edge "${BAR_CLUSTER}" "${BAR_KUBECONFIG}" "${TEAM}"

phase "Setup: registered accounts and Scouts"
use_edge foo
scout_register scout-foo foo "${FOO_ZONE},${SHARED_ZONE}"
bootstrap_scout "${IMAGE_REF}"
pass "Scout foo running with no Scout-side allowlist"
use_edge bar
scout_register scout-bar bar "${BAR_ZONE},${SHARED_ZONE}"
bootstrap_scout "${IMAGE_REF}"
pass "Scout bar running with --allowed-zones ${SCOUT_ALLOWED_ZONES} --allowed-ip-ranges ${SCOUT_ALLOWED_RANGES}"
pass "registry: $(${KUBECTL} get configmap bindy-scout-sources -n "${NAMESPACE}" -o jsonpath='{.data}')"

# ─────────────────────────────────────────────────────────────────────────────
# G1 / G5. Each cluster publishes into its own zone
# ─────────────────────────────────────────────────────────────────────────────

phase "G1/G5. team-a/web on both edges, each in its own zone"
P=${ERRORS}
use_edge foo; ZONE="${FOO_ZONE}" lb_service "${TEAM}" web "" "" "${FOO_WEB_IP}"
use_edge bar; ZONE="${BAR_ZONE}" lb_service "${TEAM}" web "" "" "${BAR_WEB_IP}"
ZONE="${FOO_ZONE}" expect_a web "${FOO_WEB_IP}"
ZONE="${BAR_ZONE}" expect_a web "${BAR_WEB_IP}"
wait_for "foo and bar each keep their own web record (#474)" \
    bash -c "[ -n \"\$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/source-cluster=foo,bindy.firestoned.io/zone=${FOO_ZONE} -o name)\" ] && [ -n \"\$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/source-cluster=bar,bindy.firestoned.io/zone=${BAR_ZONE} -o name)\" ]"
FOO_WEB=$(record_of foo "${FOO_ZONE}" web)
ZONE_OF_FOO_WEB=$(${KUBECTL} get arecord "${FOO_WEB}" -n "${NAMESPACE}" -o jsonpath='{.status.zoneRef.zoneName}' 2>/dev/null)
if [ "${ZONE_OF_FOO_WEB}" = "${FOO_ZONE}" ]; then
    pass "G5: foo's record is bound to ${FOO_ZONE}, not taken by ${BAR_ZONE}'s managed-by-only selector"
else
    fail "G5: foo's record is bound to '${ZONE_OF_FOO_WEB}'"
fi
# The skip is logged when bar-test reconciles; a new foo record does not
# trigger that (it is not selected), so reconcile bar-test on purpose and read
# only the lines logged since.
ZONE_NUDGE_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)
${KUBECTL} annotate dnszone bar-test -n "${NAMESPACE}" "e2e.bindy.firestoned.io/nudge=$(date +%s)" --overwrite >/dev/null
operator_logged_zone_skip() {
    ${KUBECTL} logs -n "${NAMESPACE}" -l app=bindy --since-time="${ZONE_NUDGE_AT}" --max-log-requests=10 2>/dev/null \
        | grep -q "${FOO_WEB} matches the selector of DNSZone ${NAMESPACE}/bar-test but is a Scout record labelled for another zone"
}
wait_for "G5: on reconcile, bar-test skipped ${FOO_WEB} (labelled for another zone)" operator_logged_zone_skip
record_phase "G1/G5: each cluster in its own zone; loose selector cannot cross zones" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# G2. A tenant on foo targets bar.test: refused at admission
# ─────────────────────────────────────────────────────────────────────────────

phase "G2. A tenant on foo (Scout without allowlist) publishes into ${BAR_ZONE}"
P=${ERRORS}
use_edge foo
ZONE="${BAR_ZONE}" lb_service "${TEAM}" evil web "" "${EVIL_IP}"
sleep "${REFUSAL_WINDOW}"
if [ "$(records_in foo "${BAR_ZONE}")" -eq 0 ]; then
    pass "no record from cluster foo in ${BAR_ZONE}"
else
    fail "cluster foo has $(records_in foo "${BAR_ZONE}") record(s) in ${BAR_ZONE}"
fi
if scout_logs | grep -q "bindy-scout-source-binding"; then
    pass "Scout foo's write was denied by bindy-scout-source-binding: $(scout_logs | grep -oE "denied (the )?request: .{0,140}" | head -1)"
else
    fail "Scout foo logged no admission denial for ${BAR_ZONE}"
fi
ZONE="${BAR_ZONE}" expect_a web "${BAR_WEB_IP}"
${KEDGE} delete service evil -n "${TEAM}" --wait=true --timeout=120s >/dev/null \
    && pass "the refused source deletes cleanly (finalizer released)" \
    || fail "the refused source is stuck deleting"
record_phase "G2: a cluster cannot publish into another cluster's zone (admission)" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# G3. bar's own allowlists
# ─────────────────────────────────────────────────────────────────────────────

phase "G3. bar's Scout-side allowlists"
P=${ERRORS}
use_edge bar
ZONE="${FOO_ZONE}" lb_service "${TEAM}" stray web "" "10.20.9.9"
ZONE="${BAR_ZONE}" lb_service "${TEAM}" outside api "" "${OUTSIDE_IP}"
sleep "${REFUSAL_WINDOW}"
if scout_logs | grep -q "not in this Scout's allowed zones"; then
    pass "Scout bar refused ${FOO_ZONE} itself (allowed zones)"
else
    fail "Scout bar did not log the allowed-zones refusal"
fi
if scout_logs | grep -q "outside this Scout's allowed ranges"; then
    pass "Scout bar refused ${OUTSIDE_IP} itself (allowed ranges)"
else
    fail "Scout bar did not log the allowed-ranges refusal"
fi
if scout_logs | grep -q "bindy-scout-source-binding"; then
    fail "Scout bar reached admission; its allowlist should have stopped it first"
else
    pass "nothing reached the bindy cluster's admission"
fi
[ "$(records_in bar "${FOO_ZONE}")" -eq 0 ] && pass "no record from bar in ${FOO_ZONE}" || fail "bar wrote into ${FOO_ZONE}"
ZONE="${BAR_ZONE}" expect_a api
ZONE="${FOO_ZONE}" expect_a web "${FOO_WEB_IP}"
${KEDGE} delete service stray outside -n "${TEAM}" --wait=true --timeout=120s >/dev/null || fail "refused sources stuck deleting"
record_phase "G3: a Scout's allowlists refuse other zones and outside addresses first" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# G4. foo's stolen token, used directly
# ─────────────────────────────────────────────────────────────────────────────

phase "G4. foo's stolen token against the bindy cluster"
P=${ERRORS}
use_edge foo
${KEDGE} get secret scout-foo-kubeconfig -n "${NAMESPACE}" -o jsonpath='{.data.kubeconfig}' \
    | base64 -d > "${STATE}/stolen.kubeconfig"
chmod 600 "${STATE}/stolen.kubeconfig"
# The kubeconfig points at the bindy cluster's network address; from this
# host the same API server answers on kind's mapped port, which its
# certificate also covers.
KSTOLEN="kubectl --kubeconfig ${STATE}/stolen.kubeconfig --server ${QUEEN_SERVER}"
if ${KSTOLEN} auth can-i create arecords -n "${NAMESPACE}" >/dev/null 2>&1; then
    pass "the stolen token authenticates as system:serviceaccount:${NAMESPACE}:scout-foo"
else
    fail "the stolen token does not work at all; G4 would prove nothing"
fi
BAR_WEB=$(record_of bar "${BAR_ZONE}" web)
expect_refused "create a record in ${BAR_ZONE} as cluster foo" \
    bash -c "$(declare -f arecord_manifest); NAMESPACE=${NAMESPACE} arecord_manifest g4-foo-in-bar foo ${BAR_ZONE} web ${EVIL_IP} | ${KSTOLEN} create -f -"
expect_refused "create a record stamped with cluster bar" \
    bash -c "$(declare -f arecord_manifest); NAMESPACE=${NAMESPACE} arecord_manifest g4-as-bar bar ${BAR_ZONE} web ${EVIL_IP} | ${KSTOLEN} create -f -"
expect_refused "create a record without the Scout labels" \
    bash -c "$(declare -f arecord_manifest); NAMESPACE=${NAMESPACE} arecord_manifest g4-unlabelled '' ${BAR_ZONE} web ${EVIL_IP} | ${KSTOLEN} create -f -"
expect_refused "rewrite bar's record ${BAR_WEB}" \
    ${KSTOLEN} patch arecord "${BAR_WEB}" -n "${NAMESPACE}" --type=merge -p "{\"spec\":{\"ipv4Addresses\":[\"${EVIL_IP}\"]}}"
expect_refused "relabel bar's record into foo's cluster" \
    ${KSTOLEN} label arecord "${BAR_WEB}" -n "${NAMESPACE}" --overwrite bindy.firestoned.io/source-cluster=foo
expect_refused "delete bar's record ${BAR_WEB}" \
    ${KSTOLEN} delete arecord "${BAR_WEB}" -n "${NAMESPACE}" --wait=false
if arecord_manifest g4-foo-own foo "${FOO_ZONE}" g4own "${FOO_OWN_IP}" | ${KSTOLEN} create -f - >/dev/null 2>&1; then
    pass "positive control: the token may still write its own zone"
    ZONE="${FOO_ZONE}" expect_a g4own "${FOO_OWN_IP}"
    ${KSTOLEN} delete arecord g4-foo-own -n "${NAMESPACE}" >/dev/null 2>&1 \
        && pass "positive control: and delete its own record" || fail "the token could not delete its own record"
else
    fail "positive control: the token could not write its own zone"
fi
ZONE="${BAR_ZONE}" expect_a web "${BAR_WEB_IP}"
rm -f "${STATE}/stolen.kubeconfig"
record_phase "G4: a stolen token is bound to its own cluster names and zones" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# G6. A refused withdrawal is skipped, never retried into a stuck finalizer
# ─────────────────────────────────────────────────────────────────────────────

phase "G6. A record left under an unregistered cluster name (${STALE_CLUSTER})"
P=${ERRORS}
STALE_LABEL="contributor.scout.bindy.firestoned.io/$(printf '%s\0%s\0%s' Service "${TEAM}" web | sha256sum | cut -c1-16)"
STALE_NAME="g6-${STALE_CLUSTER}-web"
must ${KUBECTL} apply -f - <<YAML
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ${STALE_NAME}
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/managed-by: scout
    bindy.firestoned.io/source-cluster: ${STALE_CLUSTER}
    bindy.firestoned.io/zone: ${FOO_ZONE}
    ${STALE_LABEL}: "true"
  annotations:
    bindy.firestoned.io/scout-contributors: '{"Service/${TEAM}/web":{"ips":["${FOO_WEB_IP}"]}}'
spec:
  name: web
  ipv4Addresses: ["${FOO_WEB_IP}"]
YAML
# What a Scout write would change. The operator's status writes bump the
# resourceVersion on their own, so that is not compared.
stale_content() {
    ${KUBECTL} get arecord "${STALE_NAME}" -n "${NAMESPACE}" -o json 2>/dev/null \
        | jq -c '{generation: .metadata.generation, spec, labels: .metadata.labels,
                  contributors: .metadata.annotations["bindy.firestoned.io/scout-contributors"]}'
}
STALE_BEFORE=$(stale_content)
use_edge foo
${KEDGE} annotate service web -n "${TEAM}" "e2e.bindy.firestoned.io/nudge=$(date +%s)" --overwrite >/dev/null
sleep "${REFUSAL_WINDOW}"
if scout_logs | grep -q "refused withdrawing from another cluster's ARecord"; then
    pass "Scout foo logged the refused withdrawal and moved on"
else
    fail "Scout foo did not log a refused withdrawal from ${STALE_NAME}"
fi
STALE_AFTER=$(stale_content)
if [ -n "${STALE_AFTER}" ] && [ "${STALE_AFTER}" = "${STALE_BEFORE}" ]; then
    pass "${STALE_NAME} was left untouched (generation, spec, labels and contributors unchanged)"
else
    fail "${STALE_NAME} changed although the bindy cluster refuses foo's writes to it: ${STALE_BEFORE} -> ${STALE_AFTER}"
fi
${KEDGE} delete service web -n "${TEAM}" --wait=true --timeout=120s >/dev/null \
    && pass "deleting team-a/web on foo completes: the refusal does not hold its finalizer" \
    || fail "team-a/web on foo is stuck deleting"
wait_for "foo's own web record removed" bash -c "[ -z \"\$(${KUBECTL} get arecord ${FOO_WEB} -n ${NAMESPACE} -o name 2>/dev/null)\" ]"
${KUBECTL} get arecord "${STALE_NAME}" -n "${NAMESPACE}" >/dev/null 2>&1 \
    && pass "${STALE_NAME} still exists (it belongs to a cluster foo cannot write)" \
    || fail "${STALE_NAME} disappeared"
${KUBECTL} delete arecord "${STALE_NAME}" -n "${NAMESPACE}" >/dev/null
ZONE="${FOO_ZONE}" expect_a web
record_phase "G6: a refused cleanup is skipped and never blocks a source's deletion" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# G7. A zone registered for both clusters: the first to publish keeps the name
# ─────────────────────────────────────────────────────────────────────────────

# Ready reason of cluster $1's record for name $3 in zone $2.
ready_reason_of() {
    scout_records_json | jq -r --arg c "$1" --arg z "$2" --arg n "$3" \
        '.items[] | select(.metadata.labels["bindy.firestoned.io/source-cluster"] == $c
                           and .metadata.labels["bindy.firestoned.io/zone"] == $z
                           and .spec.name == $n)
         | [.status.conditions[]? | select(.type == "Ready") | .reason] | first // ""' | head -1
}
reason_is() { [ "$(ready_reason_of "$1" "$2" "$3")" = "$4" ]; }

phase "G7. ${SHARED_ZONE} is registered for both clusters; both publish 'api' (ADR-0027)"
P=${ERRORS}
use_edge foo; ZONE="${SHARED_ZONE}" lb_service "${TEAM}" api api "" "${FOO_SHARED_IP}"
ZONE="${SHARED_ZONE}" expect_a api "${FOO_SHARED_IP}"
wait_for "foo's api record has published" reason_is foo "${SHARED_ZONE}" api ReconcileSucceeded
use_edge bar; ZONE="${SHARED_ZONE}" lb_service "${TEAM}" api api "" "${BAR_SHARED_IP}"
wait_for "bar's api record is held back (RRsetConflict)" reason_is bar "${SHARED_ZONE}" api RRsetConflict
BAR_API=$(record_of bar "${SHARED_ZONE}" api)
FOO_API=$(record_of foo "${SHARED_ZONE}" api)
MSG=$(${KUBECTL} get arecord "${BAR_API}" -n "${NAMESPACE}" -o jsonpath='{.status.conditions[?(@.type=="Ready")].message}')
if grep -q "${FOO_API} of cluster 'foo' has published 'api'" <<< "${MSG}"; then
    pass "the held-back record names the holder: ${MSG}"
else
    fail "the held-back record's message does not name the holder: '${MSG}'"
fi
sleep 15
ZONE="${SHARED_ZONE}" expect_a api "${FOO_SHARED_IP}"
step "Wipe a primary: the zone replay must keep serving the holder"
delete_pod_of "$(primary_instances | head -1)"
wait_bind_pods_ready || fail "BIND9 pods never came back"
ZONE="${SHARED_ZONE}" expect_a api "${FOO_SHARED_IP}"
step "The holder leaves: bar takes over without a timer"
use_edge foo; ${KEDGE} delete service api -n "${TEAM}" --wait=true --timeout=120s >/dev/null || fail "foo's api Service stuck deleting"
ZONE="${SHARED_ZONE}" expect_a api "${BAR_SHARED_IP}"
wait_for "bar's api record is now Ready" reason_is bar "${SHARED_ZONE}" api ReconcileSucceeded
record_phase "G7: the first cluster to publish keeps the name; replay and handover follow (ADR-0027)" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# G8. A registered rename is not a conflict
# ─────────────────────────────────────────────────────────────────────────────

phase "G8. foo is renamed foo2 with both names registered"
P=${ERRORS}
use_edge foo
scout_register scout-foo "foo,foo2" "${FOO_ZONE},${SHARED_ZONE}"
ZONE="${FOO_ZONE}" lb_service "${TEAM}" rn rn "" "10.10.8.8"
ZONE="${FOO_ZONE}" expect_a rn "10.10.8.8"
wait_for "foo's rn record has published" reason_is foo "${FOO_ZONE}" rn ReconcileSucceeded
EDGE_NAME="foo2"
bootstrap_scout "${IMAGE_REF}"
wait_for "foo2 took rn over and foo's record is gone" bash -c \
    "[ -n \"\$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/source-cluster=foo2,bindy.firestoned.io/zone=${FOO_ZONE} -o name)\" ] && [ -z \"\$(${KUBECTL} get arecords -n ${NAMESPACE} -l bindy.firestoned.io/source-cluster=foo,bindy.firestoned.io/zone=${FOO_ZONE} -o name)\" ]"
if scout_records_json | jq -e '[.items[].status.conditions[]? | select(.type == "Ready" and .reason == "RRsetConflict")] | length == 0' >/dev/null; then
    pass "no record was held back during the rename"
else
    fail "a record went RRsetConflict during a registered rename"
fi
ZONE="${FOO_ZONE}" expect_a rn "10.10.8.8"
EDGE_NAME="foo"
bootstrap_scout "${IMAGE_REF}"
record_phase "G8: a rename with both names registered completes without a conflict" "${P}"

# ─────────────────────────────────────────────────────────────────────────────
# Cleanup
# ─────────────────────────────────────────────────────────────────────────────

phase "Cleanup: remove every source on both edges"
P=${ERRORS}
for edge in foo bar; do
    use_edge "${edge}"
    ${KEDGE} delete services,ingresses -n "${TEAM}" --all --wait=true --timeout=180s >/dev/null || fail "${edge}: sources stuck deleting"
done
wait_for "no foo or bar record left on the bindy cluster" bash -c \
    "[ -z \"\$(${KUBECTL} get arecords -n ${NAMESPACE} -l 'bindy.firestoned.io/source-cluster in (foo,foo2,bar)' -o name)\" ]"
record_phase "Removing every source on both edges removes their records" "${P}"

finish "${SUITE_TITLE}"
