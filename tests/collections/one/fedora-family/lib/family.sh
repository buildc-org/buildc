#!/bin/bash

install_packages() {
    local args=()
    if [ -n "${BUILDC_ENABLE_REPO:-}" ]; then
        args+=(--enablerepo="$BUILDC_ENABLE_REPO")
    fi
    dnf5 install -y "${args[@]}" "$@"
}
