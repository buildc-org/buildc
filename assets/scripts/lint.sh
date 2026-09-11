#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

mapfile -t scripts < <(find scripts generated/lib modules -path modules/.remote -prune -o \
    -name '*.sh' -type f ! \( -path '*/files/*' \
    \( -path '*/libexec/*' -o -path '*/system-generators/*' \) \) -print)
mapfile -t repos < <(find modules -path modules/.remote -prune -o -name repo -type f -print)
mapfile -t helpers < <(find modules -path modules/.remote -prune -o -path '*/files/*' -type f \
    \( -path '*/libexec/*' -o -path '*/system-generators/*' \) -print)
shfmt=(shfmt -i 4 -ci -bn -sr)
counted="${#scripts[@]} scripts, ${#repos[@]} repo files and ${#helpers[@]} helpers"

if [ "${1:-}" = "--fix" ]; then
    "${shfmt[@]}" -w "${scripts[@]}" "${repos[@]}" "${helpers[@]}"
    echo "lint: formatted ${counted}"
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
[ "${#helpers[@]}" -eq 0 ] || shellcheck "${helpers[@]}"
"${shfmt[@]}" -d "${scripts[@]}" "${repos[@]}" "${helpers[@]}" || {
    echo "lint: unformatted, run ./scripts/lint.sh --fix" >&2
    exit 1
}
echo "lint: ${counted} pass shellcheck and shfmt"

./scripts/buildc.sh fetch modules

./scripts/buildc.sh check

./scripts/buildc.sh verify
