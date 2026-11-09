//! The legacy-id coverage gate of the wire protocol (Plan/11 section 11, Plan/02 section 9.6, WP-PROTO-05, WP-TEST-54).
//!
//! Inputs: the committed descriptor set (`flyff_proto::DESCRIPTOR_SET` or `--descriptor`), `proto/legacy/legacy_index.csv`
//! (generated once from annex 11a by `--import-11a`), `proto/legacy/ui_send_sites.csv` (from ref/99 appendix B by
//! `--import-ui-sites`, plus the extra rows of ADR-024), the block table of Plan/11 section 4.2 (embedded in [`checks`])
//! and the `// legacy-send:` markers under `web/src` (`--web-src`). Checks C1-C8 and rule C9 ([`c9`]); outputs
//! `target/reports/proto-coverage.{md,json}`; a non-zero exit code on any error. `cargo xtask coverage proto` runs it.
//!
//! Rule C9 (the client definition of done): every `done` row of `ui_send_sites.csv` has a marker line
//! `// legacy-send: SendName[, SendName]` in a non-test `*.ts(x)` file of `web/src`; a marker naming no row is an error
//! (the message names the nearest row); a `todo` row whose milestone is at or below `current_milestone` (or
//! `--stub-max-milestone`) is a warning before M8 and an error from M8. `--sync-sites` sets `done` on every `todo` row
//! that has a marker (never the reverse).

pub mod c9;
pub mod checks;
pub mod csv;
pub mod import;
pub mod locate;
pub mod model;
pub mod report;

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Where the inputs and outputs of a run are.
#[derive(Clone, Debug)]
pub struct Config {
    /// Repository root.
    pub root: PathBuf,
    /// Descriptor set file; `None` = the one compiled into `flyff-proto`.
    pub descriptor: Option<PathBuf>,
    pub index: PathBuf,
    pub sites: PathBuf,
    /// Root of the `.proto` sources (for `file:line`).
    pub proto_dir: PathBuf,
    /// The client sources scanned for `// legacy-send:` markers (rule C9); a missing directory skips the rule.
    pub web_src: PathBuf,
    pub out_dir: PathBuf,
    /// `M3`; `None` = `PROTO_STUB_MAX_MILESTONE`, then `current_milestone` of `docs/wp-status.toml`.
    pub stub_max: Option<String>,
}

impl Config {
    /// The default layout below a repository root.
    pub fn for_root(root: &Path) -> Config {
        Config {
            root: root.to_path_buf(),
            descriptor: None,
            index: root.join("proto/legacy/legacy_index.csv"),
            sites: root.join("proto/legacy/ui_send_sites.csv"),
            proto_dir: root.join("proto"),
            web_src: root.join("web/src"),
            out_dir: root.join("target/reports"),
            stub_max: None,
        }
    }
}

/// The result of a run.
#[derive(Debug)]
pub struct Outcome {
    pub report: report::Report,
    pub findings: Vec<checks::Finding>,
    pub md_path: PathBuf,
    pub json_path: PathBuf,
}

/// `M3` -> 3.
pub fn parse_milestone(s: &str) -> Result<u32> {
    match import::milestone_rank(s.trim()) {
        u32::MAX => bail!("`{s}` is not a milestone (want M0, M1, ...)"),
        n => Ok(n),
    }
}

/// Resolves `PROTO_STUB_MAX_MILESTONE`: the option, the environment variable, then `current_milestone` of
/// `docs/wp-status.toml` (Plan/11 C6, Plan/02 section 9.6).
pub fn resolve_stub_max(cfg: &Config) -> Result<String> {
    if let Some(m) = &cfg.stub_max {
        return Ok(m.trim().to_owned());
    }
    if let Ok(m) = std::env::var("PROTO_STUB_MAX_MILESTONE")
        && !m.trim().is_empty()
    {
        return Ok(m.trim().to_owned());
    }
    let path = cfg.root.join("docs/wp-status.toml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let doc: toml::Table = text
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    match doc.get("current_milestone").and_then(toml::Value::as_str) {
        Some(m) => Ok(m.to_owned()),
        None => bail!("{} has no current_milestone", path.display()),
    }
}

/// Scans `cfg.web_src` for markers; `None` when the directory does not exist (a temp root without a client).
pub fn scan_markers(cfg: &Config) -> Result<Option<c9::Markers>> {
    if !cfg.web_src.is_dir() {
        return Ok(None);
    }
    c9::scan(&cfg.root, &cfg.web_src).map(Some)
}

/// `--sync-sites`: sets `done` on every `todo` row of `ui_send_sites.csv` that has a marker under `web/src` (never the
/// reverse, the order of the rows is kept) and returns the names that were flipped.
pub fn sync_sites(cfg: &Config) -> Result<Vec<String>> {
    let Some(markers) = scan_markers(cfg)? else {
        bail!(
            "{} does not exist: nothing to scan for markers",
            cfg.web_src.display()
        );
    };
    let text = std::fs::read_to_string(&cfg.sites)
        .with_context(|| format!("read {}", cfg.sites.display()))?;
    let mut sites = csv::read_sites(&text)?;
    let flipped = c9::sync(&mut sites, &markers);
    if !flipped.is_empty() {
        std::fs::write(&cfg.sites, csv::write_sites(&sites))
            .with_context(|| format!("write {}", cfg.sites.display()))?;
    }
    Ok(flipped)
}

/// Runs the gate: loads the inputs, runs C1-C9, writes the reports.
pub fn run(cfg: &Config) -> Result<Outcome> {
    let descriptor: Vec<u8> = match &cfg.descriptor {
        Some(p) => std::fs::read(p).with_context(|| format!("read {}", p.display()))?,
        None => flyff_proto::DESCRIPTOR_SET.to_vec(),
    };
    let model = model::from_descriptor_set(&descriptor)?;
    let index_text = std::fs::read_to_string(&cfg.index)
        .with_context(|| format!("read {}", cfg.index.display()))?;
    let index = csv::read_index(&index_text)?;
    let sites_text = std::fs::read_to_string(&cfg.sites)
        .with_context(|| format!("read {}", cfg.sites.display()))?;
    let sites = csv::read_sites(&sites_text)?;
    let stub_max = resolve_stub_max(cfg)?;
    let opts = checks::Options {
        stub_max: parse_milestone(&stub_max)?,
        markers: scan_markers(cfg)?,
    };
    let mut checked = checks::run_all(&model, &index, &sites, &opts);

    let locator = locate::Locator::load(&cfg.root, &cfg.proto_dir);
    for f in &mut checked.findings {
        // C9 findings carry the `file:line` of the marker or of the row themselves
        if f.check == c9::CHECK {
            continue;
        }
        if let Some(m) = locate::guess_message(&model, &f.subject) {
            f.loc = locator.message(&m.file, &m.name);
        }
    }

    let report = report::build(&model, &index, &sites, &checked, &stub_max);
    std::fs::create_dir_all(&cfg.out_dir)
        .with_context(|| format!("create {}", cfg.out_dir.display()))?;
    let md_path = cfg.out_dir.join("proto-coverage.md");
    let json_path = cfg.out_dir.join("proto-coverage.json");
    std::fs::write(&md_path, report::markdown(&report))
        .with_context(|| format!("write {}", md_path.display()))?;
    let json = serde_json::to_string_pretty(&report).context("serialise the report")?;
    std::fs::write(&json_path, json + "\n")
        .with_context(|| format!("write {}", json_path.display()))?;
    Ok(Outcome {
        report,
        findings: checked.findings,
        md_path,
        json_path,
    })
}
