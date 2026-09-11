#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

mapfile -t scripts < <(
    find scripts generated/lib modules -path modules/.remote -prune -o \
        -name '*.sh' -type f -print
    find modules -path modules/.remote -prune -o -path '*/files/*' -type f \
        \( -path '*/libexec/*' -o -path '*/system-generators/*' \) -print
)
mapfile -t repos < <(find modules -path modules/.remote -prune -o -name repo -type f -print)
shfmt=(shfmt -i 4 -ci -bn -sr)

if [ "${1:-}" = "--fix" ]; then
    "${shfmt[@]}" -w "${scripts[@]}" "${repos[@]}"
    echo "lint: formatted ${#scripts[@]} scripts"
    exit
fi

for tool in shellcheck shfmt; do
    command -v "$tool" > /dev/null 2>&1 || {
        echo "lint: $tool not found, install it first" >&2
        exit 1
    }
done

shellcheck -s bash "${scripts[@]}"
# A repo.sh sets REPO_ID for the generated script.
# Shellcheck cannot see that reader, so SC2034 is off for these files.
[ "${#repos[@]}" -eq 0 ] || shellcheck -s bash -e SC2034 "${repos[@]}"
"${shfmt[@]}" -d "${scripts[@]}" "${repos[@]}" || {
    echo "lint: unformatted, run ./scripts/lint.sh --fix" >&2
    exit 1
}
echo "lint: ${#scripts[@]} scripts pass shellcheck and shfmt"

./scripts/buildc.sh fetch modules

./scripts/buildc.sh check

./scripts/buildc.sh verify
