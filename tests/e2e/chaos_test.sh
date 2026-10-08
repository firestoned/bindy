#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# E2E suite: chaos.
#
# Release candidates kept passing unit tests and the steady-state suites, then
# broke on a real cluster the first time pods were replaced. This suite is the
# real-cluster regression: it builds a realistic topology, breaks it fifteen
# different ways, and after every step proves the system converged back to a
# fully correct state, quickly, and then went quiet.
#
# Fixture (tests/lib/chaos.sh): a Bind9Cluster with 2 primaries and 1
# secondary (the secondary really transfers), a standalone primary in the same
# cluster, a forward zone with A/AAAA/CNAME/MX/TXT/SRV/CAA records, a reverse
# zone with a PTR, and an extra zone that step 12 swaps. The operator runs 2
# replicas with leader election. Kind: 1 control plane + 2 workers
# (deploy/kind-config-chaos.yaml).
#
# Invariants, checked after every step, polled until they all hold at once or
# CONVERGE_TIMEOUT (default 120 s) runs out; the convergence time is reported:
#   - every instance has exactly one live, Ready pod; the operator has all its
#     replicas Ready, the lease holder is a live pod, no operator container
#     ever restarted; every BIND9 Service and the cluster ConfigMap exist;
#   - every BIND9 pod (primaries and the secondary), dug by pod IP on 5353
#     from the tools pod, serves the exact expected answer for every record
#     (deleted churn records must not resolve) and an SOA for every zone; the
#     secondary's serial equals the serial of at least one primary;
#   - `rndc zonestatus` reports every zone loaded on every pod, and deleted
#     zones gone; `rndc showzone`: each primary's allow-transfer is exactly the
#     live secondary pod IPs and its also-notify exactly expected_also_notify
#     (ALSO_NOTIFY_MODE=clusterip: the secondary Services' ClusterIPs on 53;
#     podip: the secondary pod IPs on 5353); each secondary's primaries are
#     exactly the live primary pod IPs on 5353, nothing stale;
#   - every DNSZone, Bind9Instance, Bind9Cluster and record is Ready=True, none
#     Degraded, none left RolloutQueued. When convergence times out with every
#     status claiming Ready while a DNS invariant fails, the step also reports
#     UNTRUTHFUL STATUS;
#   - the operator is quiet for QUIET_WINDOW (default 60 s) after convergence:
#     reconcile counters grow by at most QUIET_RECONCILE_TOLERANCE (3), no
#     Deployment PATCH, no ERROR log line, no WARN line repeated 3+ times.
#
# DNS prober: a pod queries the SOA of both static zones on every BIND9
# Service once a second for the whole run. Per step it reports the longest run
# of consecutive failed seconds per Service and the longest run of seconds in
# which a zone had no answering Service at all.
#
# Documented, honest limits encoded as bounds instead of dropped assertions:
#   - Each instance runs one replica. Deleting its pod (steps 4, 5, 7, 11, 12,
#     14), restarting its named or bindcar container (8, 9: the pod's
#     readiness includes both containers, and a restarted named answers only
#     once its readiness probe passes), or deleting its Service (3) takes that
#     Service out of answering until the replacement is Ready. That Service
#     may fail for up to TARGET_GAP_BOUND (120 s); every other Service must
#     not fail for more than UNTOUCHED_GAP_TOLERANCE (2) consecutive seconds,
#     and a zone must never be without an answering Service.
#   - Step 6 deletes every BIND9 pod at once: the zones have no nameserver
#     until the replacements load them. The outage must end within
#     ALL_DOWN_RECOVERY_BOUND (120 s); the measured time is reported.
#   - Rollouts (steps 10, 13) are staggered (ADR-0018) and surge a Ready pod
#     before the old one leaves (ADR-0017), so a rolling Service may fail at
#     most ROLLOUT_GAP_TOLERANCE (3) consecutive seconds, and at most one
#     instance of the cluster may be mid-rollout at any sampled second.
#   - Step 3 deletes the secondary's Service with the operator down; its
#     ClusterIP is gone until the operator recreates it (a new ClusterIP).
#
# Order: a fixed pass of the steps, then a second pass in an order shuffled
# from CHAOS_SEED (printed with the order up front; default 20261007). The run
# keeps going after a failed step and prints a results table at the end.
#
# Knobs (env): CHAOS_STEPS=1,4,6 (subset), CHAOS_PASSES=1|2, CHAOS_SEED,
# CONVERGE_TIMEOUT, QUIET_WINDOW, ALSO_NOTIFY_MODE=clusterip|podip, plus the
# bounds above. Flags: --image REF, --skip-deploy (reuse the cluster as is).
#
# Usage: tests/e2e/chaos_test.sh [--image REF] [--skip-deploy]
#        make e2e-chaos [E2E_IMAGE=REF]

