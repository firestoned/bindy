#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: Apache-2.0
#
# SBOM post-processing and quality gate (ADR-0010).
#
#   scripts/sbom.sh annotate <sbom.cdx.json>   add producer metadata if absent
#   scripts/sbom.sh check    <sbom.cdx.json>   enforce NTIA minimum elements
#
# Works on CycloneDX JSON from cargo-cyclonedx (binaries) and Syft (images).
# `annotate` runs before `check`, and both run before the SBOM is attested,
# so the attested bytes are the checked bytes.

set -euo pipefail

readonly MIN_SPEC_MAJOR=1
readonly MIN_SPEC_MINOR=5
readonly SUPPLIER_NAME="firestoned"
readonly SUPPLIER_URL="https://github.com/firestoned/bindy"

usage() {
    echo "usage: $0 {annotate|check} <sbom.cdx.json>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
cmd="$1"
sbom="$2"
[ -f "$sbom" ] || { echo "ERROR: SBOM not found: $sbom" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq is required" >&2; exit 1; }

annotate() {
    # Syft records no SBOM author or supplier; cargo-cyclonedx records the
    # crate authors but no supplier. We are the producer of every SBOM we
    # ship, so say so, without overwriting anything the tool did record.
    local tmp
    tmp="$(mktemp)"
    jq --arg name "$SUPPLIER_NAME" --arg url "$SUPPLIER_URL" '
        .metadata.supplier //= {name: $name, url: [$url]}
        | .metadata.authors //= [{name: $name}]
        | if (.metadata.authors | length) == 0 then .metadata.authors = [{name: $name}] else . end
    ' "$sbom" > "$tmp"
    mv "$tmp" "$sbom"
    echo "annotated: $sbom"
}

check() {
    local failures
    # Each rule yields a message when it fails; the SBOM passes when none do.
    failures="$(jq -r --argjson maj "$MIN_SPEC_MAJOR" --argjson min "$MIN_SPEC_MINOR" '
        def is_package: (.type // "library") | IN("library", "application", "framework");
        def spec_ok: (.specVersion // "0.0" | split(".") | map(tonumber)) as $v
            | ($v[0] > $maj) or ($v[0] == $maj and $v[1] >= $min);
        def tools_present:
            .metadata.tools as $t
            | if $t == null then false
              elif ($t | type) == "array" then ($t | length) > 0
              else (($t.components // []) + ($t.services // []) | length) > 0
              end;
        [
          (if .bomFormat != "CycloneDX" then "bomFormat is not CycloneDX" else empty end),
          (if spec_ok | not then "specVersion \(.specVersion) is below \($maj).\($min)" else empty end),
          (if (.serialNumber // "") == "" then "no serialNumber" else empty end),
          (if (.metadata.timestamp // "") == "" then "no metadata.timestamp" else empty end),
          (if tools_present | not then "no metadata.tools (generating tool)" else empty end),
          (if ((.metadata.authors // []) | length) == 0 and .metadata.supplier == null
             then "no SBOM author (metadata.authors or metadata.supplier)" else empty end),
          (if (.metadata.component.name // "") == "" then "root component has no name" else empty end),
          (if (.metadata.component.version // "") == "" then "root component has no version" else empty end),
          (if (.metadata.component.type // "") != "container" and (.metadata.component.purl // "") == ""
             then "root component has no purl" else empty end),
          (.components // [] | map(select((.name // "") == "")) | length
             | if . > 0 then "\(.) component(s) without a name" else empty end),
          (.components // [] | map(select(is_package and (.version // "") == "")) | length
             | if . > 0 then "\(.) package component(s) without a version" else empty end),
          (.components // [] | map(select(is_package and (.purl // "") == "")) | length
             | if . > 0 then "\(.) package component(s) without a purl" else empty end),
          (if ((.dependencies // []) | length) == 0 then "no dependency graph" else empty end)
        ] | .[]
    ' "$sbom")"

    # Supplier data comes from upstream package metadata, which registries
    # such as crates.io do not require: report coverage, do not fail on it.
    jq -r '
        def is_package: (.type // "library") | IN("library", "application", "framework");
        [.components // [] | .[] | select(is_package)] as $p
        | ($p | map(select(.supplier != null or .author != null or .publisher != null)) | length) as $s
        | "\(input_filename): CycloneDX \(.specVersion), \($p | length) packages, \($s) with supplier/author/publisher"
    ' "$sbom"

    if [ -n "$failures" ]; then
        echo "ERROR: $sbom fails the SBOM quality gate:" >&2
        while IFS= read -r line; do echo "  - $line" >&2; done <<< "$failures"
        exit 1
    fi
    echo "OK: $sbom meets the NTIA minimum elements"
}

case "$cmd" in
    annotate) annotate ;;
    check) check ;;
    *) usage ;;
esac
