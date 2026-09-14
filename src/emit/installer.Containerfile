# renovate: datasource=docker depName=docker.io/library/golang
ARG GO_IMAGE=docker.io/library/golang:1.26
# renovate: datasource=docker depName=quay.io/fedora/fedora-bootc
ARG LIVE_BASE=quay.io/fedora/fedora-bootc:44

FROM ${GO_IMAGE} AS tools
ARG FISHERMAN_ORG=buildc-org
ARG FISHERMAN_COMMIT=2fec4b24fcd26f139323f99e666099c733f5b7fd
ARG FISHERMAN_SHA256=5504225365e92f6ec550cce84ec1aeddb4fed806ea7662ecd69bfa65a85f07c6
ARG TACKLEBOX_ORG=buildc-org
ARG TACKLEBOX_COMMIT=da820e5e9ac34c7a851382a9a1c7a4c6bd0c886f
ARG TACKLEBOX_SHA256=c873e6a102640b2688de126238e2faae3abfd1c40370bf46df2906965d9c0439
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
        dnf install -y --setopt=install_weak_deps=False kmscon || true; \
        dnf clean all; \
    fi; \
    for tool in podman fuse-overlayfs skopeo cryptsetup systemd-cryptenroll \
        openssl sfdisk mkfs.fat mkfs.ext4 mkfs.xfs; do \
        command -v "$tool" > /dev/null 2>&1 \
            || { echo "the live environment has no ${tool}" >&2; exit 1; }; \
    done; \
    command -v kmscon > /dev/null 2>&1 \
        || echo "no kmscon here; the console falls back to the kernel VT" >&2

# The installer media loads its squashfs store from a USB device at boot.
# A modprobe.d blacklist or install line would stop these modules loading.
ARG MEDIA_MODULES="usb_storage uas squashfs loop overlay isofs vfat"
RUN set -eux; \
    pattern="$(echo "$MEDIA_MODULES" | sed 's/_/[-_]/g; s/ /|/g')"; \
    find /etc/modprobe.d /usr/lib/modprobe.d -name '*.conf' -type f -exec sed -i -E \
        "/^[[:space:]]*(install|blacklist)[[:space:]]+(${pattern})([[:space:]]|\$)/d" {} +; \
    kver="$(find /usr/lib/modules -mindepth 1 -maxdepth 1 -printf '%f\n' | head -n1)"; \
    blocked="$(modprobe -S "$kver" -c 2> /dev/null \
        | grep -E "^(install|blacklist) ($(echo "$MEDIA_MODULES" | tr ' ' '|'))( |\$)" || true)"; \
    [ -z "$blocked" ] || { echo "the media cannot load: ${blocked}" >&2; exit 1; }; \
    systemctl mask usbguard.service fapolicyd.service auditd.service; \
    grep -rlZ TMOUT /etc/profile /etc/profile.d /etc/bashrc /etc/bash.bashrc 2> /dev/null \
        | xargs -0r sed -i '/TMOUT/d'; \
    systemctl set-default multi-user.target

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

# The installer draws its interface on the console.
# Kernel messages at the default level overwrite the interface.
# The console loglevel therefore admits alerts and emergencies alone.
COPY <<'QUIET' /usr/lib/sysctl.d/50-bootc-anywhere-console.conf
kernel.printk = 1 4 1 4
QUIET

RUN systemctl enable var-lib-buildc-store.mount

COPY buildc /usr/bin/buildc
RUN /usr/bin/buildc --version

# kmscon needs a DRM card to draw the installer on the console.
# Without one, the assertion fails and systemd starts bootc-anywhere-vt.service.
COPY <<'UNIT' /usr/lib/systemd/system/bootc-anywhere.service
[Unit]
Description=Install this image onto a disk
After=var-lib-buildc-store.mount systemd-user-sessions.service getty@tty1.service
Conflicts=getty@tty1.service
AssertPathExistsGlob=/dev/dri/card*
OnFailure=bootc-anywhere-vt.service
StartLimitIntervalSec=60
StartLimitBurst=10

[Service]
Type=idle
ExecStart=/usr/bin/kmscon --vt=1 --no-switchvt --oneshot --login -- /usr/bin/buildc installer
Restart=always
RestartSec=1

[Install]
WantedBy=multi-user.target
UNIT

COPY <<'UNIT' /usr/lib/systemd/system/bootc-anywhere-vt.service
[Unit]
Description=Install this image onto a disk, on the kernel console
After=var-lib-buildc-store.mount systemd-user-sessions.service getty@tty1.service
Conflicts=getty@tty1.service
StartLimitIntervalSec=0

[Service]
Type=idle
ExecStart=/usr/bin/buildc installer
Restart=always
RestartSec=1
StandardInput=tty
StandardOutput=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes

[Install]
WantedBy=multi-user.target
UNIT

RUN set -eux; \
    systemctl mask getty@tty1.service autovt@tty1.service; \
    if command -v kmscon > /dev/null 2>&1; then \
        systemctl enable bootc-anywhere.service; \
    else \
        systemctl enable bootc-anywhere-vt.service; \
    fi