# Not -e: the invariant checker and the steps report failures and keep going;
# every command whose failure matters is checked explicitly.
set -uo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-bindy-e2e-chaos}"
# shellcheck disable=SC2034  # read by the libraries sourced below
KUBECTL="kubectl --context kind-${CLUSTER_NAME}"

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/cluster.sh"
source "${LIB_DIR}/chaos.sh"

parse_common_args "$@"

CHAOS_SEED="${CHAOS_SEED:-20261007}"
CHAOS_PASSES="${CHAOS_PASSES:-2}"
RANDOM=${CHAOS_SEED}

if [ -n "${CHAOS_STEPS:-}" ]; then
    IFS=',' read -r -a STEP_LIST <<< "${CHAOS_STEPS}"
else
    STEP_LIST=("${CHAOS_STEP_IDS[@]}")
fi

# The randomized order is drawn first, so it is printed up front and depends
# on the seed alone.
SHUFFLED=("${STEP_LIST[@]}")
for ((i = ${#SHUFFLED[@]} - 1; i > 0; i--)); do
    j=$(( RANDOM % (i + 1) ))
    tmp=${SHUFFLED[i]}; SHUFFLED[i]=${SHUFFLED[j]}; SHUFFLED[j]=${tmp}
done

RESULTS=()
RESULTS_NOTES=()
FAILED_STEPS=0

info "E2E: chaos (cluster '${CLUSTER_NAME}', seed ${CHAOS_SEED}, passes ${CHAOS_PASSES})"
info "  fixed order:      ${STEP_LIST[*]}"
[ "${CHAOS_PASSES}" -ge 2 ] && info "  randomized order: ${SHUFFLED[*]}"
info "  state dir:        ${CHAOS_STATE_DIR}"

# ── Setup ────────────────────────────────────────────────────────────────────

phase "Setting up cluster and operator"
bindy_setup deploy/kind-config-chaos.yaml
${KUBECTL} scale deployment/bindy -n "${NAMESPACE}" --replicas="${OPERATOR_REPLICAS}" >/dev/null
if ! wait_operator_ready "${OPERATOR_REPLICAS}"; then
    fail "operator never reached ${OPERATOR_REPLICAS} ready replicas"
    finish "E2E: chaos" || true
    exit 1
fi
pass "operator running ${OPERATOR_REPLICAS} replicas, leader $(operator_leader)"

phase "Applying the chaos fixture"
# The CRDs were just (re)installed; give the API server a few tries.
for attempt in 1 2 3 4 5; do
    if apply_chaos_cluster 2>/dev/null; then break; fi
    warn "Bind9Cluster apply attempt ${attempt} failed; retrying"
    sleep 5
done
apply_chaos_zone "${CHAOS_ZONE_CR}" "${CHAOS_ZONE}"
apply_chaos_zone "${CHAOS_REVERSE_ZONE_CR}" "${CHAOS_REVERSE_ZONE}"
apply_extra_zone "${CHAOS_CURRENT_EXTRA_ZONE}"
apply_chaos_records
apply_tools_pods
pass "fixture applied; prober running"

# ── Step runner ──────────────────────────────────────────────────────────────

# Allowed consecutive failed seconds for one Service in the current step.
service_bound() {
    local svc=$1
    if ${STEP_ALL_DOWN}; then echo "${ALL_DOWN_RECOVERY_BOUND}"; return; fi
    if [[ " ${STEP_TARGETS} " == *" ${svc} "* ]]; then echo "${TARGET_GAP_BOUND}"; return; fi
    if ${STEP_ROLLOUT}; then echo "${ROLLOUT_GAP_TOLERANCE}"; return; fi
    echo "${UNTOUCHED_GAP_TOLERANCE}"
}

# Everything needed to understand a failed step after the fact, saved under
# the state directory.
dump_step_diagnostics() {
    local dir="${CHAOS_STATE_DIR}/diag-$1" pod inst
    mkdir -p "${dir}"
    cp "${VIOLATIONS_FILE}" "${dir}/violations" 2>/dev/null || true
    cp "${CHAOS_STATE_DIR}/status.json" "${CHAOS_STATE_DIR}/dig-results" "${dir}/" 2>/dev/null || true
    ${KUBECTL} get pods -n "${NAMESPACE}" -o wide > "${dir}/pods.txt" 2>&1 || true
    ${KUBECTL} get dnszones,bind9instances -n "${NAMESPACE}" -o yaml > "${dir}/crs.yaml" 2>&1 || true
    for pod in $(operator_pods); do
        ${KUBECTL} logs -n "${NAMESPACE}" "${pod}" --since=15m > "${dir}/operator-${pod}.log" 2>&1 || true
    done
    for inst in $(all_instances); do
        pod=$(first_pod_of "${inst}")
        [ -n "${pod}" ] || continue
        # shellcheck disable=SC2046
        rndc_report "${pod}" $(zone_names) > "${dir}/rndc-${inst}.txt" 2>&1 || true
        ${KUBECTL} logs -n "${NAMESPACE}" "${pod}" -c bind9 --since=15m > "${dir}/named-${inst}.log" 2>&1 || true
        ${KUBECTL} logs -n "${NAMESPACE}" "${pod}" -c api --since=15m > "${dir}/bindcar-${inst}.log" 2>&1 || true
    done
    warn "diagnostics saved to ${dir}"
}

# $1 = pass label, $2 = step id (0 = baseline)
run_step() {
    local pass_label=$1 id=$2 name start chaos_done end probe gaps="" zone_out="" verdict=PASS l
    name=${CHAOS_STEP_NAMES[${id}]:-baseline}
    STEP_TARGETS=""; STEP_ROLLOUT=false; STEP_ALL_DOWN=false; STEP_FAILED=false; STEP_NOTES=()
    phase "[${pass_label}] step ${id}: ${name}"
    start=$(now)
    remember_operator_restarts
    : > "${VIOLATIONS_FILE}"
    if [ "${id}" != 0 ]; then
        "step_${id}"
    fi
    # Violations a step records itself (a container that did not restart).
    if [ -s "${VIOLATIONS_FILE}" ]; then
        STEP_FAILED=true
        while IFS= read -r l; do STEP_NOTES+=("${l}"); fail "${l}"; done < "${VIOLATIONS_FILE}"
    fi
    chaos_done=$(now)

    if wait_converged "${chaos_done}"; then
        pass "converged ${CONVERGED_SECS}s after the chaos ended"
    else
        STEP_FAILED=true
        fail "not converged within ${CONVERGE_TIMEOUT}s; still violated:"
        sed 's/^/      /' "${VIOLATIONS_FILE}" | head -40
        while IFS= read -r l; do STEP_NOTES+=("${l}"); done < <(head -12 "${VIOLATIONS_FILE}")
        if ! grep -q '^status:' "${VIOLATIONS_FILE}" \
           && grep -qE '^(dns|soa|rndc|acl|notify|primaries):' "${VIOLATIONS_FILE}"; then
            fail "UNTRUTHFUL STATUS: every CR reports Ready while the zones are not correctly served"
            STEP_NOTES+=("UNTRUTHFUL STATUS: every CR Ready while DNS invariants fail")
        fi
        dump_step_diagnostics "${pass_label}-${id}"
    fi
    end=$(now)

    # DNS availability over the whole step. The baseline window covers the
    # fixture coming up, when nothing serves yet: not assessed.
    probe=""
    [ "${id}" != 0 ] && probe=$(analyze_probe "${start}" "${end}")
    local kind n max bound
    while read -r kind n max; do
        [ -n "${kind}" ] || continue
        case "${kind}" in
            svc)
                bound=$(service_bound "${n}")
                gaps+="${n#chaos-}=${max} "
                if [ "${max}" -gt "${bound}" ]; then
                    STEP_FAILED=true
                    fail "Service ${n} failed ${max} consecutive seconds (bound ${bound})"
                    STEP_NOTES+=("Service ${n} down ${max}s > ${bound}s")
                fi
                ;;
            zone)
                zone_out+="${n%%.*}=${max} "
                if ${STEP_ALL_DOWN}; then
                    if [ "${max}" -gt "${ALL_DOWN_RECOVERY_BOUND}" ]; then
                        STEP_FAILED=true
                        fail "zone ${n} had no nameserver for ${max}s (recovery bound ${ALL_DOWN_RECOVERY_BOUND}s)"
                        STEP_NOTES+=("zone ${n} outage ${max}s > ${ALL_DOWN_RECOVERY_BOUND}s")
                    else
                        pass "zone ${n} recovered from losing every nameserver in ${max}s"
                    fi
                elif [ "${max}" -gt 0 ]; then
                    STEP_FAILED=true
                    fail "zone ${n} had NO answering nameserver for ${max} consecutive seconds"
                    STEP_NOTES+=("zone ${n} had no nameserver for ${max}s")
                fi
                ;;
            samples)
                if [ "${n}" -eq 0 ]; then STEP_FAILED=true; fail "prober produced no samples"; fi
                ;;
        esac
    done <<< "${probe}"
    echo "    DNS max consecutive failed seconds per Service: ${gaps}| zone outage seconds: ${zone_out}"

    # Quiet window, only meaningful once converged.
    local quiet="-"
    if [ "${CONVERGED_SECS}" -ge 0 ]; then
        QUIET_RECONCILES=0
        if check_quiet; then
            quiet="${QUIET_RECONCILES}"
            pass "operator quiet for ${QUIET_WINDOW}s (${QUIET_RECONCILES} reconciles)"
        else
            quiet="LOUD"
            STEP_FAILED=true
            fail "operator not quiet after convergence:"
            sed 's/^/      /' "${VIOLATIONS_FILE}"
            dump_step_diagnostics "${pass_label}-${id}-quiet"
            cp "${CHAOS_STATE_DIR}"/quiet-* "${CHAOS_STATE_DIR}/diag-${pass_label}-${id}-quiet/" 2>/dev/null || true
            while IFS= read -r l; do STEP_NOTES+=("${l}"); done < "${VIOLATIONS_FILE}"
        fi
    fi

    if ${STEP_FAILED}; then
        verdict=FAIL
        FAILED_STEPS=$((FAILED_STEPS + 1))
        ERRORS=$((ERRORS + 1))
    fi
    RESULTS+=("${pass_label}|${id}|${name}|${CONVERGED_SECS}|${quiet}|${gaps}|${zone_out}|${verdict}")
    for l in "${STEP_NOTES[@]+"${STEP_NOTES[@]}"}"; do
        RESULTS_NOTES+=("${pass_label} step ${id}: ${l}")
    done
}

