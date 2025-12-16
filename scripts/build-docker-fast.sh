#!/bin/bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# Fast Docker build script for local development
# This script provides multiple build strategies optimized for speed

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default values
STRATEGY="${1:-local}"
TAG="${2:-latest}"
IMAGE_NAME=firestoned/bindy
REGISTRY="${REGISTRY:-ghcr.io}"
FULL_IMAGE="${REGISTRY}/${IMAGE_NAME}:${TAG}"

print_usage() {
    echo "Usage: $0 [strategy] [tag]"
    echo ""
    echo "Strategies:"
    echo "  local     - Build locally then copy binary (fastest: ~10s)"
    echo "  kind      - Build locally for kind then copy binary (fastest: ~10s)"
    echo "  fast      - Use Dockerfile.fast with better caching (~1-2min)"
    echo "  chef      - Use cargo-chef for optimal caching (first: ~5min, subsequent: ~30s)"
    echo "  ci        - Use production Dockerfile with pre-built binaries (~30s, requires binaries/)"
    echo ""
    echo "Examples:"
    echo "  $0 local              # Fastest, builds locally first"
    echo "  $0 fast               # Fast Docker build with caching"
    echo "  $0 chef               # Best for repeated builds"
    echo "  $0 ci                 # Production build (requires binaries/amd64/ and binaries/arm64/)"
    echo ""
    echo "Environment variables:"
    echo "  REGISTRY - Docker registry (default: ghcr.io)"
}

if [[ "$STRATEGY" == "--help" ]] || [[ "$STRATEGY" == "-h" ]]; then
    print_usage
    exit 0
fi

echo -e "${GREEN}Building Docker image with strategy: ${STRATEGY}${NC}"
echo -e "${GREEN}Image: ${FULL_IMAGE}${NC}"
echo ""

case "$STRATEGY" in
    local)
        echo -e "${YELLOW}Strategy: Local build (fastest)${NC}"
        echo "Step 1/2: Building binary locally with cargo..."
        cargo build --release
        echo ""
        echo "Step 2/2: Building Docker image..."
        docker build -f docker/Dockerfile.local -t "$FULL_IMAGE" .
        ;;

    kind)
        echo -e "${YELLOW}Strategy: Local build (fastest)${NC}"
        echo "Step 1/2: Building binary locally with cargo..."
        cargo build --release
        echo ""
        echo "Step 2/2: Building Docker image..."
        docker build -f docker/Dockerfile.local -t "$FULL_IMAGE" .
        ;;

    fast)
        echo -e "${YELLOW}Strategy: Fast (optimized Dockerfile)${NC}"
        docker build -f docker/Dockerfile.fast -t "$FULL_IMAGE" .
        ;;

    chef)
        echo -e "${YELLOW}Strategy: Cargo-chef (best caching)${NC}"
        echo "Note: First build will be slow (~5min), subsequent builds are fast (~30s)"
        docker build -f docker/Dockerfile.chef -t "$FULL_IMAGE" .
        ;;

    ci)
        echo -e "${YELLOW}Strategy: Production (uses pre-built binaries)${NC}"
        echo "Note: Requires binaries in binaries/amd64/ and binaries/arm64/"
        if [ ! -f "binaries/amd64/bindy" ] || [ ! -f "binaries/arm64/bindy" ]; then
            echo -e "${RED}ERROR: Pre-built binaries not found!${NC}"
            echo "This strategy requires:"
            echo "  - binaries/amd64/bindy"
            echo "  - binaries/arm64/bindy"
            echo ""
            echo "Build binaries first with:"
            echo "  cargo build --release --target x86_64-unknown-linux-gnu"
            echo "  cross build --release --target aarch64-unknown-linux-gnu"
            echo "  mkdir -p binaries/amd64 binaries/arm64"
            echo "  cp target/x86_64-unknown-linux-gnu/release/bindy binaries/amd64/"
            echo "  cp target/aarch64-unknown-linux-gnu/release/bindy binaries/arm64/"
            exit 1
        fi
        docker build -f docker/Dockerfile -t "$FULL_IMAGE" .
        ;;

    *)
        echo -e "${RED}Error: Unknown strategy '$STRATEGY'${NC}"
        echo ""
        print_usage
        exit 1
        ;;
esac

echo ""
echo -e "${GREEN}✓ Build complete!${NC}"
echo "Image: $FULL_IMAGE"
echo ""
echo "Next steps:"
echo "  docker run --rm $FULL_IMAGE --version"
echo "  docker push $FULL_IMAGE"
echo ""
