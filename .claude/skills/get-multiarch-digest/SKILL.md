---
name: get-multiarch-digest
description: Get the multi-arch manifest-list digest for a Docker base image (NOT a platform-specific digest, which breaks ARM64 with QEMU errors). Use before pinning or updating any base image digest in the Dockerfiles.
---

# get-multiarch-digest

Base images are pinned to **multi-arch manifest list** digests. A
platform-specific digest causes QEMU errors on ARM64 builds. The manifest-list
digest is the `sha256sum` of the raw index JSON.

## Preferred: repo tooling

```bash
make update-image-digests-dry-run   # show what would change
make update-image-digests           # rewrite all Dockerfiles (wraps scripts/pin-image-digests.sh)
```

## Manual: single image

```bash
docker buildx imagetools inspect <image>:<tag> --raw | sha256sum | awk '{print "sha256:"$1}'

# Examples:
docker buildx imagetools inspect debian:13-slim --raw | sha256sum | awk '{print "sha256:"$1}'
docker buildx imagetools inspect gcr.io/distroless/cc-debian13:nonroot --raw | sha256sum | awk '{print "sha256:"$1}'
```

Use in Dockerfiles as:

```dockerfile
# NOTE: This digest points to the multi-arch manifest list (supports both AMD64 and ARM64)
FROM debian:13-slim@sha256:<digest> AS builder
```

Update ALL Dockerfiles that use the same base image: `docker/Dockerfile`,
`docker/Dockerfile.chainguard`, `docker/Dockerfile.chef`,
`docker/Dockerfile.fast` (`docker/Dockerfile.local` usually has no digest).

Do NOT build or push images — that is the user's operation.

## Verification

```bash
docker buildx imagetools inspect <image>@<digest>
# Output must show BOTH: Platform: linux/amd64 AND Platform: linux/arm64
```
