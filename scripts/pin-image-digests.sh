#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
# Pin container image digests for reproducible builds (M-1)
#
# Fetches multi-arch manifest list digests and updates all Dockerfiles.
# Uses `docker buildx imagetools inspect --raw | sha256sum` to get the
# correct multi-arch manifest list digest (NOT platform-specific).
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

# Get multi-arch manifest list digest (NOT platform-specific).
# docker manifest inspect gives platform-specific digests — wrong.
# docker buildx imagetools inspect --raw gives the manifest list JSON,
# and sha256sum of that JSON is the correct multi-arch digest.
get_multiarch_digest() {
  local image="$1"
  echo "Fetching multi-arch digest for: $image" >&2

  local digest
  digest=$(docker buildx imagetools inspect "$image" --raw 2>/dev/null | sha256sum | awk '{print "sha256:"$1}')

  # sha256 of empty string — means the inspect returned nothing
  local empty_sha="sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
  if [[ -z "$digest" || "$digest" == "$empty_sha" ]]; then
    echo "ERROR: Could not fetch digest for $image" >&2
    return 1
  fi

  echo "$digest"
}

# Update digest in all Dockerfiles that reference the given image
update_digest() {
  local image="$1"
  local new_digest="$2"
  local any_changed=false

  for dockerfile in docker/Dockerfile docker/Dockerfile.chainguard docker/Dockerfile.fast; do
    [[ -f "$dockerfile" ]] || continue
    grep -q "FROM ${image}@sha256:" "$dockerfile" || continue

    local old_digest
    old_digest=$(grep "FROM ${image}@sha256:" "$dockerfile" | sed 's/.*@\(sha256:[a-f0-9]*\).*/\1/' | head -1)

    if [[ "$old_digest" == "$new_digest" ]]; then
      echo "   ↔ $dockerfile: unchanged"
      continue
    fi

    echo "   ✅ $dockerfile: ${old_digest:7:12}... → ${new_digest:7:12}..."
    if [[ "$DRY_RUN" == false ]]; then
      sed -i.bak "s|FROM ${image}@sha256:[a-f0-9]*|FROM ${image}@${new_digest}|g" "$dockerfile"
      rm -f "${dockerfile}.bak"
    fi
    any_changed=true
  done

  echo "$any_changed"
}

echo "============================================"
echo "Pin Container Image Digests (Nightly)"
echo "============================================"
echo ""

# Images to update: must match exactly what appears in FROM lines
IMAGES=(
  "debian:12-slim"
  "gcr.io/distroless/cc-debian12:nonroot"
  "cgr.dev/chainguard/wolfi-base:latest"
  "cgr.dev/chainguard/glibc-dynamic:latest"
  "rust:1.94.0"
  "alpine:3.21"
)

FAILED=()
ANY_CHANGED=false

for image in "${IMAGES[@]}"; do
  echo "Processing: $image"
  if digest=$(get_multiarch_digest "$image"); then
    result=$(update_digest "$image" "$digest")
    [[ "$result" == "true" ]] && ANY_CHANGED=true
  else
    echo "   ❌ Failed to fetch digest"
    FAILED+=("$image")
  fi
  echo ""
done

echo "============================================"
echo "Summary"
echo "============================================"

if [[ ${#FAILED[@]} -gt 0 ]]; then
  echo "Failed images:"
  for f in "${FAILED[@]}"; do
    echo "  - $f"
  done
  exit 1
fi

if [[ "$ANY_CHANGED" == false ]]; then
  echo "✓ All digests are already up to date"
elif [[ "$DRY_RUN" == true ]]; then
  echo "🔍 DRY RUN COMPLETE - No files were modified"
else
  echo "✅ Digests updated. Review with: git diff docker/"
fi
