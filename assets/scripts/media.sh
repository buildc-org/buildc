#!/usr/bin/env bash
# shellcheck disable=SC2034
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."
[ -f repo.kdl ] || cd ..
source ./generated/lib/vm-helpers.sh

die() {
    echo "media: $*" >&2
    exit 1
}

usage() {
    cat >&2 << 'EOF'
usage: scripts/media.sh <build|run> [options]

  build     assemble the installer ISO under out/bootiso/
  run       boot that ISO under qemu, building it if it is missing

  --image <ref>       container image the media installs (default: localhost/
                      and the plan's ungated published name)
  --tag <tag>         its tag (default: $DEFAULT_TAG, else latest)
  --target <image/flavour>
                      the target the media installs (default: the plan's
                      ungated target)
  --rebuild           generate, then build the container image first. The one
                      form of this that changes the repository.
  --ram <size>        memory for the virtual machine (default: 8G)
  --live-image <ref>  what the media boots, built here from out/bootiso/
                      Containerfile. `buildc media` names it, because this
                      script has no JSON reader to find it with.

Environment:
  VM_USER             console account passed as a systemd credential (default: buildc)
  VM_PASSWORD_HASH    its crypt(5) hash; when unset, run asks for a password
  GPU                 Y for hardware rendering in the qemu viewer (default: N)
EOF
}

command="${1:-}"
case "$command" in
    build | run) shift ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage
        die "unknown command '${command}'"
        ;;
esac

image=""
tag="${DEFAULT_TAG:-latest}"
target=""
rebuild=0
ram=8G
live_image=""
login=0

while [ $# -gt 0 ]; do
    case "$1" in
        --image | --tag | --target | --ram | --live-image)
            [ "$#" -ge 2 ] || die "$1 needs a value"
            case "$1" in
                --image) image="$2" ;;
                --tag) tag="$2" ;;
                --target) target="$2" ;;
                --ram) ram="$2" ;;
                --live-image) live_image="$2" ;;
            esac
            shift 2
            ;;
        --rebuild)
            rebuild=1
            shift
            ;;
        --login)
            login=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            usage
            die "unknown argument '$1'"
            ;;
    esac
done

if [ -n "$image" ]; then
    ref="${image}:${tag}"
else
    ref="$(IMAGE_REGISTRY=localhost ./scripts/buildc.sh registry ref --tag "$tag")"
fi

type=iso
staged=out/bootiso
image_file="${staged}/install.iso"

