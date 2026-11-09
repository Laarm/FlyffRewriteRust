//! `flyff check` (Plan/10 §9.2, 14 §9, 24 §11.4, 30 §17.1): validates the configuration, the Resource folder, the
//! game data, the derived-asset cache, `scripts/npc/`, the database and every binary file of the VFS, and prints
//! the counts of the data report.
//!
//! Sections, in order: configuration (retired/unknown keys), Resource, VFS (overrides), game data (the reference
//! counts of 14 §9.1 with diagnostics digest), cache (per kind: jobs, present, failed, bytes; budget; motion and
//! collision summaries when the cache is complete), `scripts/npc/` against `MANIFEST.txt` (02 §8.5), the
//! database (read-only: schema version, init-only values against the configuration), the exposure warning, the
//! binary corpus (every `.o3d .chr .ani .lnd .dyo .sfx .dds .tga .bmp .cur` the VFS serves, `overrides/`
//! included, parsed on all cores by `flyff_assets::check::binary_corpus`; per-kind tallies; the three known
//! unreadable files of the original Resource are listed, not findings; the text world files are only counted).
//!
//! Exit codes: 0 = fine, 1 = findings (Resource missing, modified NPC scripts, init-only mismatch, failed cache
//! jobs, unreadable binary files, `--expect-reference` mismatches of the data counts and of the corpus tallies),
//! 2 = the game data could not be loaded, 78 = invalid configuration. The exposure warning is a warning, not a
//! finding. Modded data (different tallies) is exit 0 without `--expect-reference`. Without game data the corpus
//! is skipped.
//!
//! `--json` prints one JSON document with the same content instead of the text.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use super::{Cli, env_pairs, home_of, overrides_of, prepare::init_cli_logging, web_source};
use crate::assets::{ServerClock, prep_config, start_prep};
use crate::build_info;
use crate::config::{self, Config, ResolvedPaths};
use crate::data_load::{self, DataLoadError, DataMode, LoadedData};
use crate::exit::{self, ExitStatusCode};
use crate::exposure;
use crate::runtime::resource_present;
use flyff_assets::AssetTelemetry;
use flyff_assets::check::{BinaryCorpusReport, binary_corpus};
use flyff_formats::FormatKind;
use flyff_formats::corpus::{KNOWN_UNREADABLE, RefMismatch};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// Flags of `flyff check`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Compare with the reference values of the original Resource (and the cache budget).
    pub expect_reference: bool,
    /// More detail (every override, every diagnostic code).
    pub verbose: bool,
    /// One JSON document instead of text.
    pub json: bool,
}

pub use flyff_data::check::report::{CountRow, count_rows, define_rows};

/// Collects the report: text lines are printed as they come (unless `--json`), the JSON document is built
/// alongside.
struct Out {
    json: bool,
    doc: Map<String, Value>,
    findings: Vec<String>,
}

impl Out {
    fn say(&self, s: impl AsRef<str>) {
        if !self.json {
            println!("{}", s.as_ref());
        }
    }

    fn finding(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.say(format!("  FINDING: {s}"));
        self.findings.push(s);
    }

    /// A finding that is counted and listed in `--json` but not printed (the tail of a long list).
    fn finding_quiet(&mut self, s: impl Into<String>) {
        self.findings.push(s.into());
    }

    fn put(&mut self, key: &str, v: Value) {
        self.doc.insert(key.to_owned(), v);
    }
}

