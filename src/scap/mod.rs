mod xml;

#[cfg(feature = "fuzzing")]
pub fn fuzz_xml(text: &str) {
    let _ = xml::scan(text, |_| {});
}

use crate::diag::{Issue, Issues, Source, Span};
use crate::emit::plan::of_target;
use crate::model::image::{Entry, Image, List, NO_FLAVOUR};
use crate::provider::Index;
use crate::resolve::overlay;
use common::json::{self, Json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const CONTENT: &str = "/usr/share/xml/scap/ssg/content";

#[derive(Default)]
pub struct Options {
    pub target: Option<String>,
    pub datastream: Option<PathBuf>,
    pub baseline: Option<PathBuf>,
    pub base: Option<PathBuf>,
}

pub enum Verdict {
    Clean,
    Wrong,
}

pub fn content(root: &Path, named: Option<&str>) -> Result<Verdict, String> {
    let Some((list, _)) = open(root, named) else {
        return Ok(Verdict::Wrong);
    };
    println!("{}", datastream(root, &list, named)?);
    Ok(Verdict::Clean)
}

pub fn content_on_host(image: &Json, target: &Json) -> Result<Verdict, String> {
    let path = match json::text(target, "conforms")
        .unwrap_or_default()
        .is_empty()
    {
        true => String::new(),
        false => installed(
            CONTENT,
            &json::field(image, "base")
                .and_then(|base| json::text(base, "family"))
                .unwrap_or_default(),
        )?
        .display()
        .to_string(),
    };
    println!("{path}");
    Ok(Verdict::Clean)
}

pub fn conformance(
    list: &List,
    index: &Index,
    datastream: Option<&Path>,
) -> Result<Vec<String>, String> {
    let content = match datastream {
        Some(path) => Some(content_of(path)?),
        None => None,
    };
    let mut out = Vec::new();
    let measured = list
        .images
        .iter()
        .filter(|i| !i.conforms.is_empty() || i.flavours.iter().any(|f| !f.conforms.is_empty()));
    for image in measured {
        out.extend(unremediated(image, index));
        out.extend(allows_nothing(image));
        if let Some(content) = &content {
            out.extend(claimed_and_refused(image, content));
            out.extend(claims_nothing(image, content));
            out.extend(refuses_nothing(image, content));
        }
        match &content {
            Some(content) => out.extend(unclaimed(image, content, index)),
            None if image.modules().any(|m| !m.satisfies.is_empty())
                || image.base.iter().any(|base| !base.satisfies.is_empty()) => {}
            None => out.push(format!(
                "`{}` conforms to `{}` and nothing it installs or builds on declares `satisfies`, \
                 so nothing here claims a rule of it. Nothing read a datastream, so that is a \
                 count of declarations: `buildc check --datastream <file>` says which of the \
                 profile's rules are unclaimed",
                image.id, image.conforms
            )),
        }
    }
    Ok(out)
}

pub fn exclusions(image: &Image) -> (Vec<String>, Vec<String>) {
    let claimed = crate::emit::plan::distinct(
        image
            .base
            .iter()
            .flat_map(|base| base.satisfies.iter())
            .chain(image.modules().flat_map(|module| module.satisfies.iter()))
            .flat_map(|coverage| coverage.rules.iter().cloned()),
    );
    let lifted: BTreeSet<&str> = image
        .allows
        .iter()
        .map(|allow| rule_name(&allow.rule))
        .collect();
    let refused = crate::emit::plan::distinct(
        image
            .modules()
            .flat_map(|module| module.refuses.iter().map(|r| rule_name(&r.rule)))
            .filter(|rule| !lifted.contains(rule))
            .map(str::to_string),
    );
    (claimed, refused)
}

const REMEDIATION: &str = "scap-remediation";

fn unremediated(image: &Image, index: &Index) -> Option<String> {
    if index.providing(REMEDIATION).is_empty() {
        return None;
    }
    let has = image
        .modules()
        .any(|m| m.provides.iter().any(|p| p.name == REMEDIATION))
        || image
            .base
            .iter()
            .any(|base| base.provides.iter().any(|p| p.name == REMEDIATION));
    (!has).then(|| {
        format!(
            "`{}` conforms to `{}` and nothing it installs provides `{REMEDIATION}`, so the \
             profile is measured and nothing acts on it. That is a scan report rather than a \
             hardened image; add a module providing `{REMEDIATION}`, or drop `conforms`",
            image.id, image.conforms
        )
    })
}

fn refuses_nothing(image: &Image, content: &Content) -> Vec<String> {
    let known: BTreeSet<&str> = content.ids.iter().map(|id| rule_name(id)).collect();
    image
        .modules()
        .flat_map(|module| {
            module
                .refuses
                .iter()
                .map(move |refusal| (module.path.as_str(), refusal))
        })
        .filter(|(_, refusal)| !known.contains(rule_name(&refusal.rule)))
        .map(|(path, refusal)| {
            format!(
                "`{path}` refuses `{}`, which the content it is measured against carries no rule \
                 by. A refusal that reaches no rule keeps nothing out of remediation and reads \
                 exactly like one that does",
                refusal.rule
            )
        })
        .collect()
}

fn claims_nothing(image: &Image, content: &Content) -> Vec<String> {
    let claimants = image
        .base
        .iter()
        .map(|base| ("its base", &base.satisfies))
        .chain(image.modules().map(|m| (m.path.as_str(), &m.satisfies)));
    claimants
        .flat_map(|(who, satisfies)| satisfies.iter().map(move |coverage| (who, coverage)))
        .filter_map(|(who, coverage)| {
            let lost: Vec<&str> = coverage
                .rules
                .iter()
                .filter(|number| !content.rules.contains_key(*number))
                .map(String::as_str)
                .collect();
            (!lost.is_empty()).then(|| {
                format!(
                    "`{who}` claims {} under `{}`, which the content `{}` is measured against \
                     carries no rule by. The remediation hook resolves a claim to the rule it \
                     protects, so this stops the build in the finalize layer",
                    lost.join(", "),
                    coverage.benchmark,
                    image.id
                )
            })
        })
        .collect()
}

fn claimed_and_refused(image: &Image, content: &Content) -> Vec<String> {
    let (claimed, refused) = exclusions(image);
    let refused: BTreeSet<&str> = refused.iter().map(String::as_str).collect();
    claimed
        .iter()
        .filter_map(|number| Some((number, content.rules.get(number)?)))
        .filter(|(_, rule)| refused.contains(rule_name(rule)))
        .map(|(number, rule)| {
            format!(
                "`{}` claims `{number}` and something it installs refuses `{}`, which is the \
                 same rule. A claim says the image passes it and a refusal says nothing may set \
                 it, so one of the two is wrong",
                image.id,
                rule_name(rule)
            )
        })
        .collect()
}

pub struct Owed<'a> {
    pub selects: usize,
    pub open: BTreeSet<String>,
    pub helping: Vec<&'a crate::provider::Provider>,
    pub covered: BTreeSet<String>,
}

