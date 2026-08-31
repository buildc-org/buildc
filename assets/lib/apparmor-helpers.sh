#!/bin/bash

install_apparmor_profile() {
    local profile="$1"
    if ! command -v apparmor_parser > /dev/null 2>&1; then
        echo "apparmor_parser is not in this image, so a shipped profile cannot be checked" >&2
        return 1
    fi
    # -Q checks the profile without loading it.
    # The next boot loads it from /etc/apparmor.d.
    apparmor_parser -Q "$profile"
    install -D -m 0644 -- "$profile" "/etc/apparmor.d/$(basename "$profile")"
}
