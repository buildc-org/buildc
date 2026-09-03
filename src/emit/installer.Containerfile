# renovate: datasource=docker depName=docker.io/library/golang
ARG GO_IMAGE=docker.io/library/golang:1.26
# renovate: datasource=docker depName=quay.io/fedora/fedora-bootc
ARG LIVE_BASE=quay.io/fedora/fedora-bootc:44

FROM ${GO_IMAGE} AS tools
ARG FISHERMAN_ORG=buildc-org
ARG FISHERMAN_COMMIT=8c48ef50c4ab2922bd3a84139b6d23e67836e7de
ARG FISHERMAN_SHA256=5e05d284ec83902c4800532c8a4902dc69adddac3a060f83bff03d69765bbdae
ARG TACKLEBOX_ORG=buildc-org
ARG TACKLEBOX_COMMIT=e5f34b19c76cc3d7bc575fe28beaeee003d756fb
ARG TACKLEBOX_SHA256=a1c801b193b79755c6fc71faa398fe6c0932d63b064a668c9ac83905c7bea7d1
COPY efi-from-image.patch /tmp/efi-from-image.patch
RUN set -eux; \
    fetch() { \
        curl --retry 3 -fsSLo "/tmp/$2.tar.gz" \
            "https://github.com/$1/$2/archive/$3.tar.gz"; \
        echo "$4  /tmp/$2.tar.gz" | sha256sum -c -; \
        mkdir -p "/src/$2"; \
        tar -xf "/tmp/$2.tar.gz" -C "/src/$2" --strip-components=1; \
    }; \
    fetch "${FISHERMAN_ORG}" fisherman "${FISHERMAN_COMMIT}" "${FISHERMAN_SHA256}"; \
    fetch "${TACKLEBOX_ORG}" tacklebox "${TACKLEBOX_COMMIT}" "${TACKLEBOX_SHA256}"; \
    git -C /src/tacklebox apply -p1 /tmp/efi-from-image.patch; \
    mkdir -p /out; \
    cd /src/fisherman/fisherman && CGO_ENABLED=0 go build -trimpath -o /out/fisherman ./cmd/fisherman/; \
    cd /src/tacklebox && CGO_ENABLED=0 go build -trimpath -o /out/tacklebox ./cmd/tacklebox

FROM ${LIVE_BASE}

RUN set -eux; \
    if command -v apt-get > /dev/null 2>&1; then \
        apt-get update -y; \
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
            podman fuse-overlayfs systemd-cryptsetup cryptsetup skopeo openssl \
            fdisk dosfstools e2fsprogs xfsprogs; \
        apt-get clean -y; \
        rm -rf /var/lib/apt/lists/*; \
    else \
        dnf install -y --setopt=install_weak_deps=False \
            podman fuse-overlayfs cryptsetup skopeo openssl \
            util-linux dosfstools e2fsprogs xfsprogs; \
        dnf clean all; \
    fi; \
    for tool in podman fuse-overlayfs skopeo cryptsetup systemd-cryptenroll \
        openssl sfdisk mkfs.fat mkfs.ext4 mkfs.xfs; do \
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

COPY buildc /usr/bin/buildc
RUN /usr/bin/buildc --version

COPY <<'START' /etc/profile.d/bootc-anywhere.sh
if [ "$(id -u)" = 0 ] && [ -t 0 ]; then
    buildc install
fi
START
