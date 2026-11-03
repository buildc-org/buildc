#!/bin/sh
# Mount the live medium, its rootfs squashfs, and the writable overlay.
# The sysroot overlay mount is the generator's unit, which starts after
# this script has prepared its directories.

command -v getarg > /dev/null 2>&1 || . /lib/dracut-lib.sh

dev=$(cat /run/buildc-live-root.dev 2> /dev/null)
[ -n "$dev" ] || exit 1

squashimg=$(cat /run/buildc-live-squashimg 2> /dev/null)
[ -n "$squashimg" ] || squashimg=installer.rootfs.sfs

size=$(cat /run/buildc-live-overlay-size 2> /dev/null)
[ -n "$size" ] || size=8192

tries=0
while [ ! -e "$dev" ] && [ "$tries" -lt 60 ]; do
    udevadm settle --timeout=1 > /dev/null 2>&1 || sleep 1
    tries=$((tries + 1))
done
[ -e "$dev" ] || {
    echo "buildc-live: $dev did not appear within 60s" >&2
    exit 1
}

mkdir -p /run/initramfs/live /run/rootfsbase /run/buildc-overlay

mount -o ro "$dev" /run/initramfs/live \
    || {
        echo "buildc-live: cannot mount $dev" >&2
        exit 1
    }

sfs="/run/initramfs/live/LiveOS/${squashimg}"
mount -o ro,loop "$sfs" /run/rootfsbase \
    || {
        echo "buildc-live: cannot mount $sfs" >&2
        exit 1
    }

mount -t tmpfs -o "mode=0755,size=${size}m" buildc-overlay /run/buildc-overlay \
    || {
        echo "buildc-live: cannot mount the overlay tmpfs" >&2
        exit 1
    }

mkdir -p /run/buildc-overlay/upper /run/buildc-overlay/work
echo "buildc-live: live root prepared from $sfs"