pub fn owed<'a>(image: &Image, content: &Content, profile: &Profile, index: &'a Index) -> Owed<'a> {
    let selected = content.selected(&profile.id);
    let claimed = reached(
        content,
        image
            .modules()
            .flat_map(|module| module.satisfies.iter())
            .chain(image.base.iter().flat_map(|base| base.satisfies.iter()))
            .flat_map(|coverage| coverage.rules.iter()),
    );
    let open: BTreeSet<String> = selected.difference(&claimed).cloned().collect();
    let listed: BTreeSet<String> = image
        .entries
        .iter()
        .chain(&image.suppressed)
        .map(Entry::dir)
        .collect();
    let helping: Vec<&crate::provider::Provider> = match open.is_empty() {
        true => Vec::new(),
        false => index
            .claiming(&content.numbering(&open))
            .into_iter()
            .filter(|provider| !listed.contains(&provider.dir()))
            .collect(),
    };
    let covered: BTreeSet<String> = helping
        .iter()
        .flat_map(|provider| reached(content, provider.declares.satisfies.iter()))
        .filter(|rule| open.contains(rule))
        .collect();
    Owed {
        selects: selected.len(),
        open,
        helping,
        covered,
    }
}

pub fn reached<'a>(
    content: &Content,
    numbers: impl Iterator<Item = &'a String>,
) -> BTreeSet<String> {
    numbers
        .filter_map(|number| content.rules.get(number))
        .cloned()
        .collect()
}

pub(crate) fn profile_names(content: &Content) -> String {
    let names: Vec<&str> = content.profiles.iter().map(Profile::name).collect();
    match names.is_empty() {
        true => "no profile at all".into(),
        false => names.join(", "),
    }
}

fn unclaimed(image: &Image, content: &Content, index: &Index) -> Option<String> {
    let Some(profile) = content.profiles.iter().find(|p| p.is(&image.conforms)) else {
        return Some(format!(
            "`{}` conforms to `{}`, which is none of the profiles the datastream carries: {}",
            image.id,
            image.conforms,
            profile_names(content)
        ));
    };
    let owed = owed(image, content, profile, index);
    if owed.open.is_empty() {
        return None;
    }
    let named: Vec<String> = owed
        .helping
        .iter()
        .map(|provider| format!("`{}`", provider.qualified()))
        .collect();

    let searched = match index.unread().is_empty() && index.sourced() {
        true => "the repository or its collections",
        false => "the repository",
    };
    let found = match named.is_empty() {
        true => format!("nothing else in {searched} claims them"),
        false => format!(
            "{} would claim {} of them",
            named.join(", "),
            owed.covered.len()
        ),
    };
    let unsearched = match index.unsearched() {
        clause if clause.is_empty() => String::new(),
        clause => format!(". {clause}"),
    };
    Some(format!(
        "`{}` conforms to `{}`, and nothing it installs or builds on claims {} of the {} rules it \
         selects; {found}{unsearched}",
        image.id,
        image.conforms,
        owed.open.len(),
        owed.selects
    ))
}

fn allows_nothing(image: &Image) -> Vec<String> {
    let refused: BTreeSet<&str> = image
        .modules()
        .flat_map(|module| module.refuses.iter().map(|r| rule_name(&r.rule)))
        .collect();
    image
        .allows
        .iter()
        .filter(|allow| !refused.contains(rule_name(&allow.rule)))
        .map(|allow| {
            format!(
                "`{}` allows remediation of `{}` and nothing it installs refuses that rule, so \
                 the allowance lifts nothing. A rule no module refuses is remediated already",
                image.id, allow.rule
            )
        })
        .collect()
}

fn open(root: &Path, named: Option<&str>) -> Option<(List, Vec<crate::resolve::Resolved>)> {
    let crate::Loaded {
        list,
        resolved,
        mut issues,
        context,
        ..
    } = crate::load(root);
    if named.is_none() {
        if let Some(issue) = list.no_default() {
            issues.push(issue);
        }
    }
    match issues.report(&context) {
        true => None,
        false => Some((list, resolved)),
    }
}

pub fn content_for(root: &Path, list: &List, named: Option<&str>) -> Result<String, String> {
    datastream(root, list, named)
}

const MEASURED: &str = "xccdf_tect_profile_measured";

pub fn tailoring_for(
    root: &Path,
    named: Option<&str>,
    given: Option<&Path>,
) -> Result<Verdict, String> {
    let Some((list, _)) = open(root, named) else {
        return Ok(Verdict::Wrong);
    };
    let name = target(&list, named)?;
    let (image, flavour, _) = of_target(&list, &name).ok_or_else(|| unknown(&name))?;
    let conforms = image.conforms_of(flavour.as_deref().unwrap_or(NO_FLAVOUR));
    if conforms.is_empty() {
        return Ok(Verdict::Clean);
    }
    let path = match given {
        Some(path) => path.to_path_buf(),
        None => PathBuf::from(datastream(root, &list, Some(&name))?),
    };
    let content = content_of(&path)?;
    let profile = content
        .profiles
        .iter()
        .find(|p| p.is(conforms))
        .ok_or_else(|| {
            format!(
                "`{name}` conforms to `{conforms}`, which is none of the profiles {} carries: {}",
                path.display(),
                profile_names(&content)
            )
        })?;
    print!("{}", tailoring(&content, profile));
    Ok(Verdict::Clean)
}

