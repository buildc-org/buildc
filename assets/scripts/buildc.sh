#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
    echo "buildc: $*" >&2
    exit 1
}

version="$(sed -n 's/^buildc-version "\(.*\)"$/\1/p' repo.kdl)"
[ -n "$version" ] || die "repo.kdl declares no buildc-version"

bin="${BUILDC_BIN:-out/buildc-${version}}"

if [ ! -x "$bin" ]; then
    asset="buildc-v${version}-x86_64-linux-musl.tar.gz"
    url="https://github.com/buildc-org/buildc/releases/download/v${version}/${asset}"

    mkdir -p out
    tmp="$(mktemp -d -p out download.XXXXXXXX)"
    trap 'rm -rf "$tmp"' EXIT

    curl -fsSL --retry 3 -o "${tmp}/${asset}" "$url" \
        || die "cannot fetch ${url}"
    curl -fsSL --retry 3 -o "${tmp}/sha256" "${url}.sha256" \
        || die "cannot fetch ${url}.sha256"
    (cd "$tmp" && printf '%s  %s\n' "$(cat sha256)" "$asset" \
        | sha256sum --check --status) \
        || die "${asset} does not match its published checksum"

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
