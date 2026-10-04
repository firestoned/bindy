#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

CLUSTER_NAME="bindy-test"

echo -e "${YELLOW}🗑️  Cleaning up Kind cluster: ${CLUSTER_NAME}${NC}"

if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    kind delete cluster --name "${CLUSTER_NAME}"
    echo -e "${GREEN}✅ Cluster deleted successfully${NC}"
else
    echo -e "${YELLOW}⚠️  Cluster '${CLUSTER_NAME}' not found${NC}"
fi

echo -e "${GREEN}🧹 Cleanup complete!${NC}"