fn tailoring(content: &Content, profile: &Profile) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <xccdf-1.2:Tailoring xmlns:xccdf-1.2=\"http://checklists.nist.gov/xccdf/1.2\" \
         id=\"xccdf_tect_tailoring_measured\">\n  \
         <xccdf-1.2:version time=\"1970-01-01T00:00:00\">1</xccdf-1.2:version>\n  \
         <xccdf-1.2:Profile id=\"{MEASURED}\" extends=\"{}\">\n    \
         <xccdf-1.2:title>{}, and every other rule</xccdf-1.2:title>\n",
        profile.id,
        profile.name()
    );
    for id in content.groups.iter().chain(&content.ids) {
        let _ = writeln!(
            out,
            "    <xccdf-1.2:select idref=\"{id}\" selected=\"true\"/>"
        );
    }
    out.push_str("  </xccdf-1.2:Profile>\n</xccdf-1.2:Tailoring>\n");
    out
}

fn datastream(root: &Path, list: &List, named: Option<&str>) -> Result<String, String> {
    let name = target(list, named)?;
    let (image, flavour, _) = of_target(list, &name).ok_or_else(|| unknown(&name))?;
    if image
        .conforms_of(flavour.as_deref().unwrap_or(NO_FLAVOUR))
        .is_empty()
    {
        return Ok(String::new());
    }
    let base = image.base.as_ref();
    let mut issues = crate::diag::Issues::default();
    let bases = crate::base::catalog(root, &list.sources, &mut issues);
    Ok(measured_by(
        CONTENT,
        &bases,
        base.map_or("", |base| base.image.as_str()),
        base.map_or("", |base| base.family.as_str()),
    )?
    .display()
    .to_string())
}

fn measured_by(
    dir: &str,
    bases: &[crate::base::Base],
    image: &str,
    family: &str,
) -> Result<PathBuf, String> {
    match crate::base::find(bases, image) {
        Some(base) if !base.scap_content.is_empty() => Ok(Path::new(dir).join(&base.scap_content)),
        Some(base) => match installed(dir, family) {
            Err(refusal) => Err(refusal),
            Ok(_) => Err(format!(
                "`{}` names no `scap-content`, so nothing says which benchmark it is measured \
                 against\n\nhelp: add `scap-content \"ssg-<os>-ds.xml\"` to its base file in the \
                 base-images library",
                base.image
            )),
        },
        None => installed(dir, family),
    }
}

fn installed(dir: &str, family: &str) -> Result<PathBuf, String> {
    let file =
        match family {
            "fedora" => "ssg-fedora-ds.xml",
            "debian" | "ubuntu" => {
                return Err(format!(
                    "SSG publishes no content a `{family}` image can be measured against\n\nhelp: \
                 every SSG benchmark names the one release it applies to, and there is no Debian \
                 14 benchmark and no usable Ubuntu 26.04 one; drop `conforms` from this image, or \
                 name content of your own with `--datastream <file>`"
                ))
            }
            "rhel" => return Err(
                "SSG writes one datastream per EL release, so the `rhel` family has no default\n\n\
                 help: give the base's row a `scap-content` line, or name content of your own \
                 with `--datastream <file>`"
                    .to_string(),
            ),
            _ => return Err(format!("no SSG content is known for the `{family}` family")),
        };
    Ok(Path::new(dir).join(file))
}

pub fn content_path(family: &str, given: Option<&Path>) -> Result<PathBuf, String> {
    content_at(CONTENT, family, given)
}

pub fn content_of(path: &Path) -> Result<Content, String> {
    let content =
        Content::read(&read(path)?).map_err(|error| format!("{}: {error}", path.display()))?;
    if content.ids.is_empty() && content.profiles.is_empty() {
        return Err(format!(
            "{} carries no XCCDF rule and no profile, so it is not SCAP content\n\nhelp: use an \
             `ssg-<os>-ds.xml` file; `buildc scap content` prints the path a scan uses",
            path.display()
        ));
    }
    Ok(content)
}

fn content_at(dir: &str, family: &str, given: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = given {
        return Ok(path.to_path_buf());
    }
    let path = installed(dir, family)?;
    match path.is_file() {
        true => Ok(path),
        false => Err(format!(
            "{} is not there, so there is no content to choose a profile out of; install \
             `scap-security-guide`, or name one with `--datastream <file>`",
            path.display()
        )),
    }
}

pub fn run(root: &Path, arf: &Path, opts: &Options) -> Result<Verdict, String> {
    let named = opts.target.as_deref();
    let Some((list, resolved)) = open(root, named) else {
        return Ok(Verdict::Wrong);
    };
    let name = target(&list, named)?;
    let (image, flavour, entries) = of_target(&list, &name).ok_or_else(|| unknown(&name))?;
    let at = list.images.iter().position(|have| have.id == image.id);
    let shipped = &resolved[at.unwrap_or_default()].shipped;
    let gate = flavour.as_deref().unwrap_or(NO_FLAVOUR);

    let datastream = match &opts.datastream {
        Some(path) => path.clone(),
        None => match datastream(root, &list, Some(&name))? {
            empty if empty.is_empty() => {
                return Err(format!(
                    "`{name}` declares no `conforms`, so there is nothing to measure it against; \
                     name a datastream to scan it anyway"
                ))
            }
            path => PathBuf::from(path),
        },
    };
    let content = content_of(&datastream)?;
    let measured = results(&read(arf)?).map_err(|error| format!("{}: {error}", arf.display()))?;
    let base = match &opts.base {
        None => None,
        Some(path) => {
            let doc =
                Json::parse(&read(path)?).map_err(|err| format!("{}: {err}", path.display()))?;
            Some(baseline(&doc).ok_or_else(|| {
                format!(
                    "{}: no `passed` list, so it is not a scan's pass set",
                    path.display()
                )
            })?)
        }
    };

    let mut found: Vec<Finding> = Vec::new();
    let mut out = String::new();
    let passed: BTreeSet<String> = measured
        .iter()
        .filter(|(_, result)| *result == "pass")
        .map(|(rule, _)| rule.clone())
        .collect();
    let failed = measured.values().filter(|r| *r == "fail").count();
    let _ = writeln!(
        out,
        "The scan measured {} rules: {} pass, {failed} fail.\n",
        measured.len(),
        passed.len()
    );

    claims(
        &mut out,
        &mut found,
        Measured {
            image,
            entries: &entries,
            shipped,
            gate,
            content: &content,
            measured: &measured,
            base: base.as_ref(),
        },
    );
    profiles(&mut out, &mut found, image, &content, &measured);
    if let Some(path) = &opts.baseline {
        ratchet(&mut out, &mut found, path, &name, &passed, &measured)?;
    }
    print!("{out}");

    if found.is_empty() {
        return Ok(Verdict::Clean);
    }
    if !list.audit_enforce {
        for finding in &found {
            eprintln!("buildc: {}", finding.message);
        }
        eprintln!(
            "buildc: {} finding{}, and the repository does not enforce",
            found.len(),
            if found.len() == 1 { "" } else { "s" }
        );
        return Ok(Verdict::Clean);
    }
    let mut issues = Issues::default();
    for finding in found {
        let issue = match &finding.at {
            Some((src, span)) => {
                Issue::new(finding.message, src).at(*span, "this is what was measured")
            }
            None => Issue::new(finding.message, &list.repo_src),
        };
        issues.push(match finding.help {
            Some(help) => issue.help(help),
            None => issue,
        });
    }
    issues.report(&crate::context(&list, root));
    Ok(Verdict::Wrong)
}

