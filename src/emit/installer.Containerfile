# renovate: datasource=docker depName=docker.io/library/golang
ARG GO_IMAGE=docker.io/library/golang:1.26
ARG PAYLOAD

FROM ${GO_IMAGE} AS tools
ARG FISHERMAN_COMMIT=027fa25c1d8bc01e2ac97d119cda9e8bb9c99ac7
ARG FISHERMAN_SHA256=ffab2a2c1094fa02a9b4862958c280045c9390425c93195855a9f0f93956c72e
ARG TACKLEBOX_COMMIT=b37b7a2fe9c47a551933e70d6ef023a00abda6ee
ARG TACKLEBOX_SHA256=760ddffbe234159f039508664c0112c663e0a550f41a6e2ffa4f6548bbd0baef
COPY efi-from-image.patch /tmp/efi-from-image.patch
RUN set -eux; \
    fetch() { \
        curl --retry 3 -fsSLo "/tmp/$1.tar.gz" \
            "https://github.com/tuna-os/$1/archive/$2.tar.gz"; \
        echo "$3  /tmp/$1.tar.gz" | sha256sum -c -; \
        mkdir -p "/src/$1"; \
        tar -xf "/tmp/$1.tar.gz" -C "/src/$1" --strip-components=1; \
    }; \
    fetch fisherman "${FISHERMAN_COMMIT}" "${FISHERMAN_SHA256}"; \
    fetch tacklebox "${TACKLEBOX_COMMIT}" "${TACKLEBOX_SHA256}"; \
    git -C /src/tacklebox apply -p1 /tmp/efi-from-image.patch; \
    mkdir -p /out; \
    cd /src/fisherman/fisherman && CGO_ENABLED=0 go build -trimpath -o /out/fisherman ./cmd/fisherman/; \
    cd /src/tacklebox && CGO_ENABLED=0 go build -trimpath -o /out/tacklebox ./cmd/tacklebox

FROM ${PAYLOAD}

RUN set -eux; \
    if command -v apt-get > /dev/null 2>&1; then \
        apt-get update -y; \
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
            podman fuse-overlayfs systemd-cryptsetup cryptsetup skopeo \
            fdisk dosfstools e2fsprogs xfsprogs; \
        apt-get clean -y; \
        rm -rf /var/lib/apt/lists/*; \
    else \
        dnf install -y --setopt=install_weak_deps=False \
            podman fuse-overlayfs cryptsetup skopeo \
            util-linux dosfstools e2fsprogs xfsprogs; \
        dnf clean all; \
    fi; \
    for tool in podman fuse-overlayfs skopeo cryptsetup systemd-cryptenroll \
        sfdisk mkfs.fat mkfs.ext4 mkfs.xfs; do \
        command -v "$tool" > /dev/null 2>&1 \
            || { echo "the live environment has no ${tool}" >&2; exit 1; }; \
    done

COPY --from=tools /out/fisherman /usr/bin/fisherman
COPY recipe.json /usr/share/buildc/install-recipe.json

COPY <<'MOUNT' /usr/lib/systemd/system/var-lib-buildc-store.mount
[Unit]
Description=Offline image store carried by the installer media
ConditionPathExists=/run/initramfs/live/LiveOS/store.squashfs.img

[Mount]
What=/run/initramfs/live/LiveOS/store.squashfs.img
Where=/var/lib/buildc/store
Type=squashfs
Options=ro,loop

[Install]
WantedBy=multi-user.target
MOUNT

COPY <<'CONF' /etc/containers/storage.conf
[storage]
driver = "overlay"
runroot = "/run/containers/storage"
graphroot = "/var/lib/containers/storage"

[storage.options]
additionalimagestores = ["/var/lib/buildc/store"]

[storage.options.overlay]
mount_program = "/usr/bin/fuse-overlayfs"
CONF

COPY <<'AUTOLOGIN' /usr/lib/systemd/system/serial-getty@.service.d/autologin.conf
[Service]
ExecStart=
ExecStart=-/sbin/agetty -o '-p -f -- \\u' --autologin root --keep-baud 115200,57600,38400,9600 - $TERM
AUTOLOGIN

COPY <<'AUTOLOGIN' /usr/lib/systemd/system/getty@.service.d/autologin.conf
[Service]
ExecStart=
ExecStart=-/sbin/agetty -o '-p -f -- \\u' --autologin root --noclear %I $TERM
AUTOLOGIN

RUN systemctl enable var-lib-buildc-store.mount
