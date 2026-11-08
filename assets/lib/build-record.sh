#!/usr/bin/env bash
set -euo pipefail

out="${1:-/usr/share/buildc/build.json}"

escape() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    printf '%s' "$value"
}

json_string() {
    printf '"%s"' "$(escape "$1")"
}

optional() {
    if [ -n "${1:-}" ]; then
        json_string "$1"
    else
        printf 'null'
    fi
}

components=""
for field in ${COMPONENT_HASHES:-}; do
    IFS='|' read -r -a parts <<< "$field"
    [ "${#parts[@]}" -eq 2 ] || continue
    [ -z "$components" ] || components+=","
    components+="$(printf '{"path": %s, "content": %s}' \
        "$(json_string "${parts[0]}")" "$(json_string "${parts[1]}")")"
done

assets=""
for field in ${ASSET_RESOLUTIONS:-}; do
    IFS='|' read -r -a parts <<< "$field"
    [ "${#parts[@]}" -eq 4 ] || continue
    [ -z "$assets" ] || assets+=","
    assets+="$(printf '{"component": %s, "name": %s, "selector": %s, "resolved": %s}' \
        "$(json_string "${parts[0]}")" \
        "$(json_string "${parts[1]}")" \
        "$(json_string "${parts[2]}")" \
        "$(json_string "${parts[3]}")")"
done

enforce=false
[ "${AUDIT_ENFORCE:-}" = "true" ] && enforce=true

measurements="${VERSIONS:-}"
if [ -z "$measurements" ] && [ -n "${VERSIONS_FILE:-}" ] && [ -f "$VERSIONS_FILE" ]; then
    measurements="$(tr '\n' ' ' < "$VERSIONS_FILE")"
fi

versions=""
for field in $measurements; do
    IFS='|' read -r -a parts <<< "$field"
    [ "${#parts[@]}" -eq 4 ] || continue
    [ -z "$versions" ] || versions+=", "
    versions+="$(printf '{"provider": %s, "name": %s, "package": %s, "version": %s}' \
        "$(json_string "${parts[0]}")" \
        "$(json_string "${parts[1]}")" \
        "$(json_string "${parts[2]}")" \
        "$(json_string "${parts[3]}")")"
done

text="$(
    printf '{\n'
    printf '  "schema_version": 1,\n'
    printf '  "target": %s,\n' "$(optional "${TARGET:-}")"
    printf '  "image": %s,\n' "$(optional "${IMAGE_ID:-}")"
    printf '  "version": %s,\n' "$(optional "${IMAGE_VERSION:-}")"
    printf '  "buildc": %s,\n' "$(json_string "${BUILDC_VERSION:?build-record: BUILDC_VERSION is unset}")"
    printf '  "backend": %s,\n' "$(optional "${BUILD_BACKEND:-}")"
    printf '  "source_commit": %s,\n' "$(optional "${SOURCE_COMMIT:-}")"
    # An unversioned image writes no `versions` key. A consumer sees the key only when the build measured one.
    if [ -n "$versions" ]; then
        printf '  "base": {"declared": %s, "resolved": %s, "versions": [%s]},\n' \
            "$(optional "${BASE_DECLARED:-}")" "$(optional "${BASE:-}")" "$versions"
    else
        printf '  "base": {"declared": %s, "resolved": %s},\n' \
            "$(optional "${BASE_DECLARED:-}")" "$(optional "${BASE:-}")"
    fi
    printf '  "components": [%s],\n' "$components"
    printf '  "assets": [%s],\n' "$assets"
    printf '  "audit": {"enforce": %s},\n' "$enforce"
    printf '  "verified": null\n'
    printf '}\n'
)"

mkdir -p "$(dirname "$out")"
printf '%s\n' "$text" > "$out"
