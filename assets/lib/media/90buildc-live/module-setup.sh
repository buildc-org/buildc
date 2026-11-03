#!/bin/bash
# shellcheck disable=SC2154

check() { return 255; }

depends() {
    echo "base"
    return 0
}

installkernel() {
    hostonly='' instmods squashfs loop overlay iso9660 sr_mod cdrom
}

install() {
    inst_hook cmdline 30 "$moddir/parse-live.sh"
    inst_simple "$moddir/live-root.sh" "/sbin/buildc-live-root"
    chmod 0755 "${initdir}/sbin/buildc-live-root"
    inst_simple "$moddir/live-generator.sh" \
        "${systemdutildir}/system-generators/buildc-live-generator"
    chmod 0755 "${initdir}${systemdutildir}/system-generators/buildc-live-generator"
}
