#!/bin/bash

ensure_checkpolicy() {
    command -v checkmodule > /dev/null 2>&1 || dnf5 install -y checkpolicy
}

install_selinux_module() {
    local te="$1"
    local base="${te%.te}"
    ensure_checkpolicy
    checkmodule -M -m -o "${base}.mod" "$te"
    semodule_package -o "${base}.pp" -m "${base}.mod"
    # The image is not booted, so semodule installs the module without a policy reload.
    # Priority 200 keeps this module above a distribution module of the same name.
    semodule -n -s targeted -X 200 -i "${base}.pp"
    rm -f "$te" "${base}.mod" "${base}.pp"
}
