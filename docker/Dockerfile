# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT

# PRODUCTION DOCKERFILE - Uses pre-built binaries from GitHub Actions
# This Dockerfile is optimized for multi-architecture builds in CI/CD pipelines.
# It uses pre-built glibc binaries (much faster to build than musl) and supports
# both linux/amd64 and linux/arm64 platforms via Docker BuildKit.
#
# Build time: ~30 seconds (just copies pre-built binaries)
#
# Base image: Google Distroless (glibc-based, ~20MB, minimal attack surface)
#
# Usage in CI/CD:
#   1. Build binaries for both architectures using cargo (GNU targets)
#   2. Copy binaries to ./binaries/amd64/ and ./binaries/arm64/
#   3. Build multi-arch image:
#      docker buildx build --platform linux/amd64,linux/arm64 \
#        -t registry/image:tag .
#
# The TARGETARCH build argument is automatically set by Docker BuildKit
# to match the target platform (amd64 or arm64).

# Build stage to install BIND9 and create user
# NOTE: This digest points to the multi-arch manifest list (supports AMD64, ARM64, etc.)
FROM debian:12-slim@sha256:56ff6d36d4eb3db13a741b342ec466f121480b5edded42e4b7ee850ce7a418ee AS builder

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y \
    bind9 \
    bind9utils \
    && rm -rf /var/lib/apt/lists/*

# Runtime stage - Distroless with glibc
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:d96da2e1d6ced4ca6dc94df53e95ce8e333d5e5c6db69c8106fe9ab61fb51d4b

ARG TARGETARCH
ARG VERSION=0.1.0

LABEL org.opencontainers.image.source="https://github.com/firestoned/bindy"
LABEL org.opencontainers.image.description="BIND9 DNS Operator for Kubernetes"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.version="${VERSION}"

# Copy BIND9 binaries from builder
# Note: We only copy the essential BIND9 server and control binaries.
# DNS query tools (dig, nslookup, host) are not needed for the operator.
COPY --from=builder /usr/sbin/named /usr/sbin/named
COPY --from=builder /usr/sbin/rndc /usr/sbin/rndc
COPY --from=builder /usr/sbin/rndc-confgen /usr/sbin/rndc-confgen

# Copy required libraries
COPY --from=builder /lib/*-linux-gnu* /lib/
COPY --from=builder /usr/lib/*-linux-gnu* /usr/lib/

# Create directories (distroless runs as nonroot by default)
# Note: Distroless doesn't have mkdir, so we copy from builder
COPY --from=builder --chown=nonroot:nonroot /etc/bind /etc/bind

# Copy the pre-built binary for the target architecture
# TARGETARCH is automatically set by BuildKit (amd64 or arm64)
# Map docker architectures to our binary paths:
# - amd64 → binaries/amd64/bindy (x86_64-unknown-linux-gnu)
# - arm64 → binaries/arm64/bindy (aarch64-unknown-linux-gnu)
COPY --chmod=755 binaries/${TARGETARCH}/bindy /usr/local/bin/bindy

# Distroless runs as nonroot user (UID 65532) by default
# No need to specify USER

# Start the operator
ENTRYPOINT ["/usr/local/bin/bindy"]