struct Finding {
    message: String,
    at: Option<(Source, Span)>,
    help: Option<String>,
}

struct Measured<'a> {
    image: &'a Image,
    entries: &'a [&'a Entry],
    shipped: &'a overlay::Index,
    gate: &'a str,
    content: &'a Content,
    measured: &'a BTreeMap<String, String>,
    base: Option<&'a BTreeSet<String>>,
}

fn claims(out: &mut String, found: &mut Vec<Finding>, m: Measured) {
    let mut rows = String::new();
    let mut redundant: Vec<String> = Vec::new();
    for entry in m.entries {
        let Some(module) = entry.module.as_ref() else {
            continue;
        };
        for coverage in &module.satisfies {
            for rule in &coverage.rules {
                let at = Some((module.src.clone(), coverage.span));
                let Some(id) = m.content.rules.get(rule) else {
                    let _ = writeln!(
                        rows,
                        "| {} | {} | {rule} |{} **maps to nothing** |",
                        entry.path,
                        coverage.benchmark,
                        base_cell(m.base, None)
                    );
                    found.push(Finding {
                        message: format!(
                            "{} claims {} {rule}, which names no rule in the datastream",
                            entry.path, coverage.benchmark
                        ),
                        at,
                        help: Some(
                            "a number is only usable where the scanned family's content \
                             carries it, so the declaration is what has to change"
                                .into(),
                        ),
                    });
                    continue;
                };
                let over = m.base.is_some_and(|passed| passed.contains(id));
                let result = m.measured.get(id).map_or("notselected", String::as_str);
                let _ = writeln!(
                    rows,
                    "| {} | {} | {rule} |{} {result} |",
                    entry.path,
                    coverage.benchmark,
                    base_cell(m.base, Some(id))
                );
                if over && result == "pass" {
                    redundant.push(format!("{} {} {rule}", entry.path, coverage.benchmark));
                }
                if result != "fail" {
                    continue;
                }
                let lost = overridden(m.image, m.shipped, m.gate, &entry.path);
                let help = lost
                    .first()
                    .map(|(path, by)| {
                        format!(
                            "{by} owns the final {path}, so this claim is not contradicted; the \
                             composition defeats it"
                        )
                    })
                    .or_else(|| over.then(|| regressed(m.image)));
                found.push(Finding {
                    message: format!(
                        "{} claims {} {rule} and the image fails it",
                        entry.path, coverage.benchmark
                    ),
                    at,
                    help,
                });
            }
        }
    }
    if rows.is_empty() {
        let _ = writeln!(out, "No module here declares `satisfies`.\n");
        return;
    }
    let head = match m.base.is_some() {
        true => {
            "| Module | Benchmark | Rule | Base alone | Result |\n| --- | --- | --- | --- | \
                 --- |\n"
        }
        false => "| Module | Benchmark | Rule | Result |\n| --- | --- | --- | --- |\n",
    };
    let _ = write!(out, "## Declared coverage, measured\n\n{head}{rows}\n");
    if m.base.is_none() {
        return;
    }
    let _ = writeln!(
        out,
        "The base column is the bare base's own pass set. It records what passed and nothing \
         else, so `-` is not a failure: the rule may have failed on the base, or may never have \
         been measured there.\n"
    );
    if !redundant.is_empty() {
        let _ = writeln!(
            out,
            "Not load-bearing: {}. The base alone already passes {}, so the image passes with \
             the module and without it. Whether the module implements it too is not something \
             this scan can say, and it applies its settings either way.\n",
            redundant.join(", "),
            match redundant.len() {
                1 => "it",
                _ => "them",
            }
        );
    }
}

fn base_cell(base: Option<&BTreeSet<String>>, id: Option<&str>) -> String {
    match base {
        None => String::new(),
        Some(passed) => match id.is_some_and(|id| passed.contains(id)) {
            true => " pass |".into(),
            false => " - |".into(),
        },
    }
}

fn regressed(image: &Image) -> String {
    format!(
        "{} alone passed this rule when the base scan was taken, so the image lost it rather \
         than never having had it: a base that moved, or a layer over it",
        image.base.as_ref().map_or_else(
            || "the base".to_string(),
            |base| format!("`{}`", base.image)
        )
    )
}

fn profiles(
    out: &mut String,
    found: &mut Vec<Finding>,
    image: &Image,
    content: &Content,
    measured: &BTreeMap<String, String>,
) {
    let declared = content
        .profiles
        .iter()
        .find(|profile| profile.is(&image.conforms));
    if !image.conforms.is_empty() && declared.is_none() {
        let names: Vec<&str> = content.profiles.iter().map(|p| p.name()).collect();
        found.push(Finding {
            message: format!(
                "this image conforms to `{}`, which is none of the profiles the datastream \
                 carries: {}",
                image.conforms,
                names.join(", ")
            ),
            at: Some((image.src.clone(), image.span)),
            help: Some(
                "the benchmark set is open, so only a scan catches a name nothing measures".into(),
            ),
        });
    }
    if content.profiles.is_empty() {
        return;
    }
    let _ = write!(
        out,
        "## Measured against every profile\n\n\
         | Profile | Selected | Pass | Fail | Other |\n| --- | --- | --- | --- | --- |\n"
    );
    for profile in &content.profiles {
        let selected = content.selected(&profile.id);
        let count = |want: &str| {
            selected
                .iter()
                .filter(|rule| measured.get(*rule).map(String::as_str) == Some(want))
                .count()
        };
        let (pass, fail) = (count("pass"), count("fail"));
        let _ = writeln!(
            out,
            "| `{}`{} | {} | {pass} | {fail} | {} |",
            profile.name(),
            match profile.is(&image.conforms) {
                true => " (declared)",
                false => "",
            },
            selected.len(),
            selected.len() - pass - fail
        );
    }
    let _ = writeln!(
        out,
        "\nEvery row is scored at the values the scan ran with, which are the named profile's \
         under `buildc scap tailoring` and every default under `(all)`.\n"
    );
}

