#!/usr/bin/env bash
# Copyright (c) 2025 Erick Bourgeois, firestoned
# SPDX-License-Identifier: MIT
#
# CBOM generation and quality gate (ADR-0011, roadmap 28 Phase 0).
#
#   scripts/cbom.sh generate <out.cdx.json>   stamp the curated template with build facts
#   scripts/cbom.sh check    <cbom.cdx.json>  enforce the CBOM quality gate
#
# The template (cbom/bindy-cbom.template.cdx.json) is the curated inventory;
# `generate` injects only facts of this build: a fresh serial number, the
# timestamp, the release version, and the version of every pkg:cargo/*
# library component read from Cargo.lock. A library in the template that is
# missing from Cargo.lock fails the generation: that is the drift alarm for
# a crypto dependency being dropped or renamed without a template update.
# `generate` runs before `check`, and both run before the document ships,
# so the shipped bytes are the checked bytes.

set -euo pipefail

readonly TEMPLATE="cbom/bindy-cbom.template.cdx.json"
readonly LOCKFILE="Cargo.lock"
readonly ROOT_PACKAGE="bindy"
readonly MIN_SPEC_MAJOR=1
readonly MIN_SPEC_MINOR=6

usage() {
    echo "usage: $0 {generate|check} <cbom.cdx.json>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
cmd="$1"
cbom="$2"
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq is required" >&2; exit 1; }

# Print the version of one package from Cargo.lock, failing on absent or
# ambiguous (multiple versions of a crypto dependency is its own red flag).
lock_version() {
    local pkg="$1" versions
    versions="$(awk -v pkg="$pkg" '
        $0 == "name = \"" pkg "\"" { want = 1; next }
        want && /^version = / { gsub(/version = |"/, ""); print; want = 0 }
    ' "$LOCKFILE")"
    if [ -z "$versions" ]; then
        echo "ERROR: $pkg is in the CBOM template but not in $LOCKFILE; update the template (cbom/README.md)" >&2
        return 1
    fi
    if [ "$(wc -l <<< "$versions")" -ne 1 ]; then
        echo "ERROR: $pkg resolves to multiple versions in $LOCKFILE: $(tr '\n' ' ' <<< "$versions")" >&2
        return 1
    fi
    echo "$versions"
}

generate() {
    [ -f "$TEMPLATE" ] || { echo "ERROR: template not found: $TEMPLATE" >&2; exit 1; }
    [ -f "$LOCKFILE" ] || { echo "ERROR: $LOCKFILE not found (run from the repo root)" >&2; exit 1; }

    local version serial timestamp
    version="${VERSION:-$(lock_version "$ROOT_PACKAGE")}"
    serial="urn:uuid:$(uuidgen | tr '[:upper:]' '[:lower:]')"
    timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

    # Collect name=version pairs for every pkg:cargo/* library in the template.
    local libs lib args=()
    libs="$(jq -r '.components[] | select(.purl // "" | startswith("pkg:cargo/")) | .name' "$TEMPLATE")"
    while IFS= read -r lib; do
        args+=("$lib=$(lock_version "$lib")")
    done <<< "$libs"

    mkdir -p "$(dirname "$cbom")"
    jq --arg serial "$serial" --arg ts "$timestamp" --arg ver "$version" \
       --arg libs "$(IFS=,; echo "${args[*]}")" '
        ($libs | split(",") | map(split("=") | {(.[0]): .[1]}) | add) as $lv
        | .serialNumber = $serial
        | .metadata.timestamp = $ts
        | .metadata.component.version = $ver
        | .metadata.component.purl = "pkg:cargo/\(.metadata.component.name)@\($ver)"
        | .components |= map(
            if (.purl // "" | startswith("pkg:cargo/")) then
                .version = $lv[.name]
                | .purl = "pkg:cargo/\(.name)@\($lv[.name])"
            else . end)
    ' "$TEMPLATE" > "$cbom"
    echo "generated: $cbom (bindy $version)"
}

check() {
    [ -f "$cbom" ] || { echo "ERROR: CBOM not found: $cbom" >&2; exit 1; }
    local failures
    failures="$(jq -r --argjson maj "$MIN_SPEC_MAJOR" --argjson min "$MIN_SPEC_MINOR" '
        def spec_ok: (.specVersion // "0.0" | split(".") | map(tonumber)) as $v
            | ($v[0] > $maj) or ($v[0] == $maj and $v[1] >= $min);
        def crypto: [.components[]? | select(.type == "cryptographic-asset")];
        def algos: [crypto[] | select(.cryptoProperties.assetType == "algorithm")];
        def has_surface: [(.properties // [])[] | select(.name == "firestoned:bindy:surface")] | length > 0;
        [
          (if .bomFormat != "CycloneDX" then "bomFormat is not CycloneDX" else empty end),
          (if spec_ok | not then "specVersion \(.specVersion) is below \($maj).\($min)" else empty end),
          (if (.serialNumber // "") == "" or (.serialNumber | test("^urn:uuid:0{8}")) then "no stamped serialNumber (run generate first)" else empty end),
          (if (.metadata.timestamp // "" | startswith("1970")) or (.metadata.timestamp // "") == "" then "no stamped metadata.timestamp" else empty end),
          (if ((.metadata.tools.components // []) | length) == 0 then "no metadata.tools (generating tool)" else empty end),
          (if .metadata.supplier == null then "no metadata.supplier" else empty end),
          (if (.metadata.component.version // "0.0.0") == "0.0.0" then "root component version not stamped" else empty end),
          (if (crypto | length) == 0 then "no cryptographic-asset components" else empty end),
          (crypto | map(select(.cryptoProperties.assetType == null)) | length
             | if . > 0 then "\(.) cryptographic asset(s) without cryptoProperties.assetType" else empty end),
          (crypto | map(select(has_surface | not)) | length
             | if . > 0 then "\(.) cryptographic asset(s) without a firestoned:bindy:surface property" else empty end),
          (algos | map(select(.cryptoProperties.algorithmProperties.primitive == null)) | length
             | if . > 0 then "\(.) algorithm asset(s) without a primitive" else empty end),
          (algos | map(select(.cryptoProperties.algorithmProperties.nistQuantumSecurityLevel == null)) | length
             | if . > 0 then "\(.) algorithm asset(s) without nistQuantumSecurityLevel" else empty end),
          (.components[]? | select(.purl // "" | startswith("pkg:cargo/")) | select(.version == "0.0.0")
             | "library \(.name) version not stamped from Cargo.lock"),
          (if ([.dependencies[]? | select((.provides // []) | length > 0)] | length) == 0
             then "no provides relationship in the dependency graph" else empty end)
        ] | .[]
    ' "$cbom")"

    jq -r '
        [.components[]? | select(.type == "cryptographic-asset")] as $c
        | ([$c[] | select(.cryptoProperties.assetType == "algorithm")
              | select(.cryptoProperties.algorithmProperties.nistQuantumSecurityLevel == 0)] | length) as $vuln
        | "\(input_filename): CycloneDX \(.specVersion), \($c | length) cryptographic assets, \($vuln) quantum-vulnerable algorithm(s) declared"
    ' "$cbom"

    if [ -n "$failures" ]; then
        echo "ERROR: $cbom fails the CBOM quality gate:" >&2
        while IFS= read -r line; do echo "  - $line" >&2; done <<< "$failures"
        exit 1
    fi
    echo "OK: $cbom meets the CBOM quality gate (ADR-0011)"
}

case "$cmd" in
    generate) generate ;;
    check) check ;;
    *) usage ;;
esac