/// Runs `flyff check`.
pub fn run(cli: &Cli, opts: &CheckOptions) -> ExitStatusCode {
    init_cli_logging();
    let home = match home_of(cli) {
        Ok(h) => h,
        Err(c) => return c,
    };
    let loaded = match config::load(&home, &overrides_of(cli), &env_pairs(), &[]) {
        Ok(l) => l,
        Err(errs) => {
            for e in errs {
                eprintln!("error: {e}");
            }
            return exit::CONFIG;
        }
    };
    let cfg = &loaded.config;
    let paths = cfg.resolve_paths(&home);
    let mut out = Out {
        json: opts.json,
        doc: Map::new(),
        findings: Vec::new(),
    };
    out.say(build_info::version_line(&web_source(cli).build_id()));

    // ---- configuration ----
    let file = home.join("flyff.toml");
    out.say(format!("home:      {}", home.display()));
    out.say(format!(
        "config:    {} ({} warning(s))",
        if file.exists() {
            "flyff.toml loaded"
        } else {
            "no flyff.toml, built-in defaults"
        },
        loaded.warnings.len()
    ));
    for w in &loaded.warnings {
        out.say(format!("  warning: {w}"));
    }
    out.put(
        "config",
        json!({"home": home.display().to_string(), "file": file.exists(), "warnings": loaded.warnings.iter().map(ToString::to_string).collect::<Vec<_>>()}),
    );

    // ---- Resource, VFS, data ----
    let mut fatal = None;
    let mut data = None;
    if resource_present(&paths.resource) {
        out.say(format!(
            "resource:  {} (Masquerade.prj found)",
            paths.resource.display()
        ));
        out.put(
            "resource",
            json!({"path": paths.resource.display().to_string(), "present": true}),
        );
        match data_load::load(cfg, &paths, DataMode::Load, None) {
            Ok(d) => {
                report_data(&mut out, &d, opts);
                data = Some(d);
            }
            Err(DataLoadError::ResourceMissing(p)) => {
                out.finding(format!("Resource folder missing: {}", p.display()));
            }
            Err(DataLoadError::Data(m)) => {
                out.say(format!("data:      FATAL: {m}"));
                out.put("data", json!({"fatal": m}));
                fatal = Some(m);
            }
        }
    } else {
        out.say(format!(
            "resource:  NOT FOUND at {} (copy your Flyff V15 `Resource` folder there)",
            paths.resource.display()
        ));
        out.put(
            "resource",
            json!({"path": paths.resource.display().to_string(), "present": false}),
        );
        out.finding("the Resource folder was not found");
    }

    // ---- cache ----
    if let Some(d) = &data {
        report_cache(&mut out, cfg, &paths, d, opts);
    } else {
        out.say("cache:     skipped (no game data)");
    }

    // ---- scripts/npc ----
    report_scripts(&mut out, &paths);

    // ---- database ----
    report_database(&mut out, cfg, &paths);

    // ---- exposure ----
    if let Some(w) = exposure::exposure_warning(cfg, &exposure::interface_addrs()) {
        out.say(format!("warning:   {w}"));
        out.put("exposure_warning", Value::String(w));
    }

    // ---- binary corpus ----
    report_corpus(&mut out, data.as_ref(), opts);

    // ---- verdict ----
    let code = if fatal.is_some() {
        exit::DATA
    } else if out.findings.is_empty() {
        exit::OK
    } else {
        exit::FAILURE
    };
    out.say(match code {
        exit::OK => "check:     OK".to_owned(),
        exit::DATA => "check:     the game data could not be loaded".to_owned(),
        _ => format!("check:     {} finding(s)", out.findings.len()),
    });
    if opts.json {
        out.put("findings", json!(out.findings));
        out.put("exit_code", json!(code));
        let doc = Value::Object(std::mem::take(&mut out.doc));
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
    }
    code
}

