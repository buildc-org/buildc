#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
    echo "buildc: $*" >&2
    exit 1
}

version="$(sed -n 's/^buildc-version "\([^"]*\)".*$/\1/p' repo.kdl)"
[ -n "$version" ] || die "repo.kdl declares no buildc-version"
sha256="$(sed -n 's/^buildc-version "[^"]*" sha256="\([^"]*\)"$/\1/p' repo.kdl)"

bin="${BUILDC_BIN:-out/buildc-${version}}"

if [ ! -x "$bin" ]; then
    asset="buildc-v${version}-x86_64-linux-musl.tar.gz"
    url="https://github.com/buildc-org/buildc/releases/download/v${version}/${asset}"

    mkdir -p out
    tmp="$(mktemp -d -p out download.XXXXXXXX)"
    trap 'rm -rf "$tmp"' EXIT

    curl -fsSL --retry 3 --retry-all-errors -o "${tmp}/${asset}" "$url" \
        || die "cannot fetch ${url}"
    # A declared hash verifies the download on its own, so the published .sha256 file is fetched only when the declaration carries none.
    if [ -n "$sha256" ]; then
        (cd "$tmp" && printf '%s  %s\n' "$sha256" "$asset" \
            | sha256sum --check --status) \
            || die "${asset} does not match the sha256 repo.kdl declares"
    else
        curl -fsSL --retry 3 --retry-all-errors -o "${tmp}/sha256" "${url}.sha256" \
            || die "cannot fetch ${url}.sha256"
        (cd "$tmp" && printf '%s  %s\n' "$(cat sha256)" "$asset" \
            | sha256sum --check --status) \
            || die "${asset} does not match its published checksum"
    fi

    tar -xzf "${tmp}/${asset}" -C "$tmp" buildc
    mv "${tmp}/buildc" "$bin"
    rm -rf "$tmp"
    trap - EXIT
fi

if [ "${1:-}" = "--path" ]; then
    echo "$bin"
    exit 0
fi

exec "$bin" "$@"