build_media() {
    [ -n "$live_image" ] || die "--live-image names what the media boots; \`buildc media\` passes it"
    [ -f "${staged}/Containerfile" ] \
        || die "${staged}/Containerfile is not there; \`buildc media build\` stages it"
    [ -f "${staged}/media.json" ] \
        || die "${staged}/media.json is not there; \`buildc media build\` stages it"

    load_rootful
    rm -f "${image_file}.part"

    sudoif env HOME=/root LIVE="$live_image" PAYLOAD="$ref" \
        STAGED="${PWD}/${staged}" BUILD="${PWD}/${staged}/build" \
        ISO="${PWD}/${image_file}.part" OWNER="$(id -u):$(id -g)" \
        MODULE="${PWD}/generated/lib/media/90buildc-live" \
        bash -s << 'ROOT' || die "the media could not be assembled; the staging is under ${PWD}/${staged}/build"
set -euo pipefail

die() {
    echo "media: $*" >&2
    exit 1
}

for tool in jq podman skopeo mksquashfs mkfs.fat mcopy xorriso; do
    command -v "$tool" > /dev/null 2>&1 \
        || die "$tool is not installed, and it assembles the media"
done

mount_point=""
cleanup() {
    if mountpoint -q "${BUILD}/store/overlay" 2> /dev/null; then
        umount "${BUILD}/store/overlay" 2> /dev/null || true
    fi
    if [ -n "$mount_point" ]; then
        podman image unmount "$LIVE" > /dev/null 2>&1 || true
    fi
    chown -R "$OWNER" "$STAGED"
}
trap cleanup EXIT

manifest="$(podman image inspect --format '{{.ManifestType}}' "$PAYLOAD")" \
    || die "cannot read the manifest type of ${PAYLOAD} from root's store"
if [ "$manifest" != "application/vnd.oci.image.manifest.v1+json" ]; then
    skopeo copy --quiet --format oci \
        "containers-storage:${PAYLOAD}" "containers-storage:${PAYLOAD}" \
        || die "cannot store ${PAYLOAD} as OCI"
fi

image_id="$(podman image inspect --format '{{.Id}}' "$PAYLOAD")" \
    || die "cannot read the image ID of ${PAYLOAD}"
[[ "$image_id" =~ ^[0-9a-f]{64}$ ]] || die "podman named no image ID for ${PAYLOAD}"
printf '%s\n' "$image_id" > "${STAGED}/image-id"

media_name="$(jq -r '.media_name' "${STAGED}/media.json")"
env_id="$(jq -r '.bootable_environments[0].id' "${STAGED}/media.json")"
title="$(jq -r '.bootable_environments[0].title // .bootable_environments[0].id' "${STAGED}/media.json")"
payload_source="$(jq -r '.offline_payloads[0].source' "${STAGED}/media.json")"
payload_ref="$(jq -r '.offline_payloads[0].ref' "${STAGED}/media.json")"
[ -n "$media_name" ] && [ -n "$env_id" ] || die "media.json names no media or environment"

echo ">>> building the live environment ${LIVE}"
podman build -t "$LIVE" "$STAGED"

if mountpoint -q "${BUILD}/store/overlay" 2> /dev/null; then
    umount "${BUILD}/store/overlay" 2> /dev/null || true
fi
rm -rf "$BUILD"
iso_root="${BUILD}/iso/iso-root"
esp="${BUILD}/iso/esp-staging"
mkdir -p "$iso_root/LiveOS" "$iso_root/EFI/BOOT" "$iso_root/images/pxeboot" \
    "$esp/EFI/BOOT" "$esp/loader/entries" "$esp/images/pxeboot/${env_id}"

mount_point="$(podman image mount "$LIVE")" \
    || die "cannot mount ${LIVE}"

echo ">>> squashing the live root"
mksquashfs "$mount_point" "${iso_root}/LiveOS/${env_id}.rootfs.sfs" \
    -noappend -comp zstd -Xcompression-level 3 -b 131072 -processors 4 \
    -e proc -e sys -e dev -e run -e tmp -e var/lib/containers/storage \
    > /dev/null

kver=""
for dir in "$mount_point"/usr/lib/modules/*; do
    [ -f "${dir}/vmlinuz" ] || continue
    case "${dir##*/}" in
        *debug*) continue ;;
    esac
    kver="${dir##*/}"
    break
done
[ -n "$kver" ] || die "${LIVE} carries no non-debug kernel"

echo ">>> rebuilding the initramfs for ${kver}"
podman run --rm --privileged \
    -v "${MODULE}:/usr/lib/dracut/modules.d/90buildc-live:ro" \
    -v "${esp}/images/pxeboot/${env_id}:/out" \
    "$LIVE" \
    dracut --force --no-hostonly --add buildc-live --kver "$kver" /out/initrd.img \
    || die "cannot rebuild the initramfs; does ${LIVE} ship dracut?"
cp "${mount_point}/usr/lib/modules/${kver}/vmlinuz" \
    "${esp}/images/pxeboot/${env_id}/vmlinuz"

