#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# E2E suite: Scout's zone-scoped stale-cluster ARecord cleanup (#474, #497).
#
# Scout labels every ARecord it creates with managed-by, source-cluster,
# source-namespace, source-name and zone, then deletes "stale" records — ones
# for the same source namespace + name written under a DIFFERENT cluster name —
# on every reconcile, so a Scout restarted under a new --cluster-name cleans up
# after itself.
#
# Before #474 that selector did not match on zone, so two unrelated clusters
# that happened to run the same namespace/name deleted each other's LIVE
# records in a flapping loop. Only a real API server evaluating a real selector
# against real records can show that the fix holds end to end: the unit tests
# assert selector text, and wiremock echoes whatever it is handed.
#
# Scout here runs in single-cluster mode (its remote client is this same
# cluster), which is enough to exercise every path under test and avoids a
# second kind cluster.
#
# Usage: tests/e2e/scout_test.sh [--image REF] [--skip-deploy]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-scout}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"

parse_common_args "$@"

SOURCE_NS="team-checkout"
INGRESS_NAME="web-frontend"
ZONE_ALPHA="zone-alpha.example.internal"
ZONE_BETA="zone-beta.example.internal"
# 68 characters: every DNS label is <= 63 so the DNSZone CRD's zoneName pattern
# accepts it, but it exceeds the 63-char Kubernetes label-value limit.
ZONE_OVERLONG="payments-gateway.team-checkout.production.eu-west-1.example.internal"

SCOUT_READY_TIMEOUT=180
# Scout is watch-driven; these bound how long an assertion waits for a
# reconcile to land before calling it a failure.
SETTLE_SECS=3
POLL_TIMEOUT=90

info "🧪 E2E: Scout zone-scoped stale cleanup (cluster '${CLUSTER_NAME}')"

# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────

# Waits until an ARecord exists (or stops existing) in the operator namespace.
#
# $1 — record name, $2 — "present" | "absent"
wait_for_arecord() {
    local record="$1" want="$2" deadline=$((SECONDS + POLL_TIMEOUT))
    while [ ${SECONDS} -lt ${deadline} ]; do
        if ${KUBECTL} get arecord "${record}" -n "${NAMESPACE}" >/dev/null 2>&1; then
            [ "${want}" = "present" ] && return 0
        else
            [ "${want}" = "absent" ] && return 0
        fi
        sleep 2
    done
    return 1
}

