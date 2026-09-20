#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# Shared plumbing for the bash e2e suites in tests/e2e/. Sourced, never run.
#
# Every suite is a standalone program: it owns a kind cluster, sets up what it
# needs, asserts one thing, and exits non-zero on the first genuine failure it
# can still report around. Splitting the old 947-line integration_test.sh this
# way is what lets each suite be its own Makefile target and its own CI job.

# Guard against double-sourcing (a suite may pull in several libs that each
# want common.sh).
[ -n "${_BINDY_COMMON_SH:-}" ] && return 0
_BINDY_COMMON_SH=1

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

# These are consumed by the suites that source this file, which shellcheck
# cannot follow through the dynamic `source` path below.
# shellcheck disable=SC2034
LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TESTS_DIR="$(cd "${LIB_DIR}/.." && pwd)"
# shellcheck disable=SC2034
PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"

NAMESPACE="${NAMESPACE:-bindy-system}"

# Failures seen so far. Assertions increment it rather than exiting, so one run
# reports every broken thing instead of only the first.
ERRORS=0

# Rows for the GitHub Actions step summary, as "check|result" pairs.
SUMMARY_ROWS=()

# ── Output ───────────────────────────────────────────────────────────────────

phase() { echo ""; echo -e "${GREEN}▶ $*${NC}"; }
step()  { echo -e "${YELLOW}  … $*${NC}"; }
pass()  { echo -e "  ${GREEN}✓${NC} $*"; }
warn()  { echo -e "  ${YELLOW}⚠${NC}  $*"; }
info()  { echo -e "${BLUE}$*${NC}"; }

# Records a failure and keeps going. The assignment form of the increment is
# deliberate: `((ERRORS++))` returns 1 when ERRORS is 0, which `set -e` kills.
fail() {
    echo -e "  ${RED}✗${NC} $*"
    ERRORS=$((ERRORS + 1))
}

# ── Argument parsing ─────────────────────────────────────────────────────────

# Options every suite accepts. Suites that need more parse them before calling
# this with the remaining arguments.
IMAGE_REF="${IMAGE_REF:-}"
# shellcheck disable=SC2034  # read by tests/lib/cluster.sh
SKIP_DEPLOY=false

# shellcheck disable=SC2034  # SKIP_DEPLOY is read by tests/lib/cluster.sh
parse_common_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --image)       IMAGE_REF="$2"; shift 2 ;;
            --skip-deploy) SKIP_DEPLOY=true; shift ;;
            *)
                echo -e "${RED}Unknown option: $1${NC}" >&2
                echo "Usage: $(basename "$0") [--image IMAGE_REF] [--skip-deploy]" >&2
                exit 1
                ;;
        esac
    done
}

# ── Result reporting ─────────────────────────────────────────────────────────

summary_row() { SUMMARY_ROWS+=("$1|$2"); }

# Print the suite's verdict, render the GitHub step summary when running in
# Actions, and exit 0/1. Call this as the last line of every suite.
finish() {
    local title=$1 row check result

    if [ -n "${GITHUB_STEP_SUMMARY:-}" ] && [ ${#SUMMARY_ROWS[@]} -gt 0 ]; then
        {
            echo "## ${title}"
            echo ""
            echo "| Check | Result |"
            echo "|---|---|"
            for row in "${SUMMARY_ROWS[@]}"; do
                IFS='|' read -r check result <<< "${row}"
                echo "| ${check} | ${result} |"
            done
            echo ""
        } >> "$GITHUB_STEP_SUMMARY"
    fi

    echo ""
    if [ "${ERRORS}" -eq 0 ]; then
        echo -e "${GREEN}✅ ${title}: passed${NC}"
        return 0
    fi

    echo -e "${RED}❌ ${title}: ${ERRORS} failure(s)${NC}"
    echo ""
    echo -e "${YELLOW}Operator logs (last 30 lines):${NC}"
    ${KUBECTL} logs -n "${NAMESPACE}" -l app=bindy --tail=30 2>/dev/null || true
    return 1
}