fn report_data(out: &mut Out, d: &LoadedData, opts: &CheckOptions) {
    let r = &d.report;
    out.say(format!(
        "vfs:       {} files, {} from overrides/ ({} ms)",
        r.vfs_files, r.override_files, r.vfs_ms
    ));
    if r.override_files > 0 {
        let names: Vec<String> = d
            .vfs
            .override_entries()
            .map(|e| e.path.to_string())
            .collect();
        let shown = if opts.verbose {
            names.len()
        } else {
            names.len().min(10)
        };
        for n in names.iter().take(shown) {
            out.say(format!("  override: {n}"));
        }
        if names.len() > shown {
            out.say(format!(
                "  ... {} more (--verbose lists all)",
                names.len() - shown
            ));
        }
    }
    out.say(format!(
        "data:      loaded in {} ms; {} errors, {} warnings; {} loader steps ({} still stubs); fingerprint {}",
        r.load_ms, r.errors, r.warnings, r.steps_done, r.steps_stub, r.fingerprint
    ));
    if opts.verbose {
        for (code, n) in &r.diag_counts {
            out.say(format!("  diagnostic {code}: {n}"));
        }
    }
    for line in r
        .first_errors
        .iter()
        .take(if opts.verbose { 20 } else { 5 })
    {
        out.say(format!("  error: {line}"));
    }
    let data_report =
        flyff_data::check::xref::build_report(&d.game, d.client.as_deref(), &d.vfs, &[]);
    let rows = &data_report.rows;
    out.say("counts (Plan/14 9.1):");
    let mut mismatches = 0usize;
    for c in rows {
        let flag = if c.mismatch() { "  <-- differs" } else { "" };
        let reference = match (c.reference, opts.expect_reference || opts.verbose) {
            (Some(r), true) => format!("   [reference {r}]"),
            _ => String::new(),
        };
        out.say(format!(
            "  {:<4} {:<58} {}{}{}",
            c.id, c.label, c.value, reference, flag
        ));
        if c.mismatch() {
            mismatches += 1;
        }
    }
    out.say("spot checks (Plan/14 9.2):");
    for sc in &data_report.spot_checks {
        out.say(format!(
            "  {:<4} {}{}",
            if sc.ok { "ok" } else { "FAIL" },
            sc.name,
            if sc.ok {
                String::new()
            } else {
                format!(": {}", sc.detail)
            }
        ));
    }
    out.say("cross-references (Plan/14 9.3):");
    for x in &data_report.xref {
        out.say(format!(
            "  {:<14} {:<7} checked {:>6}, unresolved {:>5}  {}{}",
            x.id,
            format!("{:?}", x.severity),
            x.checked,
            x.missing,
            x.label,
            if opts.verbose && !x.samples.is_empty() {
                format!("  e.g. {}", x.samples.join("; "))
            } else {
                String::new()
            }
        ));
    }
    if opts.expect_reference {
        for line in data_report.mismatches() {
            out.finding(line);
        }
    }
    out.put(
        "data",
        json!({
            "vfs_files": r.vfs_files,
            "override_files": r.override_files,
            "vfs_ms": r.vfs_ms,
            "load_ms": r.load_ms,
            "errors": r.errors,
            "warnings": r.warnings,
            "diagnostics": r.diag_counts,
            "fingerprint": r.fingerprint,
            "counts": rows.iter().map(|c| json!({
                "id": c.id, "label": c.label, "value": c.value, "reference": c.reference, "differs": c.mismatch(),
            })).collect::<Vec<_>>(),
            "reference_mismatches": mismatches,
            "spot_checks": data_report.spot_checks,
            "xref": data_report.xref,
        }),
    );
}

