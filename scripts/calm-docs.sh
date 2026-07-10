#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# Generate Mermaid documentation pages from CALM architecture documents.
#
# For each `calm/*.architecture.json` model this renders the top-level
# "block-architecture" Mermaid diagram (via `calm docify`) and writes a
# fully-generated Markdown page under docs/src/architecture/. The pages contain
# ONLY a static banner + the diagram (no prose, no timestamps) so that
# `make calm-docs-check` can regenerate them and assert a clean `git diff`.
#
# Prose/overview lives in the hand-authored docs/src/architecture/calm.md.
#
# Usage: scripts/calm-docs.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CALM_DIR="${PROJECT_ROOT}/calm"
DOCS_ARCH_DIR="${PROJECT_ROOT}/docs/src/architecture"

# Pin the CLI so generated output is reproducible across machines and CI.
CALM_CLI_VERSION="${CALM_CLI_VERSION:-1.47.1}"
CALM="npx --yes @finos/calm-cli@${CALM_CLI_VERSION}"

# model basename (without .architecture.json) -> human title for the page.
title_for() {
    case "$1" in
        bindy-control-plane) echo "Control Plane — Reconcilers, CRDs & Operands" ;;
        bindy-multi-cluster) echo "Multi-Cluster — Queen Bee & Scout Fan-in" ;;
        *) echo "$1" ;;
    esac
}

# The generator emits a Mermaid `init` directive requesting the ELK layout
# engine, which is a separate package not bundled with the MkDocs mermaid.min.js.
# Rewrite it to the built-in `dagre` engine so the diagram renders everywhere.
normalize_layout() {
    sed 's/"layout": "elk"/"layout": "dagre"/'
}

# Extract the first fenced ```mermaid block (inclusive) from stdin.
extract_mermaid() {
    awk '
        /^```mermaid$/ { f=1; print; next }
        f && /^```$/   { print; exit }
        f              { print }
    '
}

generated_any=false
for model in "${CALM_DIR}"/*.architecture.json; do
    [ -e "${model}" ] || { echo "No CALM models found in ${CALM_DIR}" >&2; exit 1; }
    base="$(basename "${model}" .architecture.json)"
    title="$(title_for "${base}")"
    out="${DOCS_ARCH_DIR}/calm-${base#bindy-}.md"
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' EXIT

    echo "==> docify ${base} -> ${out#${PROJECT_ROOT}/}"
    ${CALM} docify -a "${model}" -o "${tmp}/site" >/dev/null

    mermaid="$(extract_mermaid < "${tmp}/site/docs/index.md" | normalize_layout)"
    if [ -z "${mermaid}" ]; then
        echo "ERROR: no mermaid diagram produced for ${base}" >&2
        exit 1
    fi

    {
        echo "<!--"
        echo "  GENERATED FILE — DO NOT EDIT."
        echo "  Source: calm/${base}.architecture.json"
        echo "  Regenerate with: make calm-docs"
        echo "-->"
        echo
        echo "# ${title}"
        echo
        echo "> Auto-generated from [\`calm/${base}.architecture.json\`](https://github.com/firestoned/bindy/blob/main/calm/${base}.architecture.json)"
        echo "> via \`make calm-docs\`. Edit the CALM model, not this page."
        echo
        echo "${mermaid}"
    } > "${out}"

    generated_any=true
    rm -rf "${tmp}"; trap - EXIT
done

${generated_any} && echo "✓ CALM docs generated."
