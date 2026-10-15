#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# E2E suite: an RNDC key from rndcKey.secretRef (ADR-0023).
#
# The key lives in a user Secret under custom entry names, with no rndc.key
# file. The operator mirrors it into {instance}-rndc-key, which the pods
# mount; a change to the source reaches the pods through a rollout. Every
# step is proven against BIND9 itself: rndc status inside the pod (named
# loaded the key) and a record written after the change being served (the
# operator signs with the same key the pods hold).
#
# Usage: tests/e2e/rndc_secretref_test.sh [--image REF] [--skip-deploy]

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-rndc-secretref}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"
source "${LIB_DIR}/dns_fixtures.sh"

parse_common_args "$@"

SOURCE_SECRET="bindy-e2e-rndc"
FOREIGN_SECRET="e2e-foreign-rndc"
KEY_NAME_ENTRY="corp-name"
KEY_ENTRY="corp-key"
KEY_NAME="e2e-corp-key"
# Every RNDC key bindy writes is named this in BIND9 (ADR-0023): the source's
# own key name is not used.
MIRRORED_KEY_NAME="bindy-operator"
KEY_BYTES=32
ANNOTATION_SOURCE='bindy\.firestoned\.io/rndc-source-secret'
MIRROR_TIMEOUT=120
ROLLOUT_TIMEOUT=300

info "🧪 E2E: RNDC key from rndcKey.secretRef (cluster '${CLUSTER_NAME}')"

new_key() { openssl rand -base64 "${KEY_BYTES}"; }

# Writes the source Secret with the key under custom entry names only.
# $1: base64 key
write_source_secret() {
    ${KUBECTL} create secret generic "${SOURCE_SECRET}" -n "${NAMESPACE}" \
        --from-literal="${KEY_NAME_ENTRY}=${KEY_NAME}" \
        --from-literal="${KEY_ENTRY}=$1" \
        --dry-run=client -o yaml | ${KUBECTL} apply -f - >/dev/null
}

# Sets (or, with no argument, removes) the cluster's primary rndcKey.
# $1: secret name
set_cluster_secret_ref() {
    local patch='{"spec":{"primary":{"rndcKey":null}}}'
    if [ -n "${1:-}" ]; then
        patch='{"spec":{"primary":{"rndcKey":{"secretRef":{"name":"'"$1"'","keyNameKey":"'"${KEY_NAME_ENTRY}"'","secretKey":"'"${KEY_ENTRY}"'"}}}}}'
    fi
    ${KUBECTL} patch bind9cluster "${CLUSTER_CR}" -n "${NAMESPACE}" --type=merge -p "${patch}" >/dev/null
}

mirror_field() {
    ${KUBECTL} get secret "$1-rndc-key" -n "${NAMESPACE}" -o jsonpath="$2" 2>/dev/null || true
}

mirror_key() { mirror_field "$1" '{.data.secret}' | base64 -d 2>/dev/null || true; }

# Waits until every primary's mirror holds `key` and names the source.
# $1: base64 key, $2: label
assert_mirrors_hold() {
    local key=$1 label=$2 inst deadline name source
    for inst in $(expected_primaries); do
        deadline=$((SECONDS + MIRROR_TIMEOUT))
        while [ "$(mirror_key "${inst}")" != "${key}" ] && [ "${SECONDS}" -lt "${deadline}" ]; do
            sleep "${POLL_INTERVAL}"
        done
        name=$(mirror_field "${inst}" '{.data.key-name}' | base64 -d 2>/dev/null || true)
        source=$(mirror_field "${inst}" "{.metadata.annotations.${ANNOTATION_SOURCE}}")
        if [ "$(mirror_key "${inst}")" = "${key}" ] && [ "${name}" = "${MIRRORED_KEY_NAME}" ] \
               && [ "${source}" = "${SOURCE_SECRET}" ]; then
            pass "${label}: ${inst}-rndc-key mirrors ${SOURCE_SECRET}"
        else
            fail "${label}: ${inst}-rndc-key does not mirror ${SOURCE_SECRET} (name '${name}', source '${source}')"
        fi
    done
}