fn report_cache(
    out: &mut Out,
    cfg: &Config,
    paths: &ResolvedPaths,
    d: &LoadedData,
    opts: &CheckOptions,
) {
    if !paths.cache.is_dir() {
        out.say("cache:     not created yet (the first start builds it; `flyff prepare` builds it ahead)");
        out.put("cache", json!({"present": false}));
        return;
    }
    let mut acfg = prep_config(cfg, paths);
    acfg.threads = 1;
    acfg.prewarm = false;
    let prep = match start_prep(
        Arc::clone(&d.vfs),
        Arc::clone(&d.game),
        acfg,
        Arc::new(ServerClock::default()),
        AssetTelemetry::new(),
        |_| Vec::new(),
    ) {
        Ok(p) => p,
        Err(e) => {
            out.finding(format!("the asset cache cannot be opened: {e}"));
            return;
        }
    };
    let rep = prep.cache_report();
    out.say(format!(
        "cache:     {} fingerprints, {:.1} MiB of {:.0} MiB budget{}",
        rep.fingerprint_mode,
        rep.total_bytes as f64 / 1_048_576.0,
        rep.budget_bytes as f64 / 1_048_576.0,
        if rep.over_budget {
            " (OVER BUDGET)"
        } else {
            ""
        }
    ));
    let mut complete = true;
    for k in &rep.kinds {
        let missing = k.expected_jobs.saturating_sub(k.present);
        if missing > 0 && k.kind != "gz" {
            complete = false;
        }
        out.say(format!(
            "  {:<13} jobs {:>6}  present {:>6}  failed {:>3}  {:>8.1} MiB",
            k.kind,
            k.expected_jobs,
            k.present,
            k.failed,
            k.bytes as f64 / 1_048_576.0
        ));
        if k.failed > 0 {
            out.finding(format!(
                "cache: {} job(s) of kind {} failed in the last run (see cache/prepare-report.json)",
                k.failed, k.kind
            ));
        }
    }
    if let Some(last) = &rep.last_report {
        for f in last.failures.iter().take(if opts.verbose { 50 } else { 5 }) {
            out.say(format!("  failed: {} {}: {}", f.kind, f.key, f.error));
        }
    }
    if rep.over_budget && opts.expect_reference {
        // a note, not a finding (owner 2026-10-29): the cache is derived and can be rebuilt
        out.say("note: the cache is over assets.disk_budget_mb");
    }
    let mut summary = Map::new();
    if complete {
        // Everything the start-up kinds produce is cached: reading it back changes nothing.
        match prep.load_server_derived() {
            Ok(derived) => {
                if let Some(m) = &derived.motions {
                    let missing: usize = m
                        .models
                        .values()
                        .flat_map(|mm| mm.slots.values())
                        .filter(|s| matches!(s, flyff_sim::anim::motion_table::Slot::Missing))
                        .count();
                    out.say(format!(
                        "  motions: {} clips, {} models, {} slots reference a missing .ani",
                        m.clips.len(),
                        m.models.len(),
                        missing
                    ));
                    summary.insert(
                        "motions".to_owned(),
                        json!({"clips": m.clips.len(), "models": m.models.len(), "missing_slots": missing}),
                    );
                }
                if let Some(c) = &derived.collision {
                    let t = c.total_stats();
                    out.say(format!(
                        "  collision: {} world folders, {} lands loaded of {}, {} instances, {} meshes",
                        c.stats().len(),
                        t.loaded,
                        t.lands,
                        t.instances,
                        t.meshes
                    ));
                    summary.insert(
                        "collision".to_owned(),
                        json!({"folders": c.stats().len(), "lands": t.lands, "loaded": t.loaded, "instances": t.instances}),
                    );
                }
            }
            Err(e) => out.finding(format!("the cached start-up assets do not load: {e}")),
        }
    } else {
        out.say("  (the cache is incomplete: run `flyff prepare` for the motion and collision summaries)");
    }
    out.put(
        "cache",
        json!({
            "present": true,
            "fingerprint_mode": rep.fingerprint_mode,
            "total_bytes": rep.total_bytes,
            "budget_bytes": rep.budget_bytes,
            "over_budget": rep.over_budget,
            "kinds": rep.kinds,
            "summary": summary,
        }),
    );
    prep.shutdown(std::time::Duration::from_secs(5));
}

/// Unreadable binary files printed in the text; the rest are findings without a line (`--verbose` lists all).
const SHOWN_UNREADABLE: usize = 50;

/// One output line of the corpus section, before it reaches [`Out`] (so the text is unit-testable).
#[derive(Clone, Debug, PartialEq, Eq)]
enum CorpusLine {
    /// Printed as is.
    Say(String),
    /// A printed finding.
    Finding(String),
    /// A finding without a line of its own.
    Quiet(String),
}

