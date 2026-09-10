#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

mapfile -t scripts < <(
    find scripts generated/lib modules -path modules/.remote -prune -o \
        \( -name '*.sh' -o -name repo \) -type f -print
    find modules -path modules/.remote -prune -o -path '*/files/*' -type f \
        \( -path '*/libexec/*' -o -path '*/system-generators/*' \) -print
)
shfmt=(shfmt -i 4 -ci -bn -sr)

if [ "${1:-}" = "--fix" ]; then
    "${shfmt[@]}" -w "${scripts[@]}"
    echo "lint: formatted ${#scripts[@]} scripts"
    exit
fi

for tool in shellcheck shfmt; do
    command -v "$tool" > /dev/null 2>&1 || {
        echo "lint: $tool not found, install it first" >&2
        exit 1
    }
done

mapfile -t repos < <(printf '%s\n' "${scripts[@]}" | grep '/repo$' || true)
mapfile -t rest < <(printf '%s\n' "${scripts[@]}" | grep -v '/repo$')
shellcheck -s bash "${rest[@]}"
# A repo.sh sets REPO_ID for the generated script.
# Shellcheck cannot see that reader, so SC2034 is off for these files.
[ "${#repos[@]}" -eq 0 ] || shellcheck -s bash -e SC2034 "${repos[@]}"
"${shfmt[@]}" -d "${scripts[@]}" || {
    echo "lint: unformatted, run ./scripts/lint.sh --fix" >&2
    exit 1
}
echo "lint: ${#scripts[@]} scripts pass shellcheck and shfmt"

./scripts/buildc.sh fetch modules

./scripts/buildc.sh check

./scripts/buildc.sh verify
