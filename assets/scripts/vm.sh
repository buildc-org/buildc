#!/usr/bin/env bash
# shellcheck disable=SC2034,SC2154
set -euo pipefail

RENDERER=/usr/libexec/grub-menu-from-bls
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."
[ -f repo.kdl ] || cd ..
source ./generated/lib/vm-helpers.sh

die() {
    echo "vm: $*" >&2
    exit 1
}

usage() {
    cat >&2 << 'EOF'
usage: scripts/vm.sh <command> <qcow2|raw> [options]

  build     convert the container image into a disk image under out/
  run       boot that disk image under qemu, building it if it is missing
  spawn     boot it with systemd-vmspawn instead

  An installer ISO is not a VM disk. Use `buildc media`.

  --image <ref>       container image to convert (default: localhost/ and
                      the plan's ungated published name)
  --tag <tag>         its tag (default: $DEFAULT_TAG, else latest)
  --target <image/flavour>
                      what a rebuild builds (default: the plan's ungated target)
  --rebuild           generate, then build the container image
                      first. The one form of this that changes the repository.
  --ram <size>        memory for the virtual machine (default: 8G)
  --installer <which> what converts the image into a disk: `bib` for
                      bootc-image-builder, `bootc` for `bootc install to-disk`.
                      `buildc vm` passes this off the target's base family; the
                      default here is bib, which is fedora-only.

Environment:
  BIB_IMAGE           bootc-image-builder image
  DISK_SIZE           size of the disk `--installer bootc` writes (default: 20G).
                      The file is sparse, so this is a ceiling rather than a cost.
  VM_USER             console account passed as a systemd credential (default: buildc)
  VM_PASSWORD_HASH    its crypt(5) hash; when unset, run and spawn ask for a password
  GPU                 Y for hardware rendering in the qemu viewer (default: N)
EOF
}

command="${1:-}"
case "$command" in
    build | run | spawn) shift ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage
        die "unknown command '${command}'"
        ;;
esac

type="${1:-}"
shift || true
case "${command}/${type}" in
    */qcow2 | */raw) ;;
    */iso) die "an installer iso is \`buildc media build\`, not a vm disk" ;;
    *) die "'${command}' needs an image type (qcow2 or raw)" ;;
esac

image=""
tag="${DEFAULT_TAG:-latest}"
target=""
rebuild=0
ram=8G
installer=bib
login=0
ssh_key=""

while [ $# -gt 0 ]; do
    case "$1" in
        --image | --tag | --target | --ram | --installer | --ssh-key)
            [ "$#" -ge 2 ] || die "$1 needs a value"
            case "$1" in
                --image) image="$2" ;;
                --tag) tag="$2" ;;
                --target) target="$2" ;;
                --ram) ram="$2" ;;
                --ssh-key) ssh_key="$2" ;;
                --installer)
                    case "$2" in
                        bib | bootc) installer="$2" ;;
                        *) die "--installer is bib or bootc, not '$2'" ;;
                    esac
                    ;;
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

image_file="out/${type}/disk.${type}"

