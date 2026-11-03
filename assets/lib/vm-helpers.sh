# shellcheck disable=SC2154
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
        sudoif podman image exists "$ref" || sudoif podman pull "$ref"
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

login_credentials() {
    vm_user="${VM_USER:-buildc}"
    # The name reaches a systemd credential key and a sysusers line, so it is held to the Linux account-name alphabet.
    if ! [[ "$vm_user" =~ ^[a-z_][a-z0-9_-]*$ ]] || [ "${#vm_user}" -gt 31 ]; then
        die "VM_USER must be a Linux user name of at most 31 characters"
    fi
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

partitioned() {
    [ -f "$1" ] || return 1
    [ "$(dd if="$1" bs=1 skip=510 count=2 2> /dev/null | od -An -tx1 | tr -d ' \n')" = "55aa" ]
}

run_qemu() {
    local port=8006 arguments sysusers_base64 credential_args=() storage=() media=()
    if [ "$type" = iso ]; then
        mkdir -p "${staged}/storage"
        storage=(--volume "${PWD}/${staged}/storage":/storage)
    fi
    media=(--volume "${PWD}/${image_file}":"/boot.${type}")
    if [ "$type" = iso ] && partitioned "${staged}/storage/data.img"; then
        media=()
        echo "vm: the storage disk has a partition table; booting the disk"
        echo "vm: remove ${staged}/storage to boot the installer again"
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
        "${media[@]}" \
        docker.io/qemux/qemu
}
