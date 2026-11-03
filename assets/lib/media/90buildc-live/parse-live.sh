#!/bin/sh
# shellcheck disable=SC2034
# Claim root=buildc:... for the buildc live module. The scheme keeps this
# module the only owner of the live boot path when an image also ships
# dmsquash-live.

[ -z "$root" ] && root=$(getarg root=)

case "$root" in
    buildc:CDLABEL=* | buildc:LABEL=*)
        label=${root#buildc:}
        label=${label#*LABEL=}
        label=$(printf '%s' "$label" | sed 's,/,\\x2f,g;s, ,\\x20,g')
        root="buildc:/dev/disk/by-label/${label}"
        ;;
    buildc:/dev/*)
        root="buildc:${root#buildc:}"
        ;;
    *)
        return 0
        ;;
esac

rootok=1
printf '%s' "${root#buildc:}" > /run/buildc-live-root.dev

for module in sr_mod cdrom loop overlay isofs squashfs; do
    modprobe "$module" 2> /dev/null || true
done
