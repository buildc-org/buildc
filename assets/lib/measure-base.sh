#!/usr/bin/env bash
set -euo pipefail

source /ctx/lib/version-helpers.sh

: "${FAMILY:?measure-base: FAMILY is unset}"
out=/usr/share/buildc/versions
mkdir -p "${out%/*}"
: > "$out"

if [ -n "${PROVIDES:-}" ]; then
    refresh_metadata "$FAMILY"
fi

for entry in ${PROVIDES:-}; do
    IFS='|' read -r name package <<< "$entry"
    version="$(query_version "$FAMILY" "$package")" || {
        echo "buildc: the base provides \`$name\` from \`$package\`. The package database has no such package." >&2
        exit 1
    }
    printf 'base|%s|%s|%s\n' "$name" "$package" "$version" >> "$out"
done

for entry in ${FLOORS:-}; do
    IFS='|' read -r name min flavour owner <<< "$entry"
    if [ -n "$flavour" ] && [ "${FLAVOUR:-}" != "$flavour" ]; then
        continue
    fi
    version="$(measured_version "$out" "$name")"
    if [ -z "$version" ]; then
        echo "buildc: \`$owner\` requires \`$name\` $min or later. The base names no package to measure it from." >&2
        exit 1
    fi
    if ! dotted_version "$version"; then
        echo "buildc: the base carries \`$name\` $version. That is not a dotted number. The floor $min that \`$owner\` requires cannot be compared." >&2
        exit 1
    fi
    if [ "$(version_cmp "$version" "$min")" -lt 0 ]; then
        echo "buildc: the base carries \`$name\` $version. \`$owner\` requires $min or later." >&2
        exit 1
    fi
done
