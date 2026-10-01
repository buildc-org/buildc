#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f repo.kdl ] || cd ..

die() {
    echo "buildc: $*" >&2
    exit 1
}

REPO="buildc-org/buildc"

line="$(sed -n '/^[[:space:]]*buildc-version[[:space:]]/p' repo.kdl)"
if [[ "$line" =~ ^[[:space:]]*buildc-version[[:space:]]+\"([^\"]+)\"(.*)$ ]]; then
    version="${BASH_REMATCH[1]}"
    rest="${BASH_REMATCH[2]%%//*}"
elif [ -n "$line" ]; then
    die "repo.kdl declares no readable buildc-version"
else
    latest="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest")" \
        || die "cannot resolve the latest release of ${REPO}"
    [[ "$latest" =~ /tag/v([^/]+)$ ]] \
        || die "cannot read a release out of ${latest}"
    version="${BASH_REMATCH[1]}"
    rest=""
    echo "buildc: repo.kdl pins no release, using the latest, ${version}" >&2
fi

sha256=""
if [[ "$rest" =~ (^|[[:space:]])sha256[[:space:]]*= ]]; then
    if [[ "$rest" =~ (^|[[:space:]])sha256[[:space:]]*=[[:space:]]*\"([^\"]*)\" ]]; then
        sha256="${BASH_REMATCH[2]}"
    else
        die "repo.kdl declares no readable sha256 for buildc-version"
    fi
    [[ "$sha256" =~ ^[0-9a-f]{64}$ ]] \
        || die "repo.kdl declares a malformed sha256 for buildc-version"
fi

case "$(uname -m)" in
    x86_64 | aarch64) arch="$(uname -m)" ;;
    *) die "Linux is published for x86_64 and aarch64, and this is $(uname -m)" ;;
esac

if [ -n "$sha256" ] && [ "$arch" != "x86_64" ]; then
    die "repo.kdl pins the sha256 of the x86_64 release, and this is ${arch}"
fi

# The hash joins the cache name so a same-version repin cannot reuse the old binary.
bin="${BUILDC_BIN:-out/buildc-${version}${sha256:+-${sha256}}}"

if [ ! -x "$bin" ]; then
    asset="buildc-v${version}-${arch}-linux-musl.tar.gz"
    url="https://github.com/${REPO}/releases/download/v${version}/${asset}"

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
