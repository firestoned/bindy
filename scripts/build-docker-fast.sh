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

# Parse command line arguments
NO_CACHE=""
STRATEGY=""
TAG=""

# Process arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --no-cache)
            NO_CACHE="--no-cache"
            shift
            ;;
        --help|-h)
            STRATEGY="--help"
            shift
            ;;
        *)
            if [ -z "$STRATEGY" ]; then
                STRATEGY="$1"
            elif [ -z "$TAG" ]; then
                TAG="$1"
            fi
            shift
            ;;
    esac
done

# Default values
STRATEGY="${STRATEGY:-local}"
# Use a distinct tag for local dev builds so kind load never conflicts with a
# registry-pulled image that shares the same tag.  Containerd keeps the existing
# tag when an image with that name is already present from the registry, so
# "latest" would silently leave the registry image in place.
TAG="${TAG:-local-dev}"
IMAGE_NAME=firestoned/bindy
REGISTRY="${REGISTRY:-ghcr.io}"
KIND_CLUSTER="${KIND_CLUSTER:-bindy-test}"
FULL_IMAGE="${REGISTRY}/${IMAGE_NAME}:${TAG}"

# Derive the Linux cross-compilation target from the host architecture.
# The binary is built by `make build-linux-debug` and passed to Docker as a build arg
# so Dockerfile.local picks up the right arch binary without hardcoding any path.
HOST_ARCH="$(uname -m)"
if [ "$HOST_ARCH" = "arm64" ]; then
    LINUX_TARGET="aarch64-unknown-linux-gnu"
else
    LINUX_TARGET="x86_64-unknown-linux-gnu"
fi
BINARY_PATH="target/${LINUX_TARGET}/debug/bindy"

print_usage() {
    echo "Usage: $0 [options] [strategy] [tag]"
    echo ""
    echo "Options:"
    echo "  --no-cache    - Build without using Docker cache (useful for GPG signature issues)"
    echo ""
    echo "Strategies:"
    echo "  local     - Build locally then copy binary (fastest: ~10s)"
    echo "  kind      - Build locally, copy binary, and load into kind (fastest: ~15s)"
    echo "  fast      - Use Dockerfile.fast with better caching (~1-2min)"
    echo "  ci        - Use production Dockerfile with pre-built binaries (~30s, requires binaries/)"
    echo ""
    echo "Examples:"
    echo "  $0 local              # Fastest, builds locally first"
    echo "  $0 kind               # Build locally and load into kind cluster"
    echo "  $0 --no-cache local   # Build without cache (fixes GPG signature errors)"
    echo "  $0 fast               # Fast Docker build with caching"
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
if [ -n "$NO_CACHE" ]; then
    echo -e "${YELLOW}Cache: Disabled (--no-cache)${NC}"
fi
echo ""

case "$STRATEGY" in
    local)
        echo -e "${YELLOW}Strategy: Local build (fastest) [arch: ${HOST_ARCH} → ${LINUX_TARGET}]${NC}"
        echo "Step 1/2: Building Linux binary locally with cargo..."
        make build-linux-debug
        echo ""
        echo "Step 2/2: Building Docker image..."
        # Use the binary's own directory as the build context — avoids walking the repo root
        docker build --pull $NO_CACHE \
            -f docker/Dockerfile.local -t "$FULL_IMAGE" "$(dirname "${BINARY_PATH}")"
        ;;

    kind)
        echo -e "${YELLOW}Strategy: Local build for kind (fastest) [arch: ${HOST_ARCH} → ${LINUX_TARGET}]${NC}"
        echo "Step 1/3: Building Linux binary locally with cargo..."
        make build-linux-debug
        echo ""
        echo "Step 2/3: Building Docker image..."
        # Use the binary's own directory as the build context — avoids walking the repo root
        docker build --pull $NO_CACHE \
            -f docker/Dockerfile.local -t "$FULL_IMAGE" "$(dirname "${BINARY_PATH}")"
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
        docker build --pull $NO_CACHE -f docker/Dockerfile -t "$FULL_IMAGE" .
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