/// Runs the binary corpus over the VFS of the loaded data (all cores) and reports it; no game data means
/// nothing to walk (Resource missing or fatal load: that is reported by the sections above).
fn report_corpus(out: &mut Out, data: Option<&LoadedData>, opts: &CheckOptions) {
    let Some(d) = data else {
        out.say("corpus:    skipped (no game data)");
        out.put("corpus", json!({"skipped": true}));
        return;
    };
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let started = Instant::now();
    let report = binary_corpus(&d.vfs, threads);
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    for line in corpus_lines(&report, ms, opts) {
        match line {
            CorpusLine::Say(s) => out.say(s),
            CorpusLine::Finding(s) => out.finding(s),
            CorpusLine::Quiet(s) => out.finding_quiet(s),
        }
    }
    out.put("corpus", corpus_json(&report, opts.expect_reference, ms));
}

/// `22878` -> `22,878`.
fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The reason of a known unreadable file (empty for a path the table does not list).
fn known_reason(path: &str) -> &'static str {
    KNOWN_UNREADABLE
        .iter()
        .find(|k| k.path == path)
        .map_or("", |k| k.reason)
}

/// The text section: summary, one row per format, the counted-only kinds, the known files and the findings.
fn corpus_lines(r: &BinaryCorpusReport, ms: u64, opts: &CheckOptions) -> Vec<CorpusLine> {
    use CorpusLine::{Finding, Quiet, Say};
    let mut lines = Vec::new();
    let known = r.known().count() as u64;
    let bad = r.unreadable().count() as u64;
    let readable = r.files.saturating_sub(known + bad);
    lines.push(Say(format!(
        "corpus:    {} binary files ({:.2} GiB) in {:.1} s on {} threads: {} readable, {} known unreadable, {} UNREADABLE",
        group_digits(r.files),
        r.bytes as f64 / (1u64 << 30) as f64,
        ms as f64 / 1000.0,
        r.threads,
        group_digits(readable),
        known,
        bad
    )));
    let mismatches = r.reference_mismatches();
    let differs = |kind: FormatKind| mismatches.iter().any(|m| m.kind == kind);
    for (kind, t) in r.tallies.iter().filter(|(k, _)| k.is_corpus_parsed()) {
        lines.push(Say(format!(
            "  {:<7} files {:>5}  ok {:>5}  known {}  failed {}{}",
            kind.label(),
            t.files,
            t.ok,
            t.known,
            t.failed,
            if differs(*kind) {
                "  <-- differs from the reference"
            } else {
                ""
            }
        )));
        if opts.verbose {
            for (fact, n) in &t.facts {
                lines.push(Say(format!("    {fact}: {n}")));
            }
        }
    }
    let counted: Vec<String> = r
        .tallies
        .iter()
        .filter(|(k, _)| !k.is_corpus_parsed())
        .map(|(kind, t)| {
            let reference = mismatches
                .iter()
                .find(|m| m.kind == *kind && m.what == "files")
                .map(|m| format!(" (reference {})", m.reference));
            format!(
                "{} {}{}",
                kind.label(),
                t.files,
                reference.unwrap_or_default()
            )
        })
        .collect();
    if !counted.is_empty() {
        lines.push(Say(format!(
            "  counted only (the data loader parses them): {}",
            counted.join(", ")
        )));
    }
    for f in r.known() {
        lines.push(Say(format!(
            "  known:    {} ({}): {}",
            f.path,
            f.kind.label(),
            known_reason(&f.path)
        )));
    }
    if !mismatches.is_empty() && !opts.expect_reference {
        lines.push(Say(format!(
            "  {} difference(s) from the reference Resource (--expect-reference makes them findings{})",
            mismatches.len(),
            if opts.verbose { "" } else { ", --verbose lists them" }
        )));
        if opts.verbose {
            for m in &mismatches {
                lines.push(Say(format!("    {m}")));
            }
        }
    }
    let shown = if opts.verbose {
        usize::MAX
    } else {
        SHOWN_UNREADABLE
    };
    for (i, f) in r.unreadable().enumerate() {
        let text = format!("corpus: {} ({}): {}", f.path, f.kind.label(), f.error);
        lines.push(if i < shown {
            Finding(text)
        } else {
            Quiet(text)
        });
    }
    if bad as usize > shown {
        lines.push(Say(format!(
            "  ... {} more (--verbose lists all)",
            bad as usize - shown
        )));
    }
    if opts.expect_reference {
        for m in &mismatches {
            lines.push(Finding(format!("corpus reference: {m}")));
        }
    }
    lines
}