print_results() {
    local row p id name conv quiet gaps zones verdict
    echo ""
    info "Chaos results (seed ${CHAOS_SEED})"
    printf '%-8s %-4s %-44s %-6s %-6s %-58s %-26s %s\n' PASS STEP NAME CONV_S QUIET "SVC_MAX_FAIL_S" "ZONE_OUTAGE_S" RESULT
    for row in "${RESULTS[@]}"; do
        IFS='|' read -r p id name conv quiet gaps zones verdict <<< "${row}"
        printf '%-8s %-4s %-44s %-6s %-6s %-58s %-26s %s\n' "${p}" "${id}" "${name:0:44}" "${conv}" "${quiet}" "${gaps}" "${zones}" "${verdict}"
    done
    if [ "${#RESULTS_NOTES[@]}" -gt 0 ]; then
        echo ""
        info "Failure details:"
        printf '  %s\n' "${RESULTS_NOTES[@]}"
    fi
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        {
            echo "## Chaos results (seed ${CHAOS_SEED})"
            echo ""
            echo "| Pass | Step | Name | Converged (s) | Quiet reconciles | Max consecutive failed seconds per Service | Zone outage (s) | Result |"
            echo "|---|---|---|---|---|---|---|---|"
            for row in "${RESULTS[@]}"; do
                IFS='|' read -r p id name conv quiet gaps zones verdict <<< "${row}"
                echo "| ${p} | ${id} | ${name} | ${conv} | ${quiet} | ${gaps} | ${zones} | ${verdict} |"
            done
            echo ""
        } >> "${GITHUB_STEP_SUMMARY}"
    fi
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_step baseline 0

# An operator crash during bring-up is recorded (the baseline row fails) but
# does not stop the run; an unhealthy fixture does, since the chaos steps
# would prove nothing against it.
if [ "${CONVERGED_SECS}" -lt 0 ] && grep -qv '^operator:' "${VIOLATIONS_FILE}"; then
    fail "baseline is not healthy; the chaos steps would prove nothing"
    print_results
    finish "E2E: chaos" || true
    exit 1
fi

for id in "${STEP_LIST[@]}"; do
    run_step fixed "${id}"
done

if [ "${CHAOS_PASSES}" -ge 2 ]; then
    for id in "${SHUFFLED[@]}"; do
        run_step random "${id}"
    done
fi

print_results
summary_row "Chaos: ${#RESULTS[@]} step runs, ${FAILED_STEPS} failed (seed ${CHAOS_SEED})" \
            "$([ "${FAILED_STEPS}" -eq 0 ] && echo 'passed' || echo 'failed')"
finish "E2E: chaos"