build_disk() {
    local config=disk_config/disk.toml tmp
    load_rootful
    mkdir -p out
    tmp="$(mktemp -p "${PWD}/out" -d bib.XXXXXXXX)"
    sudoif podman run \
        --rm -it --privileged --pull=newer --net=host \
        --security-opt label=type:unconfined_t \
        -v "$(realpath "$config")":/config.toml:ro \
        -v "$tmp":/output \
        -v /var/lib/containers/storage:/var/lib/containers/storage \
        "${BIB_IMAGE:-quay.io/centos-bootc/bootc-image-builder:latest}" \
        --type "$type" --use-librepo=True --rootfs=btrfs "$ref"
    sudoif mv -f "$tmp"/* out/
    sudoif rmdir "$tmp"
    sudoif chown -R "$(id -u):$(id -g)" out/
}

render_menu() {
    local raw="$1" lo target device root=""
    target="$(mktemp -d)"
    mkdir -p "${target}/boot"
    lo="$(sudoif losetup --show -fP "$raw")"
    for device in "${lo}"p*; do
        [ -b "$device" ] || continue
        sudoif mount "$device" "${target}/boot" 2> /dev/null || continue
        if [ -d "${target}/boot/loader/entries" ]; then
            root=/target
            break
        fi
        if [ -d "${target}/boot/boot/loader/entries" ]; then
            root=/target/boot
            break
        fi
        sudoif umount "${target}/boot"
    done
    if [ -z "$root" ]; then
        sudoif losetup -d "$lo"
        rmdir "${target}/boot" "$target" 2> /dev/null || true
        die "no partition of the disk carries \`loader/entries\`, so there is no menu to render"
    fi
    local status=0
    sudoif podman run --rm --net=none --security-opt label=disable \
        -v "${target}:/target" --entrypoint "" "$ref" \
        /bin/sh -c "test -x ${RENDERER} || exit 3; exec ${RENDERER} ${root}" || status=$?
    sudoif umount "${target}/boot"
    sudoif losetup -d "$lo"
    rmdir "${target}/boot" "$target" 2> /dev/null || true
    case "$status" in
        0) echo "vm: wrote the boot menu, since this image's GRUB reads no BLS" ;;
        3) ;;
        *) die "${RENDERER} in ${ref} failed: exit ${status}" ;;
    esac
}

install_disk() {
    command -v skopeo > /dev/null 2>&1 \
        || die "skopeo is not installed, and it writes the layout bootc installs from"
    command -v jq > /dev/null 2>&1 || die "jq is not installed, and it reads the install recipe"
    if [ "$type" = qcow2 ]; then
        command -v qemu-img > /dev/null 2>&1 \
            || die "qemu-img is not installed, and a qcow2 is converted from the raw disk"
    fi

    local raw="out/${type}/disk.raw.part" layout="out/oci-cache" key_args=() recipe bootloader
    recipe="$(./scripts/buildc.sh recipe ${target:+--target "$target"})"
    bootloader="$(jq -r .bootloader <<< "$recipe")"
    local install_args=(--filesystem "$(jq -r .filesystem <<< "$recipe")")
    [ "$(jq -r .composeFsBackend <<< "$recipe")" != true ] || install_args+=(--composefs-backend)
    case "$bootloader" in
        "" | grub2) ;;
        *) install_args+=(--bootloader "$bootloader") ;;
    esac
    mkdir -p "out/${type}"
    rm -f "$raw"
    truncate -s "${DISK_SIZE:-20G}" "$raw"

    rm -rf "$layout"
    skopeo copy "containers-storage:${ref}" "oci:${layout}"

    # bootc ignores the root SSH key option on a composefs disk, so the key travels as a systemd credential.
    if [ -n "$ssh_key" ]; then
        [ -s "$ssh_key" ] || die "$ssh_key does not hold an SSH public key"
        key_args=(--karg "systemd.set_credential_binary=ssh.authorized_keys.root:$(base64 -w0 "$ssh_key")")
    fi

    load_rootful
    sudoif podman run \
        --rm --privileged --pull=never \
        --pid=host --net=host \
        -v /var/lib/containers:/var/lib/containers \
        -v /dev:/dev \
        -v "${PWD}/out/${type}":/out \
        -v "${PWD}/${layout}":/oci:ro \
        --security-opt label=type:unconfined_t \
        "$ref" \
        bootc install to-disk --via-loopback --source-imgref oci:/oci \
        --wipe --generic-image "${install_args[@]}" \
        "${key_args[@]}" /out/disk.raw.part

    sudoif chown -R "$(id -u):$(id -g)" out/
    render_menu "$raw"
    if [ "$type" = qcow2 ]; then
        qemu-img convert -O qcow2 "$raw" "${image_file}.part"
        rm -f "$raw"
        mv -f "${image_file}.part" "$image_file"
    else
        mv -f "$raw" "$image_file"
    fi
}

if [ "$command" = build ] || [ "$rebuild" = 1 ] || [ ! -f "$image_file" ]; then
    if [ "$rebuild" = 1 ]; then
        ./scripts/buildc.sh generate
        args=(--backend buildah --tag "$ref")
        [ -z "$target" ] || args+=(--target "$target")
        ./scripts/buildc.sh build "${args[@]}"
    fi
    case "$installer" in
        bib) build_disk ;;
        *) install_disk ;;
    esac
fi

if [ "$login" = 1 ] && [ "$command" != build ]; then
    login_credentials
fi

case "$command" in
    run) run_qemu ;;
    spawn)
        spawn_args=()
        if [ "$login" = 1 ]; then
            spawn_args=(
                --set-credential="sysusers.extra:${sysusers}"
                --set-credential="passwd.hashed-password.${vm_user}:${vm_password_hash}"
            )
        fi
        systemd-vmspawn \
            -M "bootc-image" \
            --console=gui \
            --cpus=2 \
            --ram="$(numfmt --from=iec "$ram")" \
            --network-user-mode \
            --vsock=false --pass-ssh-key=false \
            "${spawn_args[@]}" \
            -i "$image_file"
        ;;
esac