fn mismatch_json(m: &RefMismatch) -> Value {
    json!({"kind": m.kind.label(), "what": m.what, "found": m.found, "reference": m.reference})
}

/// The `corpus` object of `--json`: `threads, files, bytes, ms, expect_reference`, `kinds` (per format: counts,
/// `facts`, `differs`), `failures` (the unreadable files, i.e. the findings), `known` (the known unreadable
/// files with their reason) and `reference_mismatches`. `ms` is the elapsed time the caller measured.
fn corpus_json(r: &BinaryCorpusReport, expect_reference: bool, ms: u64) -> Value {
    let mismatches = r.reference_mismatches();
    json!({
        "threads": r.threads,
        "files": r.files,
        "bytes": r.bytes,
        "ms": ms,
        "expect_reference": expect_reference,
        "kinds": r.tallies.iter().map(|(kind, t)| json!({
            "kind": kind.label(),
            "files": t.files,
            "ok": t.ok,
            "known": t.known,
            "failed": t.failed,
            "facts": t.facts,
            "differs": mismatches.iter().any(|m| m.kind == *kind),
        })).collect::<Vec<_>>(),
        "failures": r.unreadable().map(|f| json!({
            "path": f.path, "kind": f.kind.label(), "error": f.error,
        })).collect::<Vec<_>>(),
        "known": r.known().map(|f| json!({
            "path": f.path, "kind": f.kind.label(), "reason": known_reason(&f.path),
        })).collect::<Vec<_>>(),
        "reference_mismatches": mismatches.iter().map(mismatch_json).collect::<Vec<_>>(),
    })
}

/// The result of comparing `scripts/npc/` with its `MANIFEST.txt`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptDiff {
    /// Files whose SHA-256 differs from the manifest.
    pub modified: Vec<String>,
    /// Files listed in the manifest that do not exist.
    pub missing: Vec<String>,
    /// `.lua` files that are not in the manifest.
    pub extra: Vec<String>,
}

impl ScriptDiff {
    /// `true` when `scripts/npc/` equals the manifest.
    pub fn is_clean(&self) -> bool {
        self.modified.is_empty() && self.missing.is_empty() && self.extra.is_empty()
    }
}

