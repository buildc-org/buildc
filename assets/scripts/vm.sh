#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
    echo "vm: $*" >&2
    exit 1
}

usage() {
    cat >&2 << 'EOF'
usage: scripts/vm.sh <command> <qcow2|raw|iso> [options]

  build     convert the container image into a disk image under out/
  run       boot that disk image under qemu, building it if it is missing
  spawn     boot it with systemd-vmspawn instead (qcow2 or raw)

  --image <ref>       container image to convert (default: localhost/ and
                      the plan's ungated published name)
  --tag <tag>         its tag (default: $DEFAULT_TAG, else latest)
  --target <image/flavour>
                      what a rebuild builds and what an iso installs
                      (default: the plan's ungated target)
  --rebuild           fetch modules, generate, then build the container image
                      first. The one form of this that changes the repository.
  --ram <size>        memory for the virtual machine (default: 8G)
  --installer <which> what converts the image into a disk: `bib` for
                      bootc-image-builder, `bootc` for `bootc install to-disk`.
                      `buildc vm` passes this off the target's base family; the
                      default here is bib, which is fedora-only. An iso is
                      converted by neither and passes none.
  --live-image <ref>  what the installer media boots, built here from
                      out/bootiso/Containerfile. `buildc vm` names it, because
                      this script has no JSON reader to find it with.

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
    */qcow2 | */raw | build/iso | run/iso) ;;
    spawn/iso) die "systemd-vmspawn cannot boot an installer iso" ;;
    *) die "'${command}' needs an image type (qcow2, raw or iso)" ;;
esac

image=""
tag="${DEFAULT_TAG:-latest}"
target=""
rebuild=0
ram=8G
installer=bib
live_image=""
login=0
ssh_key=""

while [ $# -gt 0 ]; do
    case "$1" in
        --image | --tag | --target | --ram | --installer | --ssh-key | --live-image)
            [ "$#" -ge 2 ] || die "$1 needs a value"
            case "$1" in
                --image) image="$2" ;;
                --tag) tag="$2" ;;
                --target) target="$2" ;;
                --ram) ram="$2" ;;
                --ssh-key) ssh_key="$2" ;;
                --live-image) live_image="$2" ;;
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

staged=out/bootiso
image_file="out/${type}/disk.${type}"
[ "$type" != iso ] || image_file="${staged}/install.iso"

sudoif() {
    if [ "${UID}" -eq 0 ]; then
        "$@"
    elif [ -n "${SUDO_ASKPASS:-}${SSH_ASKPASS:-}" ] && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
        SUDO_ASKPASS="${SUDO_ASKPASS:-${SSH_ASKPASS}}" sudo --askpass "$@"
    else
        sudo "$@"
    fi
}

# The disk builders run rootful and read root's store, so a rootless build's image is copied across when the image IDs differ.
load_rootful() {
    if [ "${UID}" -eq 0 ] || [ -n "${SUDO_USER:-}" ]; then
        return 0
    fi
    if ! podman image exists "$ref"; then
        sudoif podman pull "$ref"
        return 0
    fi
    local mine theirs tmp
    mine="$(podman images --filter reference="$ref" --format '{{.ID}}')"
    theirs="$(sudoif podman images --filter reference="$ref" --format '{{.ID}}')"
    [ "$mine" != "$theirs" ] || return 0
    mkdir -p out
    tmp="$(mktemp -p "${PWD}/out" -d scp.XXXXXXXX)"
    sudoif env "TMPDIR=${tmp}" podman image scp \
        "${UID}@localhost::${ref}" "root@localhost::${ref}"
    rm -rf "$tmp"
}

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

install_disk() {
    command -v skopeo > /dev/null 2>&1 \
        || die "skopeo is not installed, and it writes the layout bootc installs from"
    if [ "$type" = qcow2 ]; then
        command -v qemu-img > /dev/null 2>&1 \
            || die "qemu-img is not installed, and a qcow2 is converted from the raw disk"
    fi

    local raw="out/${type}/disk.raw.part" layout="out/oci-cache" key_args=() key_mount=()
    mkdir -p "out/${type}"
    rm -f "$raw"
    truncate -s "${DISK_SIZE:-20G}" "$raw"

    rm -rf "$layout"
    skopeo copy "containers-storage:${ref}" "oci:${layout}"

    # bootc ignores the root SSH key option on a composefs disk, so the key travels as a systemd credential.
    if [ -n "$ssh_key" ]; then
        [ -s "$ssh_key" ] || die "$ssh_key does not hold an SSH public key"
        key_mount=(-v "$(realpath "$ssh_key"):/root-ssh-authorized-keys:ro")
        key_args=(--root-ssh-authorized-keys /root-ssh-authorized-keys)
    fi

    load_rootful
    sudoif podman run \
        --rm --privileged --pull=never \
        --pid=host --net=host \
        -v /var/lib/containers:/var/lib/containers \
        -v /dev:/dev \
        -v "${PWD}/out/${type}":/out \
        -v "${PWD}/${layout}":/oci:ro \
        "${key_mount[@]}" \
        --security-opt label=type:unconfined_t \
        "$ref" \
        bootc install to-disk --via-loopback --composefs-backend \
        --source-imgref oci:/oci \
        --filesystem ext4 --wipe --generic-image \
        "${key_args[@]}" /out/disk.raw.part

    sudoif chown -R "$(id -u):$(id -g)" out/
    if [ "$type" = qcow2 ]; then
        qemu-img convert -O qcow2 "$raw" "${image_file}.part"
        rm -f "$raw"
        mv -f "${image_file}.part" "$image_file"
    else
        mv -f "$raw" "$image_file"
    fi
}