fn ratchet(
    out: &mut String,
    found: &mut Vec<Finding>,
    path: &Path,
    target: &str,
    passed: &BTreeSet<String>,
    measured: &BTreeMap<String, String>,
) -> Result<(), String> {
    let before = match std::fs::read_to_string(path) {
        Ok(text) => {
            baseline(&Json::parse(&text).map_err(|err| format!("{}: {err}", path.display()))?)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(format!("{}: {err}", path.display())),
    };
    let now = Json::object([
        ("target", Json::string(target)),
        ("passed", Json::strings(passed.iter().cloned())),
    ]);
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    std::fs::write(path, now.render()).map_err(|err| format!("{}: {err}", path.display()))?;

    let Some(before) = before else {
        let _ = writeln!(
            out,
            "## Since the last scan\n\nNothing to compare against; this run is the baseline.\n"
        );
        return Ok(());
    };
    let lost: Vec<&String> = before.difference(passed).collect();
    if lost.is_empty() {
        let _ = writeln!(
            out,
            "## Since the last scan\n\nNothing that passed before stopped passing.\n"
        );
        return Ok(());
    }
    let _ = write!(
        out,
        "## Since the last scan\n\n| Rule | Now |\n| --- | --- |\n"
    );
    for rule in &lost {
        let _ = writeln!(
            out,
            "| {rule} | {} |",
            measured.get(*rule).map_or("notselected", String::as_str)
        );
    }
    out.push('\n');
    found.push(Finding {
        message: format!(
            "{} passed the last scan and does not now: {}",
            match lost.len() {
                1 => "one rule".to_string(),
                many => format!("{many} rules"),
            },
            lost.iter()
                .map(|rule| rule.as_str())
                .collect::<Vec<&str>>()
                .join(", ")
        ),
        at: None,
        help: Some(
            "nothing here declares them, so this is the image being un-hardened by something \
             it did not choose, most often a base that moved"
                .into(),
        ),
    });
    Ok(())
}

fn baseline(doc: &Json) -> Option<BTreeSet<String>> {
    let Json::Object(fields) = doc else {
        return None;
    };
    let (_, passed) = fields.iter().find(|(name, _)| name == "passed")?;
    let Json::Array(items) = passed else {
        return None;
    };
    Some(
        items
            .iter()
            .filter_map(|item| match item {
                Json::String(rule) => Some(rule.clone()),
                Json::Null | Json::Bool(_) | Json::Number(_) | Json::Array(_) | Json::Object(_) => {
                    None
                }
            })
            .collect(),
    )
}

fn overridden(
    image: &Image,
    shipped: &overlay::Index,
    gate: &str,
    module: &str,
) -> Vec<(String, String)> {
    crate::emit::plan::overrides(image, shipped, gate)
        .into_iter()
        .filter(|(_, loser, _)| loser == module)
        .map(|(path, _, by)| (path, by))
        .collect()
}

fn target(list: &List, named: Option<&str>) -> Result<String, String> {
    match named {
        Some(name) => list.find_target(name).map(|target| target.to_string()),
        None => list
            .default_target()
            .map(|target| target.to_string())
            .ok_or_else(|| "no default image to take a target from".to_string()),
    }
}

fn unknown(name: &str) -> String {
    format!("`{name}` is not a build target")
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))
}

#[derive(Default)]
pub struct Content {
    pub rules: BTreeMap<String, String>,
    pub numbers: BTreeMap<String, BTreeSet<String>>,
    pub titles: BTreeMap<String, String>,
    pub descriptions: BTreeMap<String, String>,
    ids: BTreeSet<String>,
    groups: BTreeSet<String>,
    pub profiles: Vec<Profile>,
}

#[derive(Default)]
pub struct Profile {
    pub id: String,
    pub title: String,
    extends: String,
    selects: Vec<(String, bool)>,
}

impl Profile {
    pub fn name(&self) -> &str {
        match self.id.rsplit_once("_profile_") {
            Some((_, name)) => name,
            None => &self.id,
        }
    }

    pub fn is(&self, declared: &str) -> bool {
        !declared.is_empty() && (self.id == declared || self.name() == declared)
    }
}

pub fn profile_name(id: &str) -> &str {
    id.rsplit_once("_profile_").map_or(id, |(_, tail)| tail)
}

pub fn rule_name(id: &str) -> &str {
    id.rsplit_once("_rule_").map_or(id, |(_, tail)| tail)
}

pub fn ordinal(number: &str) -> Vec<u64> {
    number
        .split('.')
        .map(|part| part.parse().unwrap_or(u64::MAX))
        .collect()
}