# Pod name per primary, one per line as "instance=pod".
pod_snapshot() {
    local inst
    for inst in $(expected_primaries); do echo "${inst}=$(instance_pod "${inst}")"; done
}

# Waits until every primary runs a Ready pod other than the one in `before`.
# $1: snapshot from pod_snapshot, $2: label
assert_pods_rolled() {
    local before=$1 label=$2 inst old deadline pod
    for inst in $(expected_primaries); do
        old=$(echo "${before}" | sed -n "s/^${inst}=//p")
        deadline=$((SECONDS + ROLLOUT_TIMEOUT))
        pod=$(instance_pod "${inst}")
        while { [ -z "${pod}" ] || [ "${pod}" = "${old}" ]; } && [ "${SECONDS}" -lt "${deadline}" ]; do
            sleep "${POLL_INTERVAL}"
            pod=$(instance_pod "${inst}")
        done
        if [ -n "${pod}" ] && [ "${pod}" != "${old}" ]; then
            pass "${label}: ${inst} rolled (${old} -> ${pod})"
        else
            fail "${label}: ${inst} did not roll"
        fi
        # The readiness check selects by label, so a pod still terminating
        # (never Ready again) would fail it: wait until the old one is gone.
        if [ -n "${old}" ]; then
            ${KUBECTL} wait --for=delete "pod/${old}" -n "${NAMESPACE}" \
                --timeout="${ROLLOUT_TIMEOUT}s" >/dev/null 2>&1 || true
        fi
    done
    assert_operands_ready
}

# rndc status inside each primary: named accepts the key the pod mounts.
assert_rndc_works() {
    local label=$1 inst pod
    for inst in $(expected_primaries); do
        pod=$(instance_pod "${inst}")
        if ${KUBECTL} exec -n "${NAMESPACE}" "${pod}" -c "${BIND9_CONTAINER}" -- rndc status >/dev/null 2>&1; then
            pass "${label}: rndc status succeeds on ${inst}"
        else
            fail "${label}: rndc status fails on ${inst}"
        fi
    done
}

# Writes a fresh A record and checks every primary serves it: the operator
# signed the update with the key the pods hold.
# $1: probe number, $2: label
assert_new_record_served() {
    local n=$1 label=$2 inst pod attempt got ip="192.0.2.$((100 + $1))"
    ${KUBECTL} apply -f - >/dev/null <<EOF
apiVersion: bindy.firestoned.io/v1beta1
kind: ARecord
metadata:
  name: rndc-probe-${n}
  namespace: ${NAMESPACE}
  labels:
    bindy.firestoned.io/zone: ${ZONE_FQDN}
spec:
  name: rndc-probe-${n}
  ipv4Addresses:
    - "${ip}"
  ttl: 60
EOF
    for inst in $(expected_primaries); do
        pod=$(instance_pod "${inst}")
        got=""
        for ((attempt = 1; attempt <= DNS_SETTLE_RETRIES; attempt++)); do
            got=$(dig_in_pod "${pod}" A "rndc-probe-${n}.${ZONE_FQDN}.")
            [ "${got}" = "${ip}" ] && break
            sleep "${DNS_SETTLE_SLEEP}"
        done
        if [ "${got}" = "${ip}" ]; then
            pass "${label}: ${inst} serves rndc-probe-${n} written after the change"
        else
            fail "${label}: ${inst} does not serve rndc-probe-${n} (got '${got}')"
        fi
    done
}

# The operator's whole log, ANSI colour codes stripped (the text format
# colours each field name, so `kind="audit"` never appears verbatim). Read pod
# by pod: with a label selector, `kubectl logs` caps the output at 10 lines
# whatever `--tail` says (-1 means "10" there).
operator_logs() {
    local pod
    for pod in $(${KUBECTL} get pods -n "${NAMESPACE}" -l app=bindy -o name 2>/dev/null); do
        ${KUBECTL} logs -n "${NAMESPACE}" "${pod}" 2>/dev/null || true
    done | sed 's/\x1b\[[0-9;]*m//g'
}

