FROM rootfs AS greeting
RUN --mount=type=bind,from=rootfs,source=/,target=/rootfs,rw \
    cp /rootfs/etc/os-release /os-release

FROM rootfs
COPY --from=greeting /os-release /usr/share/hello-os-release
