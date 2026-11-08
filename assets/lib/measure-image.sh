#!/usr/bin/env bash
set -euo pipefail

source /ctx/lib/version-helpers.sh

: "${FAMILY:?measure-image: FAMILY is unset}"
out=/usr/share/buildc/versions
mkdir -p "${out%/*}"
if [ "${RESET_VERSIONS:-}" = "1" ]; then
    : > "$out"
else
    touch "$out"
fi

for entry in ${MEASURED:-}; do
    IFS='|' read -r component name package flavour <<< "$entry"
    if [ -n "$flavour" ] && [ "${FLAVOUR:-}" != "$flavour" ]; then
        continue
    fi
    version="$(query_installed "$FAMILY" "$package")" || {
        echo "buildc: \`$component\` provides \`$name\` from \`$package\`. The image has no such package." >&2
        exit 1
    }
    printf '%s|%s|%s|%s\n' "$component" "$name" "$package" "$version" >> "$out"
done