build_iso() {
    local tools=localhost/bootc-anywhere-tools:latest tbx="${staged}/tacklebox" cid
    local build="${PWD}/${staged}/build"
    [ -n "$live_image" ] || die "--live-image names what the media boots; \`buildc vm\` passes it"
    [ -f "${staged}/Containerfile" ] \
        || die "${staged}/Containerfile is not there; \`buildc vm build iso\` stages it"

    load_rootful

    sudoif podman build --target tools -t "$tools" \
        --build-arg "PAYLOAD=${ref}" "$staged"
    cid="$(sudoif podman create "$tools")"
    sudoif podman cp "${cid}:/out/tacklebox" "$tbx"
    sudoif podman rm "$cid" > /dev/null
    sudoif chown "$(id -u):$(id -g)" "$tbx"

    sudoif podman build --build-arg "PAYLOAD=${ref}" -t "$live_image" "$staged"

    if mountpoint -q "${build}/tbox-offline-store/overlay" 2> /dev/null; then
        sudoif umount "${build}/tbox-offline-store/overlay"
    fi

    rm -f "${image_file}.part"
    if ! sudoif env HOME=/root "$tbx" build "${PWD}/${staged}/media.json" \
        --iso "${PWD}/${image_file}.part" -b "$build"; then
        sudoif chown -R "$(id -u):$(id -g)" "$staged"
        die "tacklebox could not assemble the media; it left its staging under ${build}"
    fi
    sudoif chown -R "$(id -u):$(id -g)" "$staged"
    mv -f "${image_file}.part" "$image_file"
}

login_credentials() {
    vm_user="${VM_USER:-buildc}"
    [[ "$vm_user" =~ ^[a-z_][a-z0-9_-]*$ ]] && [ "${#vm_user}" -le 31 ] \
        || die "VM_USER must be a Linux user name of at most 31 characters"
    vm_password_hash="${VM_PASSWORD_HASH:-}"
    if [ -z "$vm_password_hash" ]; then
        [ -t 0 ] || die "set VM_PASSWORD_HASH when no terminal can ask for a VM password"
        command -v openssl > /dev/null 2>&1 \
            || die "openssl is not installed, and it hashes the VM password"
        read -rsp "vm: password for ${vm_user}: " vm_password
        echo
        [ -n "$vm_password" ] || die "the VM password cannot be empty"
        read -rsp "vm: password again: " repeated
        echo
        [ "$vm_password" = "$repeated" ] || die "the VM passwords do not match"
        vm_password_hash="$(printf '%s\n' "$vm_password" | openssl passwd -6 -stdin)"
        unset vm_password repeated
    fi
    # The run path passes the hash in a space-separated string, so whitespace would split the credential.
    [[ "$vm_password_hash" != *[[:space:]]* ]] \
        || die "VM_PASSWORD_HASH must not contain whitespace"
    sysusers="u ${vm_user} - \"VM user\" /var/home/${vm_user} /bin/bash"
    echo "vm: login as ${vm_user}; credentials provision the account on its first boot"
}

run_qemu() {
    local port=8006 arguments sysusers_base64 credential_args=() storage=()
    if [ "$type" = iso ]; then
        mkdir -p "${staged}/storage"
        storage=(--volume "${PWD}/${staged}/storage":/storage)
    fi
    while ss -tunal | grep -q ":${port} "; do
        port=$((port + 1))
    done
    if [ "$login" = 1 ]; then
        sysusers_base64="$(printf %s "$sysusers" | base64 -w0)"
        arguments="-smbios type=11,value=io.systemd.credential.binary:sysusers.extra=${sysusers_base64}"
        arguments+=" -smbios type=11,value=io.systemd.credential:passwd.hashed-password.${vm_user}=${vm_password_hash}"
        credential_args=(--env "ARGUMENTS=${arguments}")
    fi
    echo "vm: connect to http://localhost:${port}"
    (sleep 30 && xdg-open "http://localhost:${port}") &
    podman run \
        --rm --privileged --pull=newer \
        --publish "127.0.0.1:${port}:8006" \
        --env CPU_CORES=4 \
        --env "RAM_SIZE=${ram}" \
        --env DISK_SIZE=64G \
        --env TPM=Y \
        --env "GPU=${GPU:-N}" \
        "${credential_args[@]}" \
        "${storage[@]}" \
        --device=/dev/kvm \
        --volume "${PWD}/${image_file}":"/boot.${type}" \
        docker.io/qemux/qemu
}

if [ "$command" = build ] || [ "$rebuild" = 1 ] || [ ! -f "$image_file" ]; then
    if [ "$rebuild" = 1 ]; then
        ./scripts/buildc.sh fetch modules
        ./scripts/buildc.sh generate
        args=(--tag "$ref")
        [ -z "$target" ] || args+=(--target "$target")
        ./scripts/buildc.sh build "${args[@]}"
    fi
    case "$type" in
        iso) build_iso ;;
        *)
            case "$installer" in
                bib) build_disk ;;
                *) install_disk ;;
            esac
            ;;
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
