#!/bin/sh
# Write the units that assemble the live root. An early generator outranks
# the unit systemd-fstab-generator writes for the unrecognized root=.

command -v getarg > /dev/null 2>&1 || . /lib/dracut-lib.sh

[ -z "$root" ] && root=$(getarg root=)
case "$root" in
    buildc:*) ;;
    *) exit 0 ;;
esac

case "$root" in
    buildc:CDLABEL=* | buildc:LABEL=*)
        label=${root#buildc:}
        label=${label#*LABEL=}
        label=$(printf '%s' "$label" | sed 's,/,\\x2f,g;s, ,\\x20,g')
        printf '%s' "/dev/disk/by-label/${label}" > /run/buildc-live-root.dev
        ;;
    buildc:/dev/*)
        printf '%s' "${root#buildc:}" > /run/buildc-live-root.dev
        ;;
esac

squashimg=$(getarg buildc.live.squashimg)
[ -n "$squashimg" ] || squashimg=installer.rootfs.sfs
printf '%s' "$squashimg" > /run/buildc-live-squashimg

size=$(getarg buildc.live.overlay.size)
[ -n "$size" ] || size=8192
printf '%s' "$size" > /run/buildc-live-overlay-size

dir="$2"
[ -z "$dir" ] && exit 1
mkdir -p "$dir"

{
    echo "[Unit]"
    echo "DefaultDependencies=no"
    echo "Before=sysroot.mount initrd-root-fs.target"
    echo "After=systemd-udevd.service"
    echo "[Service]"
    echo "Type=oneshot"
    echo "RemainAfterExit=yes"
    echo "ExecStart=/sbin/buildc-live-root"
    echo "StandardOutput=journal+console"
    echo "StandardError=journal+console"
} > "$dir/buildc-live-prepare.service"

{
    echo "[Unit]"
    echo "Before=initrd-root-fs.target"
    echo "After=buildc-live-prepare.service"
    echo "Requires=buildc-live-prepare.service"
    echo "[Mount]"
    echo "Where=/sysroot"
    echo "What=buildc_live_root"
    echo "Options=lowerdir=/run/rootfsbase,upperdir=/run/buildc-overlay/upper,workdir=/run/buildc-overlay/work"
    echo "Type=overlay"
} > "$dir/sysroot.mount"

mkdir -p "$dir/buildc_live_root.device.d"
{
    echo "[Unit]"
    echo "JobTimeoutSec=3000"
    echo "JobRunningTimeoutSec=3000"
} > "$dir/buildc_live_root.device.d/timeout.conf"
