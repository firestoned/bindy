#!/usr/bin/env bash
set -euo pipefail

# Force delete Kubernetes namespaces stuck in Terminating state
# Usage: ./force-delete-ns.sh <namespace1> [namespace2] [namespace3] ...

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

if [[ $# -lt 1 ]]; then
    echo -e "${RED}Usage: $0 <namespace1> [namespace2] [namespace3] ...${NC}"
    exit 1
fi

NAMESPACES=("$@")
FAILED=()
SUCCEEDED=()

delete_namespace() {
    local ns="$1"
    
    echo -e "\n${YELLOW}════════════════════════════════════════${NC}"
    echo -e "${YELLOW}Processing namespace: ${ns}${NC}"
    echo -e "${YELLOW}════════════════════════════════════════${NC}"

    # Check if namespace exists
    if ! kubectl get namespace "$ns" &>/dev/null; then
        echo -e "${RED}Namespace '$ns' not found - skipping${NC}"
        FAILED+=("$ns (not found)")
        return 1
    fi

    # Check if namespace is actually in Terminating state
    STATUS=$(kubectl get namespace "$ns" -o jsonpath='{.status.phase}')
    if [[ "$STATUS" != "Terminating" ]]; then
        echo -e "${YELLOW}Warning: Namespace '$ns' is in '$STATUS' state, not Terminating${NC}"
        read -p "Continue anyway? (y/N): " confirm
        if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
            FAILED+=("$ns (skipped)")
            return 1
        fi
    fi

    echo -e "${YELLOW}Checking for stuck resources in '$ns'...${NC}"
    kubectl api-resources --verbs=list --namespaced -o name 2>/dev/null | \
        xargs -I {} kubectl get {} -n "$ns" --ignore-not-found 2>/dev/null | \
        grep -v "^$" || true

    echo ""
    echo -e "${YELLOW}Removing finalizers from namespace '$ns'...${NC}"

    if kubectl get namespace "$ns" -o json | \
        jq '.spec.finalizers = []' | \
        kubectl replace --raw "/api/v1/namespaces/$ns/finalize" -f - >/dev/null; then
        
        # Verify
        sleep 1
        if kubectl get namespace "$ns" &>/dev/null; then
            echo -e "${YELLOW}Namespace '$ns' still exists - may need manual intervention${NC}"
            FAILED+=("$ns (still exists)")
        else
            echo -e "${GREEN}Namespace '$ns' successfully removed${NC}"
            SUCCEEDED+=("$ns")
        fi
    else
        echo -e "${RED}Failed to remove finalizers from '$ns'${NC}"
        FAILED+=("$ns (finalizer removal failed)")
    fi
}

# Process each namespace
for ns in "${NAMESPACES[@]}"; do
    delete_namespace "$ns" || true
done

# Summary
echo -e "\n${YELLOW}════════════════════════════════════════${NC}"
echo -e "${YELLOW}SUMMARY${NC}"
echo -e "${YELLOW}════════════════════════════════════════${NC}"
echo -e "${GREEN}Succeeded (${#SUCCEEDED[@]}):${NC} ${SUCCEEDED[*]:-none}"
echo -e "${RED}Failed (${#FAILED[@]}):${NC} ${FAILED[*]:-none}"
