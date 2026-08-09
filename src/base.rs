pub struct Base {
    pub image: &'static str,
    pub family: &'static str,
    pub provides: &'static [&'static str],
    pub provides_files: &'static [&'static str],
    pub about: &'static str,
}

pub const CATALOG: &[Base] = &[
    Base {
        image: "quay.io/fedora/fedora-bootc:44",
        family: "fedora",
        provides: &["rechunking", "initramfs-generation", "mac-policy"],
        provides_files: &[],
        about: "Fedora 44, nothing above the base system",
    },
    Base {
        image: "ghcr.io/ublue-os/bazzite:stable",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "KDE, gaming and hardware support over kinoite-main",
    },
    Base {
        image: "ghcr.io/ublue-os/aurora:stable",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "KDE developer workstation over kinoite-main",
    },
    Base {
        image: "ghcr.io/ublue-os/bluefin:stable",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "GNOME developer workstation over silverblue-main",
    },
    Base {
        image: "ghcr.io/ublue-os/kinoite-main:44",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "Fedora Kinoite with the ublue additions, what bazzite builds on",
    },
];

pub const DEFAULT: &Base = &CATALOG[0];

pub fn find(image: &str) -> Option<&'static Base> {
    CATALOG.iter().find(|base| base.image == image)
}