# ADR-0024: the operator recorded a Secret operation with these field values.
# $1: operation, $2: purpose, $3: label
assert_audit_event() {
    local operation=$1 purpose=$2 label=$3 logs
    logs=$(operator_logs)
    # One grep on a here-string, no pipe: a `grep -q` at the end of a pipe
    # would fail it under pipefail (the producers get SIGPIPE). The event's
    # fields always come in this order.
    if grep -qE "kind=\"?audit\"?.*operation=\"?${operation}\"?.*purpose=\"?${purpose}\"?( |$)" \
           <<< "${logs}"; then
        pass "${label}: audit event ${operation} ${purpose}"
    else
        fail "${label}: no audit event ${operation} ${purpose}"
    fi
}

# ADR-0024: no operator log line carries the key.
# $1: base64 key, $2: label
assert_key_never_logged() {
    local key=$1 label=$2
    if grep -qF -- "${key}" <<< "$(operator_logs)"; then
        fail "${label}: the RNDC key appears in the operator log"
    else
        pass "${label}: the RNDC key is not in the operator log"
    fi
}

cleanup_probes() {
    ${KUBECTL} delete arecord -n "${NAMESPACE}" rndc-probe-1 rndc-probe-2 rndc-probe-3 rndc-probe-4 \
        --ignore-not-found=true --wait=false >/dev/null 2>&1 || true
}

phase "Setting up cluster and operator"
bindy_setup deploy/kind-config-e2e.yaml

phase "Establishing the baseline"
preclean_fixtures
cleanup_probes
${KUBECTL} delete secret "${SOURCE_SECRET}" "${FOREIGN_SECRET}" -n "${NAMESPACE}" \
    --ignore-not-found=true >/dev/null 2>&1 || true
apply_test_manifests
assert_fixture_healthy "baseline"
if [ "${ERRORS}" -ne 0 ]; then
    fail "baseline is not healthy; skipping the secretRef phases"
    finish "E2E: RNDC key from rndcKey.secretRef" || true
    exit 1
fi

phase "1. A secretRef with custom entry names is mirrored and served"
BEFORE_ERRORS=${ERRORS}
KEY_ONE=$(new_key)
write_source_secret "${KEY_ONE}"
SNAPSHOT=$(pod_snapshot)
set_cluster_secret_ref "${SOURCE_SECRET}"
assert_mirrors_hold "${KEY_ONE}" "secretRef applied"
assert_pods_rolled "${SNAPSHOT}" "secretRef applied"
assert_rndc_works "secretRef applied"
assert_dns_on_primaries "secretRef applied"
assert_new_record_served 1 "secretRef applied"
assert_audit_event read rndc-key-source "secretRef applied"
assert_audit_event replace rndc-key-mirror "secretRef applied"
assert_key_never_logged "${KEY_ONE}" "secretRef applied"
summary_row "Custom-entry secretRef mirrored, pods roll, writes served" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

phase "2. A changed source key reaches the pods"
BEFORE_ERRORS=${ERRORS}
KEY_TWO=$(new_key)
SNAPSHOT=$(pod_snapshot)
write_source_secret "${KEY_TWO}"
assert_mirrors_hold "${KEY_TWO}" "source key changed"
assert_pods_rolled "${SNAPSHOT}" "source key changed"
assert_rndc_works "source key changed"
assert_dns_on_primaries "source key changed"
assert_new_record_served 2 "source key changed"
assert_key_never_logged "${KEY_TWO}" "source key changed"
summary_row "A source key change rolls the pods and keeps writes working" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

phase "3. A secretRef without the bindy- prefix is refused"
BEFORE_ERRORS=${ERRORS}
${KUBECTL} create secret generic "${FOREIGN_SECRET}" -n "${NAMESPACE}" \
    --from-literal="${KEY_NAME_ENTRY}=foreign" --from-literal="${KEY_ENTRY}=$(new_key)" \
    --dry-run=client -o yaml | ${KUBECTL} apply -f - >/dev/null
