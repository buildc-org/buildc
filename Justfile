[private]
default:
    @just --list

[group('Just')]
check:
    just --unstable --fmt --check -f Justfile

[group('Just')]
fix:
    just --unstable --fmt -f Justfile

lint:
    ./lint.sh

format:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v shfmt &> /dev/null; then
        echo "shfmt could not be found. Please install it." >&2
        exit 1
    fi
    find . -path ./target -prune -o -name '*.sh' -type f -exec shfmt --write '{}' ';'
