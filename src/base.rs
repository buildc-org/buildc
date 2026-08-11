use crate::diag::{Issue, Issues, Span};
use crate::model::remote::Collection;
use std::path::Path;

pub const BASES_FILE: &str = "bases.kdl";

pub struct Base {
    pub image: String,
    pub family: String,
    pub provides: Vec<String>,
    pub provides_files: Vec<String>,
    pub about: String,
    pub signed: bool,
    pub span: Span,
}

pub struct Seed {
    pub image: &'static str,
    pub family: &'static str,
    pub provides: &'static [&'static str],
    pub provides_files: &'static [&'static str],
    pub about: &'static str,
}

impl Seed {
    fn base(&self) -> Base {
        Base {
            image: self.image.to_string(),
            family: self.family.to_string(),
            provides: self.provides.iter().map(|n| n.to_string()).collect(),
            provides_files: self.provides_files.iter().map(|n| n.to_string()).collect(),
            about: self.about.to_string(),
            signed: false,
            span: Span::default(),
        }
    }
}

pub const SEED: &[Seed] = &[
    Seed {
        image: "quay.io/fedora/fedora-bootc:44",
        family: "fedora",
        provides: &["rechunking", "initramfs-generation", "mac-policy"],
        provides_files: &[],
        about: "Fedora 44, nothing above the base system",
    },
    Seed {
        image: "ghcr.io/ublue-os/bazzite:stable",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "KDE, gaming and hardware support over kinoite-main",
    },
    Seed {
        image: "ghcr.io/ublue-os/aurora:stable",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "KDE developer workstation over kinoite-main",
    },
    Seed {
        image: "ghcr.io/ublue-os/bluefin:stable",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "GNOME developer workstation over silverblue-main",
    },
    Seed {
        image: "ghcr.io/ublue-os/kinoite-main:44",
        family: "fedora",
        provides: &["rechunking", "flatpak"],
        provides_files: &["/usr/bin/flatpak"],
        about: "Fedora Kinoite with the ublue additions, what bazzite builds on",
    },
];

pub const DEFAULT: &Seed = &SEED[0];

pub struct Shadow {
    pub image: String,
    pub collection: String,
}

pub fn catalog(
    root: &Path,
    sources: &[Collection],
    issues: &mut Issues,
) -> (Vec<Base>, Vec<Shadow>) {
    let mut bases: Vec<Base> = SEED.iter().map(Seed::base).collect();
    let mut shadows: Vec<Shadow> = Vec::new();
    let mut declared: Vec<(String, String)> = Vec::new();

    for collection in sources {
        let Some(dir) = crate::import::cached(root, collection) else {
            continue;
        };
        let Some((found, src)) = crate::parse::bases::read(&dir.join(BASES_FILE), issues) else {
            continue;
        };
        for base in found {
            if let Some((_, first)) = declared.iter().find(|(image, _)| *image == base.image) {
                issues.push(
                    Issue::new(
                        format!("`{}` is described by two collections", base.image),
                        &src,
                    )
                    .at(base.span, format!("`{first}` describes it too"))
                    .help(
                        "one base is one entry: two of them would each write a different family \
                         and a different `provides` into an image scaffolded on it, and which one \
                         did would depend on the order repo.kdl lists the collections in",
                    ),
                );
                continue;
            }
            declared.push((base.image.clone(), collection.name.clone()));
            match bases.iter().position(|known| known.image == base.image) {
                Some(at) => {
                    shadows.push(Shadow {
                        image: base.image.clone(),
                        collection: collection.name.clone(),
                    });
                    bases[at] = base;
                }
                None => bases.push(base),
            }
        }
    }
    (bases, shadows)
}

pub fn find<'a>(bases: &'a [Base], image: &str) -> Option<&'a Base> {
    bases.iter().find(|base| base.image == image)
}