suffix=x64
[ "$(uname -m)" != aarch64 ] || suffix=aa64
shim="$(find "$mount_point/usr/lib/efi/shim" -path "*/EFI/*/shim${suffix}.efi" -type f 2> /dev/null | sort | head -n1)"
grub="$(find "$mount_point/usr/lib/efi/grub2" -path "*/EFI/*/grub${suffix}.efi" -type f 2> /dev/null | sort | head -n1)"
vendor=""
if [ -n "$shim" ] && [ -n "$grub" ]; then
    vendor="$(basename "$(dirname "$shim")")"
else
    shim="$mount_point/usr/lib/shim/shim${suffix}.efi.signed"
    [ -f "$shim" ] || shim=""
    grub="$(find "$mount_point/usr/lib/grub" -name "grub${suffix}.efi.signed" -type f 2> /dev/null | sort | head -n1)"
    vendor="$(sed -n 's/^ID=//p' "$mount_point/etc/os-release" 2> /dev/null | tr -d '"' | head -n1)"
fi
[ -n "$shim" ] && [ -n "$grub" ] \
    || die "${LIVE} ships no signed shim and GRUB pair"
cp "$shim" "${esp}/EFI/BOOT/BOOT${suffix^^}.EFI"
cp "$grub" "${esp}/EFI/BOOT/grub${suffix}.efi"
mm="$(dirname "$shim")/mm${suffix}.efi"
[ -f "$mm" ] || mm="$(dirname "$shim")/mm${suffix}.efi.signed"
[ ! -f "$mm" ] || cp "$mm" "${esp}/EFI/BOOT/mm${suffix}.efi"

options="root=buildc:CDLABEL=${media_name// /\\x20} buildc.live.squashimg=${env_id}.rootfs.sfs buildc.live.overlay.size=8192 enforcing=0 console=ttyS0,115200n8"
menu() {
    echo "set default=0"
    echo "set timeout=3"
    echo
    echo "menuentry '${title} (live)' {"
    echo "    linux /images/pxeboot/${env_id}/vmlinuz ${options}"
    echo "    initrd /images/pxeboot/${env_id}/initrd.img"
    echo "}"
}
menu > "${esp}/EFI/BOOT/grub.cfg"
if [ -n "$vendor" ]; then
    mkdir -p "${esp}/EFI/${vendor}"
    menu > "${esp}/EFI/${vendor}/grub.cfg"
fi

{
    echo "title ${title} (live)"
    echo "sort-key 0-buildc-${env_id}"
    echo "linux /images/pxeboot/${env_id}/vmlinuz"
    echo "initrd /images/pxeboot/${env_id}/initrd.img"
    echo "options ${options}"
} > "${esp}/loader/entries/${env_id}.conf"
printf 'timeout 5\ndefault *\nconsole-mode max\neditor no\n' > "${esp}/loader/loader.conf"

cp -a "${esp}/EFI/." "${iso_root}/EFI/"
cp -a "${esp}/images/pxeboot/." "${iso_root}/images/pxeboot/"

kb="$(du -sk "$esp" | cut -f1)"
mb=$(((kb * 12 / 10) / 1024 + 32))
truncate -s "${mb}M" "${iso_root}/EFI/efi.img"
mkfs.fat -F 32 -n ESP "${iso_root}/EFI/efi.img" > /dev/null
MTOOLS_SKIP_CHECK=1
for entry in "$esp"/*; do
    mcopy -i "${iso_root}/EFI/efi.img" -s "$entry" ::/
done

echo ">>> packing the offline store"
store="${BUILD}/store"
store_run="${BUILD}/store-run"
if mountpoint -q "${store}/overlay" 2> /dev/null; then
    umount "${store}/overlay" 2> /dev/null || true
fi
rm -rf "$store" "$store_run"
mkdir -p "${store}/etc/containers" "$store_run"
printf '[storage]\ndriver = "overlay"\n' > "${store}/etc/containers/storage.conf"
skopeo copy --remove-signatures \
    "containers-storage:${payload_source}" \
    "containers-storage:[overlay@${store}+${store_run}]${payload_ref}" \
    || die "cannot copy ${payload_source} into the offline store"
mksquashfs "$store" "${iso_root}/LiveOS/store.squashfs.img" \
    -noappend -comp zstd -Xcompression-level 3 -b 131072 -processors 4 \
    > /dev/null

echo ">>> assembling the ISO"
touch "$ISO"
xorriso -dev "stdio:${ISO}" -volid "$media_name" \
    -rockridge on -joliet on -map "$iso_root" / \
    -boot_image any platform_id=0xef \
    -boot_image any efi_path=EFI/efi.img \
    -boot_image any part_like_isohybrid=on -commit \
    || die "xorriso cannot assemble ${ISO}"
ROOT

    mv -f "${image_file}.part" "$image_file"
}

if [ "$command" = build ] || [ "$rebuild" = 1 ] || [ ! -f "$image_file" ]; then
    if [ "$rebuild" = 1 ]; then
        ./scripts/buildc.sh generate
        args=(--backend buildah --tag "$ref")
        [ -z "$target" ] || args+=(--target "$target")
        ./scripts/buildc.sh build "${args[@]}"
    fi
    build_media
fi

if [ "$login" = 1 ]; then
    login_credentials
fi

case "$command" in
    run) run_qemu ;;
esac