# Creates an ARecord carrying exactly the labels Scout writes, standing in for
# a record left behind by another cluster (or by this one under an older name).
#
# $1 — record name, $2 — source-cluster label, $3 — zone label
plant_arecord() {
    local record="$1" source_cluster="$2" zone="$3"
    ${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: ${record}
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/managed-by: scout
    bindy.firestoned.io/source-cluster: ${source_cluster}
    bindy.firestoned.io/source-namespace: ${SOURCE_NS}
    bindy.firestoned.io/source-name: ${INGRESS_NAME}
    bindy.firestoned.io/zone: ${zone}
spec:
  name: app
  ipv4Addresses:
    - 192.0.2.10
EOF
}

# Applies the Scout-managed Ingress. $1 — zone annotation value.
apply_ingress() {
    ${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: ${INGRESS_NAME}
  namespace: ${SOURCE_NS}
  annotations:
    bindy.firestoned.io/scout-enabled: "true"
    bindy.firestoned.io/zone: "$1"
    bindy.firestoned.io/ip: "192.0.2.50"
spec:
  rules:
    - host: app.$1
EOF
}

# Restarts Scout under a new --cluster-name and waits for it to come back.
# $1 — new cluster name.
redeploy_scout_as() {
    step "Redeploying Scout as cluster '$1'"
    ${KUBECTL} set env deployment/bindy-scout -n "${NAMESPACE}" \
        "BINDY_SCOUT_CLUSTER_NAME=$1" >/dev/null
    ${KUBECTL} rollout status deployment/bindy-scout -n "${NAMESPACE}" \
        --timeout="${SCOUT_READY_TIMEOUT}s" >/dev/null
    sleep "${SETTLE_SECS}"
}

scout_logs() {
    ${KUBECTL} logs -n "${NAMESPACE}" -l app.kubernetes.io/component=scout \
        --tail=500 2>/dev/null || true
}

# ─────────────────────────────────────────────────────────────────────────────
# Setup
# ─────────────────────────────────────────────────────────────────────────────

phase "Setting up cluster and CRDs"
bindy_ensure_cluster deploy/kind-config-e2e.yaml
bindy_prepare_image

phase "Installing Scout RBAC and deployment"
for f in serviceaccount clusterrole clusterrolebinding role rolebinding; do
    ${KUBECTL} apply -f "${PROJECT_ROOT}/deploy/scout/${f}.yaml" >/dev/null
done

${KUBECTL} create namespace "${SOURCE_NS}" --dry-run=client -o yaml \
    | ${KUBECTL} apply -f - >/dev/null

step "Creating DNSZones that authorize ${SOURCE_NS}"
for zone in "${ZONE_ALPHA}" "${ZONE_BETA}" "${ZONE_OVERLONG}"; do
    ${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: DNSZone
metadata:
  name: $(echo "${zone}" | tr '.' '-')
  namespace: ${NAMESPACE}
  annotations:
    bindy.firestoned.io/allow-zone-namespaces: "${SOURCE_NS}"
spec:
  zoneName: ${zone}
  clusterRef: scout-e2e-unused
  soaRecord:
    primaryNs: ns1.${zone}.
    adminEmail: admin.${zone}.
    serial: 2026010101
    refresh: 3600
    retry: 600
    expire: 604800
    negativeTtl: 86400
EOF
done

step "Deploying Scout as cluster 'north'"
sed -E -e "s|image: ghcr.io/firestoned/bindy[:@][^\"[:space:]]*|image: ${IMAGE_REF}|g" \
       -e "s|value: \"test-cluster.example.com\"|value: \"north\"|" \
    "${PROJECT_ROOT}/deploy/scout/deployment.yaml" | ${KUBECTL} apply -f - >/dev/null
${KUBECTL} rollout status deployment/bindy-scout -n "${NAMESPACE}" \
    --timeout="${SCOUT_READY_TIMEOUT}s" >/dev/null
pass "Scout is available"

# ─────────────────────────────────────────────────────────────────────────────
# 1. Scout creates a labelled ARecord for an opted-in Ingress
# ─────────────────────────────────────────────────────────────────────────────

phase "1. Scout creates a zone-labelled ARecord"
apply_ingress "${ZONE_ALPHA}"

OWN_RECORD="scout-north-${SOURCE_NS}-${INGRESS_NAME}-0"
if wait_for_arecord "${OWN_RECORD}" present; then
    pass "ARecord ${OWN_RECORD} created"
else
    fail "Scout never created ${OWN_RECORD}"
    scout_logs | tail -40
fi

ACTUAL_ZONE=$(${KUBECTL} get arecord "${OWN_RECORD}" -n "${NAMESPACE}" \
    -o jsonpath='{.metadata.labels.bindy\.firestoned\.io/zone}' 2>/dev/null || true)
if [ "${ACTUAL_ZONE}" = "${ZONE_ALPHA}" ]; then
    pass "zone label is ${ZONE_ALPHA}"
else
    fail "zone label was '${ACTUAL_ZONE}', expected '${ZONE_ALPHA}'"
fi

summary_row "Scout creates an ARecord labelled with its zone" \
            "$([ ${ERRORS} -eq 0 ] && echo '✅ passed' || echo '❌ failed')"

# ─────────────────────────────────────────────────────────────────────────────
# 2. #474: a live record from another cluster in a DIFFERENT zone survives
# ─────────────────────────────────────────────────────────────────────────────

phase "2. Cross-zone record from an unrelated cluster survives (#474)"
BEFORE_ERRORS=${ERRORS}

plant_arecord "live-west-beta" "west" "${ZONE_BETA}"
# Force a reconcile so stale cleanup definitely runs.
${KUBECTL} annotate ingress "${INGRESS_NAME}" -n "${SOURCE_NS}" \
    "e2e.bindy.firestoned.io/nudge=$(date +%s)" --overwrite >/dev/null
sleep "${SETTLE_SECS}"

if ${KUBECTL} get arecord "live-west-beta" -n "${NAMESPACE}" >/dev/null 2>&1; then
    pass "live-west-beta survived — cross-zone records are not treated as stale"
else
    fail "REGRESSION (#474): Scout deleted a live ARecord belonging to another \
cluster in a different zone"
fi

summary_row "Cross-zone record from an unrelated cluster is left alone (#474)" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

# ─────────────────────────────────────────────────────────────────────────────
# 3. A real --cluster-name rename cleans up the records left under the old name
# ─────────────────────────────────────────────────────────────────────────────

phase "3. Renaming the cluster cleans up records left under the old name"
BEFORE_ERRORS=${ERRORS}

# This is the scenario the whole mechanism exists for, driven for real rather
# than simulated with a planted record: the same physical Scout comes back
# under a new --cluster-name and has to collect what it left behind.
redeploy_scout_as "south"

RENAMED_RECORD="scout-south-${SOURCE_NS}-${INGRESS_NAME}-0"
if wait_for_arecord "${RENAMED_RECORD}" present; then
    pass "${RENAMED_RECORD} created under the new cluster name"
else
    fail "Scout never created ${RENAMED_RECORD} after the rename"
    scout_logs | tail -40
fi

if wait_for_arecord "${OWN_RECORD}" absent; then
    pass "${OWN_RECORD} deleted — the old cluster name's record was cleaned up"
else
    fail "Scout left ${OWN_RECORD} behind after a --cluster-name change"
    scout_logs | tail -40
fi

# The whole point of #474: the rename cleanup must not reach across zones.
if ${KUBECTL} get arecord "live-west-beta" -n "${NAMESPACE}" >/dev/null 2>&1; then
    pass "live-west-beta still untouched after the rename"
else
    fail "REGRESSION (#474): the rename cleanup deleted a cross-zone record"
fi

summary_row "A --cluster-name rename cleans up only its own zone's records" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

# ─────────────────────────────────────────────────────────────────────────────
# 4. A zone too long to be a label value is refused, not retried forever
# ─────────────────────────────────────────────────────────────────────────────

phase "4. A label-illegal zone is refused without hot-looping"
BEFORE_ERRORS=${ERRORS}

${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: overlong-zone
  namespace: ${SOURCE_NS}
  annotations:
    bindy.firestoned.io/scout-enabled: "true"
    bindy.firestoned.io/zone: "${ZONE_OVERLONG}"
    bindy.firestoned.io/ip: "192.0.2.51"
spec:
  rules:
    - host: app.${ZONE_OVERLONG}
EOF
sleep "${SETTLE_SECS}"

# This is the assertion that actually distinguishes fixed from unfixed
# behaviour. "No ARecord created" holds either way — before the fix the apply
# failed with a 422 because the zone label is too long; after it, Scout never
# attempts the apply at all. Only the warning proves it was refused at the
# guard, so this is a hard failure, not a warning.
if scout_logs | grep -q "not a usable Kubernetes label value"; then
    pass "Scout warned that the zone is not a usable label value"
else
    fail "Scout did not warn that the zone is unusable — it is not refusing the \
zone at the guard"
    scout_logs | tail -20
fi

if ${KUBECTL} get arecord -n "${NAMESPACE}" \
       -l "bindy.firestoned.io/source-name=overlong-zone" \
       -o name 2>/dev/null | grep -q .; then
    fail "Scout created an ARecord for a zone it cannot label"
else
    pass "no ARecord created for the label-illegal zone"
fi

# The real regression: a 400 from the stale-cleanup list used to be retried
# every 30s forever. Scout must still be Ready and not crash-looping.
RESTARTS=$(${KUBECTL} get pods -n "${NAMESPACE}" \
    -l app.kubernetes.io/component=scout \
    -o jsonpath='{.items[0].status.containerStatuses[0].restartCount}' 2>/dev/null || echo 0)
if [ "${RESTARTS:-0}" -eq 0 ]; then
    pass "Scout has not restarted (no crash loop on the invalid zone)"
else
    fail "Scout restarted ${RESTARTS} time(s) after seeing an invalid zone"
fi

${KUBECTL} delete ingress overlong-zone -n "${SOURCE_NS}" >/dev/null 2>&1 || true

summary_row "Label-illegal zone is refused without a crash or hot loop" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

# ─────────────────────────────────────────────────────────────────────────────
# 5. Opting out by stripping every annotation still cleans up stale records
# ─────────────────────────────────────────────────────────────────────────────

phase "5. Opt-out that removes the zone annotation still cleans stale records"
BEFORE_ERRORS=${ERRORS}

# A record left by a previous cluster name, in the same zone: this is what the
# opt-out path must still clean up even though the zone annotation is about to
# disappear along with the opt-in.
plant_arecord "stale-oldsouth-optout" "old-south" "${ZONE_ALPHA}"
wait_for_arecord "${RENAMED_RECORD}" present || true

step "Removing every bindy.firestoned.io/* annotation in one edit"
${KUBECTL} annotate ingress "${INGRESS_NAME}" -n "${SOURCE_NS}" \
    bindy.firestoned.io/scout-enabled- \
    bindy.firestoned.io/zone- \
    bindy.firestoned.io/ip- >/dev/null

if wait_for_arecord "${RENAMED_RECORD}" absent; then
    pass "Scout removed its own ARecord on opt-out"
else
    fail "Scout left its own ARecord behind after opt-out"
fi

# Before the zone was recovered from the deleted records' labels, this record
# was orphaned permanently: no annotation meant no zone, no zone meant no
# stale cleanup, and the finalizer was released regardless.
if wait_for_arecord "stale-oldsouth-optout" absent; then
    pass "stale-oldsouth-optout deleted — zone recovered from record labels"
else
    fail "Opt-out orphaned a same-zone stale record (zone not recovered from \
the deleted records' labels)"
    scout_logs | tail -40
fi

# The cross-zone record planted in step 2 must STILL be untouched.
if ${KUBECTL} get arecord "live-west-beta" -n "${NAMESPACE}" >/dev/null 2>&1; then
    pass "live-west-beta still untouched after opt-out"
else
    fail "opt-out cleanup deleted a cross-zone record from another cluster"
fi

summary_row "Opt-out recovers the zone from record labels and cleans up" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

finish "E2E — Scout zone-scoped stale cleanup"