# With the admission policies installed the patch itself is rejected; without
# them the reconciler refuses it. Either way the mirrors keep the source key.
set_cluster_secret_ref "${FOREIGN_SECRET}" 2>/dev/null || true
sleep "${RECONCILE_SETTLE_SECS}"
for inst in $(expected_primaries); do
    if [ "$(mirror_key "${inst}")" = "${KEY_TWO}" ]; then
        pass "foreign secretRef: ${inst}-rndc-key still holds the bindy- source key"
    else
        fail "foreign secretRef: ${inst}-rndc-key changed"
    fi
done
# Captured first: `grep -q` in a pipeline exits on its first match, and under
# pipefail the producer's SIGPIPE would fail the test.
OPERATOR_LOGS=$(operator_logs)
APPLIED_REF=$(${KUBECTL} get bind9cluster "${CLUSTER_CR}" -n "${NAMESPACE}" \
    -o jsonpath='{.spec.primary.rndcKey.secretRef.name}' 2>/dev/null || true)
if [[ "${OPERATOR_LOGS}" == *"rndcKey.secretRef rejected"* ]] \
   || [ "${APPLIED_REF}" != "${FOREIGN_SECRET}" ]; then
    pass "foreign secretRef refused (reconciler log or admission)"
else
    fail "foreign secretRef was neither rejected at admission nor refused by the reconciler"
fi
set_cluster_secret_ref "${SOURCE_SECRET}"
assert_mirrors_hold "${KEY_TWO}" "back to the bindy- source"
summary_row "A secretRef without the bindy- prefix is refused" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

phase "4. A deleted source keeps the mirror and DNS up"
BEFORE_ERRORS=${ERRORS}
${KUBECTL} delete secret "${SOURCE_SECRET}" -n "${NAMESPACE}" >/dev/null
sleep "${RECONCILE_SETTLE_SECS}"
for inst in $(expected_primaries); do
    if [ "$(mirror_key "${inst}")" = "${KEY_TWO}" ]; then
        pass "source deleted: ${inst}-rndc-key kept"
    else
        fail "source deleted: ${inst}-rndc-key was removed or changed"
    fi
done
assert_rndc_works "source deleted"
assert_dns_on_primaries "source deleted"
summary_row "A deleted source Secret keeps the mirror and DNS up" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

phase "5. Switching back to a generated key replaces the mirror"
BEFORE_ERRORS=${ERRORS}
SNAPSHOT=$(pod_snapshot)
set_cluster_secret_ref ""
for inst in $(expected_primaries); do
    deadline=$((SECONDS + MIRROR_TIMEOUT))
    while [ -n "$(mirror_field "${inst}" "{.metadata.annotations.${ANNOTATION_SOURCE}}")" ] \
          && [ "${SECONDS}" -lt "${deadline}" ]; do
        sleep "${POLL_INTERVAL}"
    done
    if [ -z "$(mirror_field "${inst}" "{.metadata.annotations.${ANNOTATION_SOURCE}}")" ] \
           && [ "$(mirror_key "${inst}")" != "${KEY_TWO}" ]; then
        pass "generated again: ${inst}-rndc-key holds a fresh generated key"
    else
        fail "generated again: ${inst}-rndc-key is still the mirror"
    fi
done
assert_pods_rolled "${SNAPSHOT}" "generated again"
assert_rndc_works "generated again"
assert_dns_on_primaries "generated again"
assert_new_record_served 3 "generated again"
assert_audit_event delete rndc-key "generated again"
assert_audit_event create rndc-key "generated again"
summary_row "Switching back to a generated key rolls the pods with a fresh key" \
            "$([ ${ERRORS} -eq ${BEFORE_ERRORS} ] && echo '✅ passed' || echo '❌ failed')"

cleanup_probes
${KUBECTL} delete secret "${FOREIGN_SECRET}" -n "${NAMESPACE}" --ignore-not-found=true >/dev/null 2>&1 || true
finish "E2E: RNDC key from rndcKey.secretRef"