impl Content {
    pub fn read(text: &str) -> Result<Self, String> {
        let mut content = Self::default();
        let mut rule = String::new();
        let mut wanted: Option<String> = None;
        let mut profile: Option<Profile> = None;
        xml::scan(text, |event| {
            if matches!(&event, xml::Event::Text(text) if text.trim().is_empty()) {
                return;
            }
            let want = wanted.take();
            match event {
                xml::Event::Open { name, attrs } => match xml::local(&name) {
                    "Rule" => {
                        rule = xml::attr(&attrs, "id").unwrap_or_default().to_string();
                        content.ids.insert(rule.clone());
                    }
                    "Group" => {
                        content
                            .groups
                            .insert(xml::attr(&attrs, "id").unwrap_or_default().to_string());
                    }
                    name @ ("reference" | "ident" | "version" | "title" | "description") => {
                        wanted = Some(name.to_string());
                    }
                    "Profile" => {
                        profile = Some(Profile {
                            id: xml::attr(&attrs, "id").unwrap_or_default().to_string(),
                            extends: xml::attr(&attrs, "extends").unwrap_or_default().to_string(),
                            ..Profile::default()
                        });
                    }
                    "select" => {
                        if let (Some(profile), Some(idref)) =
                            (profile.as_mut(), xml::attr(&attrs, "idref"))
                        {
                            let on = xml::attr(&attrs, "selected") != Some("false");
                            profile.selects.push((idref.to_string(), on));
                        }
                    }
                    _ => {}
                },
                xml::Event::Close { name } => match xml::local(&name) {
                    "Rule" => rule.clear(),
                    "Profile" => content.profiles.extend(profile.take()),
                    _ => {}
                },
                xml::Event::Text(text) => match want.as_deref() {
                    Some("title") if !rule.is_empty() => {
                        content
                            .titles
                            .entry(rule.clone())
                            .or_insert_with(|| text.trim().to_string());
                    }
                    Some("title") => {
                        if let Some(profile) = profile.as_mut() {
                            if profile.title.is_empty() {
                                profile.title = text.trim().to_string();
                            }
                        }
                    }
                    Some("description") if !rule.is_empty() => {
                        content
                            .descriptions
                            .entry(rule.clone())
                            .or_insert_with(|| text.trim().to_string());
                    }
                    Some(_) if !rule.is_empty() => {
                        content
                            .rules
                            .entry(text.trim().to_string())
                            .or_insert_with(|| rule.clone());
                    }
                    _ => {}
                },
            }
        })?;
        let mut numbers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (number, rule) in &content.rules {
            numbers
                .entry(rule.clone())
                .or_default()
                .insert(number.clone());
        }
        content.numbers = numbers;
        Ok(content)
    }

    pub fn numbering(&self, rules: &BTreeSet<String>) -> BTreeSet<String> {
        rules
            .iter()
            .filter_map(|rule| self.numbers.get(rule))
            .flatten()
            .cloned()
            .collect()
    }

    pub fn selected(&self, id: &str) -> BTreeSet<String> {
        self.select(id, 0)
    }

    fn select(&self, id: &str, depth: usize) -> BTreeSet<String> {
        let Some(profile) = self.profiles.iter().find(|have| have.id == id) else {
            return BTreeSet::new();
        };
        // A profile's `extends` can name a cycle, so the walk stops at a fixed depth.
        let mut out = match profile.extends.is_empty() || depth > 8 {
            true => BTreeSet::new(),
            false => self.select(&profile.extends, depth + 1),
        };
        for (rule, on) in &profile.selects {
            if !self.ids.contains(rule) {
                continue;
            }
            match on {
                true => out.insert(rule.clone()),
                false => out.remove(rule),
            };
        }
        out
    }
}

