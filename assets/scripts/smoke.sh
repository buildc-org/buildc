#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

RENDERER=/usr/libexec/grub-menu-from-bls
PORT=2222

die() {
    echo "smoke: $*" >&2
    exit 1
}

[ "$#" -eq 2 ] || die "usage: scripts/smoke.sh <image-ref in root's store> <work dir>"
ref="$1"
dir="$(realpath -m "$2")"
mkdir -p "$dir"
for tool in jq qemu-system-x86_64 ssh ssh-keygen; do
    command -v "$tool" > /dev/null 2>&1 || die "$tool is not installed"
done

recipe="$(./scripts/buildc.sh recipe)"
filesystem="$(jq -r .filesystem <<< "$recipe")"
composefs="$(jq -r .composeFsBackend <<< "$recipe")"

rm -f "${dir}/key" "${dir}/key.pub" "${dir}/disk.raw"
ssh-keygen -q -t ed25519 -f "${dir}/key" -N '' -C smoke
truncate -s "${DISK_SIZE:-20G}" "${dir}/disk.raw"

# The kernel sends unit status to the last console=, so ttyS0 comes last.
args=(
    --via-loopback --generic-image --filesystem "$filesystem"
    --karg console=tty0 --karg 'console=ttyS0,115200n8'
    --karg systemd.wants=sshd.service
)
mounts=()
# A composefs install reads an OCI layout, so the image is pushed there before bootc runs.
if [ "$composefs" = true ]; then
    sudo rm -rf "${dir}/oci"
    sudo podman push -q "$ref" "oci:${dir}/oci"
    mounts=(-v "${dir}/oci:/oci:ro")
    args+=(--composefs-backend --source-imgref oci:/oci)
fi
sudo podman run --rm --privileged --pull=never --pid=host --net=host \
    --security-opt label=type:unconfined_t \
    -v /dev:/dev \
    -v /var/lib/containers/storage:/var/lib/containers/storage \
    -v "${dir}:/output" \
    "${mounts[@]}" \
    "$ref" bootc install to-disk "${args[@]}" /output/disk.raw

render_menu() {
    local lo target device root="" status=0
    target="$(mktemp -d)"
    mkdir "${target}/boot"
    lo="$(sudo losetup --show -fP "${dir}/disk.raw")"
    for device in "${lo}"p*; do
        [ -b "$device" ] || continue
        sudo mount "$device" "${target}/boot" 2> /dev/null || continue
        if [ -d "${target}/boot/loader/entries" ]; then
            root=/target
            break
        fi
        if [ -d "${target}/boot/boot/loader/entries" ]; then
            root=/target/boot
            break
        fi
        sudo umount "${target}/boot"
    done
    if [ -z "$root" ]; then
        sudo losetup -d "$lo"
        die "no partition of the disk carries \`loader/entries\`"
    fi
    sudo podman run --rm --net=none --security-opt label=disable \
        -v "${target}:/target" --entrypoint "" "$ref" \
        /bin/sh -c "test -x ${RENDERER} || exit 3; exec ${RENDERER} ${root}" || status=$?
    sudo umount "${target}/boot"
    sudo losetup -d "$lo"
    rmdir "${target}/boot" "$target"
    case "$status" in
        0) echo "smoke: rendered the boot menu" ;;
        3) ;;
        *) die "${RENDERER} failed: exit ${status}" ;;
    esac
}
render_menu
sudo chown "$(id -u):$(id -g)" "${dir}/disk.raw"

for code in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
    [ -f "$code" ] && break
done
[ -f "$code" ] || die "no OVMF firmware is installed"
cp "${code/CODE/VARS}" "${dir}/vars.fd"
# shellcheck disable=SC2016 # expanded by systemd, from each family's EnvironmentFile
dropin="$(printf '%s\n' '[Service]' 'ExecStart=' \
    'ExecStart=/usr/sbin/sshd -D -oPermitRootLogin=prohibit-password $OPTIONS $SSHD_OPTS' \
    | base64 -w0)"
qemu-system-x86_64 \
    -smbios "type=11,value=io.systemd.credential.binary:ssh.authorized_keys.root=$(base64 -w0 "${dir}/key.pub")" \
    -smbios "type=11,value=io.systemd.credential.binary:systemd.unit-dropin.sshd.service=${dropin}" \
    -smbios "type=11,value=io.systemd.credential.binary:systemd.unit-dropin.ssh.service=${dropin}" \
    -machine q35 -m "${RAM:-4096}" -smp 2 -cpu host -enable-kvm -display none \
    -drive "if=pflash,unit=0,format=raw,readonly=on,file=${code}" \
    -drive "if=pflash,unit=1,format=raw,file=${dir}/vars.fd" \
    -serial "file:${dir}/console.log" \
    -drive "file=${dir}/disk.raw,if=virtio,format=raw" \
    -nic "user,hostfwd=tcp:127.0.0.1:${PORT}-:22" \
    -daemonize -pidfile "${dir}/qemu.pid" \
    > "${dir}/qemu.log" 2>&1

for i in $(seq 60); do
    if ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -o ConnectTimeout=5 -o LogLevel=ERROR -i "${dir}/key" -p "$PORT" \
        root@localhost true 2> /dev/null; then
        echo "smoke: ssh answered after ${i} attempts"
        exit 0
    fi
    sleep 10
done
die "ssh did not answer in ten minutes; the serial console is ${dir}/console.log"