/// Compares the `.lua` files of `dir` with `<dir>/MANIFEST.txt` (`<sha256>  <file>` per line, `#` comments).
/// `None` when there is no manifest.
pub fn compare_scripts(dir: &Path) -> Option<ScriptDiff> {
    let text = std::fs::read_to_string(dir.join("MANIFEST.txt")).ok()?;
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if let Some((sum, name)) = line.split_once("  ") {
            listed.insert(name.trim().to_owned(), sum.trim().to_ascii_lowercase());
        }
    }
    let mut diff = ScriptDiff::default();
    for (name, sum) in &listed {
        match std::fs::read(dir.join(name)) {
            Ok(bytes) => {
                if hex(&Sha256::digest(&bytes)) != *sum {
                    diff.modified.push(name.clone());
                }
            }
            Err(_) => diff.missing.push(name.clone()),
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut extra: Vec<String> = rd
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".lua") && !listed.contains_key(n))
            .collect();
        extra.sort();
        diff.extra = extra;
    }
    Some(diff)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn report_scripts(out: &mut Out, paths: &ResolvedPaths) {
    let dir = &paths.npc_scripts;
    if !dir.is_dir() {
        out.say(format!(
            "scripts:   {} missing (restored from the embedded copy at the first start)",
            dir.display()
        ));
        out.put("scripts", json!({"present": false}));
        return;
    }
    match compare_scripts(dir) {
        None => {
            out.say(format!(
                "scripts:   {} (no MANIFEST.txt: cannot compare)",
                dir.display()
            ));
            out.put("scripts", json!({"present": true, "manifest": false}));
        }
        Some(diff) if diff.is_clean() => {
            out.say(format!("scripts:   {} matches MANIFEST.txt", dir.display()));
            out.put(
                "scripts",
                json!({"present": true, "manifest": true, "clean": true}),
            );
        }
        Some(diff) => {
            out.say(format!(
                "scripts:   {} differs from MANIFEST.txt",
                dir.display()
            ));
            for f in &diff.modified {
                out.say(format!("  modified: {f}"));
            }
            for f in &diff.missing {
                out.say(format!("  missing:  {f}"));
            }
            for f in &diff.extra {
                out.say(format!("  extra:    {f}"));
            }
            out.finding(format!(
                "scripts/npc differs from MANIFEST.txt ({} modified, {} missing, {} extra)",
                diff.modified.len(),
                diff.missing.len(),
                diff.extra.len()
            ));
            out.put(
                "scripts",
                json!({"present": true, "manifest": true, "clean": false,
                       "modified": diff.modified, "missing": diff.missing, "extra": diff.extra}),
            );
        }
    }
}

fn report_database(out: &mut Out, cfg: &Config, paths: &ResolvedPaths) {
    let db = &paths.database;
    if !db.exists() {
        out.say(format!("database:  {} (not created yet)", db.display()));
        out.put("database", json!({"present": false}));
        return;
    }
    let conn = match flyff_db::conn::open_reader(db, &cfg.database) {
        Ok(c) => c,
        Err(e) => {
            out.finding(format!("the database cannot be opened read-only: {e}"));
            out.put("database", json!({"present": true, "error": e.to_string()}));
            return;
        }
    };
    let status = match flyff_db::migrate::status(&conn) {
        Ok(s) => s,
        Err(e) => {
            out.finding(format!("database schema: {e}"));
            out.put("database", json!({"present": true, "error": e.to_string()}));
            return;
        }
    };
    let count = |sql: &str| -> i64 {
        conn.query_row(sql, [], |r| r.get::<_, i64>(0))
            .unwrap_or(-1)
    };
    let accounts = count("SELECT COUNT(*) FROM accounts");
    let characters = count("SELECT COUNT(*) FROM characters WHERE state = 0");
    out.say(format!(
        "database:  {} (schema {} of {}, {} account(s), {} character(s))",
        db.display(),
        status.current,
        status.latest,
        accounts,
        characters
    ));
    if !status.pending.is_empty() {
        out.say(format!(
            "  {} migration(s) pending: the next start applies them after a backup (`flyff db migrate --dry-run`)",
            status.pending.len()
        ));
    }
    let get = |key: &str| flyff_db::migrate::meta_get(&conn, key).ok().flatten();
    let stored = (
        get("init.features.inventory_168"),
        get("init.features.bigger_pocket"),
        get("init.gameplay.max_pack_item"),
    );
    let mut init_ok = true;
    if let (Some(inv), Some(pocket), Some(pack)) = stored {
        let stored = flyff_db::InitOnlyValues {
            inventory_168: inv != 0,
            bigger_pocket: pocket != 0,
            max_pack_item: u32::try_from(pack).unwrap_or(0),
        };
        let current = flyff_common::config::init_only_snapshot(&cfg.rules());
        if let Err(m) = flyff_common::config::check_init_only(&stored, &current) {
            init_ok = false;
            out.finding(format!("init-only configuration changed: {m}"));
        }
    }
    out.put(
        "database",
        json!({"present": true, "schema": status.current, "latest": status.latest,
               "pending": status.pending.len(), "accounts": accounts, "characters": characters,
               "init_only_ok": init_ok}),
    );
}