fn results(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut rule = String::new();
    let mut wanted = false;
    xml::scan(text, |event| match event {
        xml::Event::Open { name, attrs } if xml::local(&name) == "rule-result" => {
            rule = xml::attr(&attrs, "idref").unwrap_or_default().to_string();
        }
        xml::Event::Close { name } if xml::local(&name) == "rule-result" => rule.clear(),
        xml::Event::Open { name, .. } if xml::local(&name) == "result" => wanted = !rule.is_empty(),
        xml::Event::Text(text) if wanted => {
            let text = text.trim();
            if !text.is_empty() {
                out.entry(rule.clone()).or_insert_with(|| text.to_string());
            }
            wanted = false;
        }
        xml::Event::Open { .. } | xml::Event::Close { .. } | xml::Event::Text(_) => wanted = false,
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::disk::Disk;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
    }

    #[test]
    fn content_values_belong_only_to_the_rule_that_contains_them() {
        let content = Content::read(
            "<root xmlns:x='urn:test'><x:Rule id='one'><x:title> One </x:title><description>About one</description>\
             <reference>1</reference></x:Rule><title>Outside</title><description>Outside</description>\
             <reference>2</reference><Rule id='two'><title>Two</title>\
             <reference>3</reference></Rule></root>",
        )
        .unwrap();

        assert_eq!(content.rules.get("1").map(String::as_str), Some("one"));
        assert!(!content.rules.contains_key("2"));
        assert_eq!(content.rules.get("3").map(String::as_str), Some("two"));
        assert_eq!(content.titles.get("one").map(String::as_str), Some("One"));
        assert_eq!(content.titles.get("two").map(String::as_str), Some("Two"));
        assert_eq!(
            content.descriptions.get("one").map(String::as_str),
            Some("About one")
        );
        assert!(!content.titles.contains_key(""));
        assert!(!content.descriptions.contains_key(""));
    }

    #[test]
    fn results_take_only_result_text_inside_its_rule_result() {
        let measured = results(
            "<root><result>before</result><rule-result idref='one'><wrapper/>\
             <result> pass </result></rule-result><rule-result idref='lookalike'>\
             <wrapper>wrong</wrapper></rule-result><rule-result idref='closed'></rule-result>\
             <result>after</result><rule-result idref='two'><result>fail</result>\
             </rule-result></root>",
        )
        .unwrap();

        assert_eq!(
            measured,
            BTreeMap::from([
                ("one".to_string(), "pass".to_string()),
                ("two".to_string(), "fail".to_string()),
            ])
        );
    }

    #[test]
    fn malformed_xml_never_supplies_scap_content_or_results() {
        for text in [
            "<root><rule-result idref='r'><result>pass</result></rule-result></root><!DOCTYPE later>",
            "<![CDATA[ ]]><root/>",
            "<1root/>",
            "<root>&#1;</root>",
            "<?xml version='1.1'?><root/>",
            "<root/><?xml version='1.0'?>",
        ] {
            assert!(Content::read(text).is_err(), "content accepted {text:?}");
            assert!(results(text).is_err(), "results accepted {text:?}");
        }
    }

    #[test]
    fn the_index_answers_which_modules_claim_a_profile_s_rules() {
        let content = Content::read(&read(&fixture("tests/scap/datastream.xml")).unwrap()).unwrap();
        let profile = content
            .profiles
            .iter()
            .find(|profile| profile.is("standard"))
            .expect("the fixture carries `standard`");
        assert_eq!(profile.title, "Standard System Security Profile for Fedora");
        assert_eq!(
            content
                .titles
                .get("xccdf_org.ssgproject.content_rule_package_aide_installed")
                .map(String::as_str),
            Some("Install AIDE")
        );

        let numbers = content.numbering(&content.selected(&profile.id));
        assert!(numbers.contains("5.2.20") && numbers.contains("RHEL-09-232010"));
        assert!(!numbers.contains("5.5.2"), "`standard` does not select it");

        let root = fixture("tests/repos/enforced");
        let index = Index::scan(&root, &[], &Disk::scan(&root), false);
        let claiming: Vec<String> = index
            .claiming(&numbers)
            .iter()
            .map(|held| held.qualified())
            .collect();
        assert_eq!(claiming, ["one/auditing", "one/hello"]);
        assert!(index.claiming(&BTreeSet::new()).is_empty());
    }

    #[test]
    fn a_named_datastream_is_taken_and_a_missing_one_names_both_remedies() {
        let given = fixture("tests/scap/datastream.xml");
        assert_eq!(
            content_at("/nowhere", "fedora", Some(&given)).unwrap(),
            given
        );
        assert_eq!(
            content_at(
                given
                    .parent()
                    .expect("a fixture directory")
                    .to_str()
                    .unwrap(),
                "fedora",
                None
            )
            .unwrap_err(),
            format!(
                "{} is not there, so there is no content to choose a profile out of; install \
                 `scap-security-guide`, or name one with `--datastream <file>`",
                given.with_file_name("ssg-fedora-ds.xml").display()
            )
        );
        assert!(content_at("/nowhere", "arch", None)
            .unwrap_err()
            .contains("no SSG content is known"));
    }

    #[test]
    fn a_deb_family_is_refused_rather_than_given_content_that_measures_nothing() {
        for family in ["debian", "ubuntu"] {
            let err = content_at("/nowhere", family, None).unwrap_err();
            assert!(
                err.contains(&format!("no content a `{family}` image")),
                "{err}"
            );
            assert!(err.contains("drop `conforms`"), "{err}");
        }
        let given = fixture("tests/scap/datastream.xml");
        assert_eq!(
            content_at("/nowhere", "debian", Some(&given)).unwrap(),
            given
        );
    }

    #[test]
    fn a_base_row_names_content_the_family_cannot() {
        let row = |image: &str, family: &str, content: &str| crate::base::Base {
            name: image.to_string(),
            image: image.to_string(),
            family: family.to_string(),
            provides: Vec::new(),
            requires: Vec::new(),
            about: String::new(),
            signed: false,
            scap_content: content.to_string(),
            bootloaders: Vec::new(),
            span: crate::diag::Span::default(),
        };
        let bases = [
            row(
                "quay.io/centos-bootc/centos-bootc:stream10",
                "rhel",
                "ssg-cs10-ds.xml",
            ),
            row(
                "quay.io/fedora/fedora-bootc:44",
                "fedora",
                "ssg-fedora-ds.xml",
            ),
            row("example.invalid/silent:1", "fedora", ""),
        ];
        let of = |image, family| measured_by("/nowhere", &bases, image, family);

        let el = of("quay.io/centos-bootc/centos-bootc:stream10", "rhel").unwrap();
        assert!(el.ends_with("ssg-cs10-ds.xml"), "{}", el.display());

        let fedora = of("quay.io/fedora/fedora-bootc:44", "fedora").unwrap();
        assert!(
            fedora.ends_with("ssg-fedora-ds.xml"),
            "{}",
            fedora.display()
        );

        let pinned = of(
            "quay.io/centos-bootc/centos-bootc:stream10@sha256:0000",
            "rhel",
        )
        .unwrap();
        assert!(pinned.ends_with("ssg-cs10-ds.xml"), "{}", pinned.display());

        let fell = of("example.invalid/unknown:1", "fedora").unwrap();
        assert!(fell.ends_with("ssg-fedora-ds.xml"), "{}", fell.display());
        let err = of("example.invalid/alma:10", "rhel").unwrap_err();
        assert!(err.contains("one datastream per EL release"), "{err}");

        let err = of("example.invalid/silent:1", "fedora").unwrap_err();
        assert!(err.contains("names no `scap-content`"), "{err}");
        assert!(err.contains("add `scap-content"), "{err}");

        let err = of("example.invalid/silent:1", "debian").unwrap_err();
        assert!(err.contains("no content a `debian` image"), "{err}");
    }

    #[test]
    fn a_file_without_rules_or_profiles_is_not_scap_content() {
        let path =
            std::env::temp_dir().join(format!("buildc-not-scap-content.{}.xml", std::process::id()));
        std::fs::write(&path, "<root/>").unwrap();
        assert_eq!(
            content_of(&path)
                .err()
                .expect("the image file is not SCAP content"),
            format!(
                "{} carries no XCCDF rule and no profile, so it is not SCAP content\n\nhelp: use \
                 an `ssg-<os>-ds.xml` file; `buildc scap content` prints the path a scan uses",
                path.display()
            )
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_exclusions_carry_every_claim_including_the_bases_and_every_refusal() {
        let root = fixture("tests/repos/refused");
        let loaded = crate::load(&root);
        let image = loaded
            .list
            .images
            .first()
            .expect("the fixture declares one image");
        let (claimed, refused) = exclusions(image);

        assert!(claimed.contains(&"1.1.1.1".to_string()), "{claimed:?}");
        assert!(
            claimed.contains(&"RHEL-09-232010".to_string()),
            "a base claims the way a module does: {claimed:?}"
        );
        assert!(
            refused.contains(&"package_aide_installed".to_string()),
            "{refused:?}"
        );
    }

    #[test]
    fn check_catches_a_rule_both_claimed_and_refused_and_an_image_nothing_remediates() {
        let root = fixture("tests/repos/refused");
        let loaded = crate::load(&root);
        let said = conformance(
            &loaded.list,
            &loaded.index,
            Some(&fixture("tests/scap/datastream.xml")),
        )
        .expect("the fixture datastream reads");

        assert!(
            said.iter().any(|m| m.contains("claims `1.1.1.1`")
                && m.contains("refuses `package_aide_installed`")
                && m.contains("one of the two is wrong")),
            "{said:#?}"
        );
        assert!(
            said.iter()
                .any(|m| m.contains("nothing it installs provides `scap-remediation`")),
            "{said:#?}"
        );
        assert!(
            said.iter().any(|m| m.contains("one/typo")
                && m.contains("claims CCI-000198 under `stig`")
                && m.contains("stops the build in the finalize layer")),
            "{said:#?}"
        );

        assert!(
            said.iter().any(|m| m.contains("one/typo")
                && m.contains("refuses `not_a_rule_this_content_has`")
                && m.contains("keeps nothing out of remediation")),
            "{said:#?}"
        );

        assert!(
            said.iter()
                .any(|m| m.contains("`no_module_here_refuses_this`")
                    && m.contains("the allowance lifts nothing")),
            "{said:#?}"
        );
        assert!(
            !said
                .iter()
                .any(|m| m.contains("`sshd_disable_root_login`") && m.contains("lifts nothing")),
            "an allowance that answers a real refusal is silent: {said:#?}"
        );

        let image = &loaded.list.images[0];
        let (_, refused) = exclusions(image);
        assert!(
            !refused.contains(&"sshd_disable_root_login".to_string()),
            "{refused:?}"
        );
        assert!(
            refused.contains(&"package_aide_installed".to_string()),
            "another module's refusal is untouched: {refused:?}"
        );

        let bare = fixture("tests/scap");
        let nothing = Index::scan(&bare, &[], &Disk::scan(&bare), false);
        assert!(
            !conformance(&loaded.list, &nothing, None)
                .expect("no datastream is the first tier")
                .iter()
                .any(|m| m.contains("scap-remediation")),
            "an unavailable capability is not offered"
        );
    }

    #[test]
    fn the_notice_names_the_rules_no_listed_module_claims() {
        let root = fixture("tests/repos/enforced");
        let loaded = crate::load(&root);
        let said = conformance(
            &loaded.list,
            &loaded.index,
            Some(&fixture("tests/scap/datastream.xml")),
        )
        .expect("the fixture datastream reads");
        assert_eq!(
            said,
            [
                "`one/hello` claims 9.9.9.9 under `cis-fedora`, which the content `enforced` is \
              measured against carries no rule by. The remediation hook resolves a claim to the \
              rule it protects, so this stops the build in the finalize layer",
                "`enforced` conforms to `standard`, and nothing it installs or builds on claims 2 \
              of the 4 rules it selects; `one/auditing` would claim 1 of them"
            ]
        );
        let bare = fixture("tests/scap");
        let nothing = Index::scan(&bare, &[], &Disk::scan(&bare), false);
        assert_eq!(
            conformance(
                &loaded.list,
                &nothing,
                Some(&fixture("tests/scap/datastream.xml"))
            )
            .expect("the fixture datastream reads"),
            [
                "`one/hello` claims 9.9.9.9 under `cis-fedora`, which the content `enforced` is \
              measured against carries no rule by. The remediation hook resolves a claim to the \
              rule it protects, so this stops the build in the finalize layer",
                "`enforced` conforms to `standard`, and nothing it installs or builds on claims 2 \
              of the 4 rules it selects; nothing else in the repository claims them"
            ]
        );
        assert!(conformance(&loaded.list, &loaded.index, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_module_the_base_suppressed_is_not_offered_back() {
        let root = fixture("tests/scap/suppressed-claimant");
        let loaded = crate::load(&root);
        let image = loaded.list.images.first().expect("one image");
        assert!(image.entries.is_empty(), "the base covers the only module");
        assert_eq!(
            image.suppressed.iter().map(Entry::dir).collect::<Vec<_>>(),
            ["one/aide"]
        );
        assert_eq!(
            conformance(
                &loaded.list,
                &loaded.index,
                Some(&fixture("tests/scap/datastream.xml"))
            )
            .expect("the fixture datastream reads"),
            [
                "`suppressed` conforms to `standard`, and nothing it installs or builds on claims \
              4 of the 4 rules it selects; nothing else in the repository claims them"
            ]
        );
    }

    #[test]
    fn a_base_claims_the_rules_it_says_it_satisfies() {
        let root = fixture("tests/scap/claiming-base");
        let loaded = crate::load(&root);
        assert!(
            loaded.issues.plain().is_empty(),
            "{}",
            loaded.issues.plain()
        );
        let image = loaded.list.images.first().expect("one image");
        assert!(image.entries.is_empty(), "the base covers the only module");
        assert_eq!(
            image
                .base
                .as_ref()
                .expect("a base")
                .satisfies
                .iter()
                .map(|coverage| (coverage.benchmark.as_str(), coverage.rules.len()))
                .collect::<Vec<_>>(),
            [("cis-fedora", 2)]
        );
        assert_eq!(
            conformance(
                &loaded.list,
                &loaded.index,
                Some(&fixture("tests/scap/datastream.xml"))
            )
            .expect("the fixture datastream reads"),
            [
                "`claiming` conforms to `standard`, and nothing it installs or builds on claims 2 of \
              the 4 rules it selects; nothing else in the repository claims them"
            ]
        );
        assert!(conformance(&loaded.list, &loaded.index, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_search_that_reached_the_collections_says_so() {
        let root = fixture("tests/scap/sourced-claimant");
        let loaded = crate::load(&root);
        assert!(
            loaded.issues.plain().is_empty(),
            "{}",
            loaded.issues.plain()
        );
        assert!(loaded.index.sourced() && loaded.index.unread().is_empty());
        assert!(
            loaded.index.at(".remote/held/one/plain").is_some(),
            "the collection was walked"
        );
        assert_eq!(
            conformance(
                &loaded.list,
                &loaded.index,
                Some(&fixture("tests/scap/datastream.xml"))
            )
            .expect("the fixture datastream reads"),
            [
                "`sourced` conforms to `standard`, and nothing it installs or builds on claims 4 \
              of the 4 rules it selects; nothing else in the repository or its collections claims them"
            ]
        );
    }

    #[test]
    fn a_module_the_image_lists_is_not_offered_as_one_that_would_help() {
        let root = fixture("tests/scap/listed-claimant");
        let loaded = crate::load(&root);
        assert_eq!(
            loaded
                .index
                .at("one/broken")
                .expect("the index reads the manifest leniently")
                .declares
                .satisfies,
            ["4.1.3.1"],
            "the index has the claim the resolved module dropped"
        );
        assert_eq!(
            conformance(
                &loaded.list,
                &loaded.index,
                Some(&fixture("tests/scap/datastream.xml"))
            )
            .expect("the fixture datastream reads"),
            [
                "`listed` conforms to `standard`, and nothing it installs or builds on claims 3 \
              of the 4 rules it selects; nothing else in the repository claims them"
            ]
        );
    }
}
