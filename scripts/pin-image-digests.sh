#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
# Pin container image digests for reproducible builds (M-1)
#
# This script fetches the current digests for all base images used in Dockerfiles
# and updates the Dockerfiles to pin them. This ensures reproducible builds.
#
# Usage:
#   ./scripts/pin-image-digests.sh [--dry-run]
#
# Options:
#   --dry-run    Show what would be changed without modifying files

set -euo pipefail

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=true
  echo "🔍 DRY RUN MODE - No files will be modified"
  echo ""
fi

# Function to get image digest
get_digest() {
  local image="$1"
  echo "Fetching digest for: $image" >&2

  # Try docker manifest first (faster)
  if command -v docker >/dev/null 2>&1; then
    digest=$(docker manifest inspect "$image" 2>/dev/null | jq -r '.config.digest // .manifests[0].digest' || echo "")
    if [ -n "$digest" ] && [ "$digest" != "null" ]; then
      echo "$digest"
      return 0
    fi
  fi

  # Fallback to skopeo (if available)
  if command -v skopeo >/dev/null 2>&1; then
    digest=$(skopeo inspect "docker://$image" 2>/dev/null | jq -r '.Digest' || echo "")
    if [ -n "$digest" ] && [ "$digest" != "null" ]; then
      echo "$digest"
      return 0
    fi
  fi

  # Fallback to crane (if available)
  if command -v crane >/dev/null 2>&1; then
    digest=$(crane digest "$image" 2>/dev/null || echo "")
    if [ -n "$digest" ]; then
      echo "$digest"
      return 0
    fi
  fi

  echo "ERROR: Could not fetch digest for $image" >&2
  echo "Install docker, skopeo, or crane" >&2
  return 1
}

# Function to pin digest in Dockerfile
pin_digest() {
  local dockerfile="$1"
  local image="$2"
  local digest="$3"

  echo "📌 Pinning $image to digest $digest"

  if [ "$DRY_RUN" = true ]; then
    echo "   Would update: $dockerfile"
    echo "   FROM $image"
    echo "   TO:   FROM $image@$digest"
    echo ""
    return 0
  fi

  # Check if image already has a digest pinned
  if grep -q "FROM ${image}@sha256:" "$dockerfile"; then
    echo "   Updating existing digest in $dockerfile"
    # Update existing digest
    sed -i.bak "s|FROM ${image}@sha256:[a-f0-9]*|FROM ${image}@${digest}|g" "$dockerfile"
  else
    echo "   Adding digest to $dockerfile"
    # Add digest to unpinned image
    sed -i.bak "s|FROM ${image}|FROM ${image}@${digest}|g" "$dockerfile"
  fi

  # Remove backup file
  rm -f "${dockerfile}.bak"
  echo "   ✅ Updated $dockerfile"
  echo ""
}

echo "============================================"
echo "Pin Container Image Digests (M-1)"
echo "============================================"
echo ""

# Array of images to pin: "dockerfile:image"
IMAGES_TO_PIN=(
  "docker/Dockerfile:debian:12-slim"
  "docker/Dockerfile:gcr.io/distroless/cc-debian12:nonroot"
  "docker/Dockerfile.chainguard:cgr.dev/chainguard/wolfi-base:latest"
  "docker/Dockerfile.chainguard:cgr.dev/chainguard/glibc-dynamic:latest"
  "docker/Dockerfile.chef:rust:1.91.0"
  "docker/Dockerfile.chef:alpine:3.20"
  "docker/Dockerfile.fast:rust:1.91.0"
  "docker/Dockerfile.fast:alpine:3.20"
  "docker/Dockerfile.local:alpine:3.20"
)

FAILED=()
UPDATED=0

for entry in "${IMAGES_TO_PIN[@]}"; do
  IFS=':' read -r dockerfile_image <<< "$entry"
  IFS=':' read -r dockerfile image_name image_tag <<< "$dockerfile_image"

  image="${image_name}:${image_tag}"

  echo "Processing: $image (in $dockerfile)"

  # Get digest
  if digest=$(get_digest "$image"); then
    pin_digest "$dockerfile" "$image" "$digest"
    UPDATED=$((UPDATED + 1))
  else
    echo "❌ Failed to fetch digest for $image"
    FAILED+=("$image")
  fi
done

echo ""
echo "============================================"
echo "Summary"
echo "============================================"
echo "Updated: $UPDATED images"
echo "Failed:  ${#FAILED[@]} images"

if [ ${#FAILED[@]} -gt 0 ]; then
  echo ""
  echo "Failed images:"
  for failed in "${FAILED[@]}"; do
    echo "  - $failed"
  done
  exit 1
fi

if [ "$DRY_RUN" = true ]; then
  echo ""
  echo "🔍 DRY RUN COMPLETE - No files were modified"
  echo "Run without --dry-run to apply changes"
else
  echo ""
  echo "✅ All images pinned successfully"
  echo ""
  echo "Next steps:"
  echo "1. Review changes: git diff docker/"
  echo "2. Test builds:    make docker-build"
  echo "3. Commit changes: git add docker/ && git commit -m 'Pin container image digests (M-1)'"
fi
