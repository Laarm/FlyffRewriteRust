//! `cargo xtask lint-determinism`: the source scanner behind the clippy lists (Plan/02 §11.3-11.4,
//! WP-CORE-03). Belt and braces for clippy's primitive-method matching, plus the rules clippy cannot
//! express (SIMD attributes, std I/O in wasm-safe crates, channel libraries).
//!
//! A line ending in `// det-ok: <reason>` is exempt, with one exception: the wall clock of `flyff-game` (rule `G-clock`,
//! Plan/10 §17, WP-OPS-15) may be read in `crates/flyff-game/src/tick.rs` only and is flagged anywhere else in the
//! crate's non-test sources even with `// det-ok`. Scanning of a file stops at its `#[cfg(test)] mod` (unit tests sit
//! at the end of the file by convention); `tests/` and `benches/` are not scanned.

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::cmd::dep_rules;
use crate::cmd::gen_lint::{Tier, load_tiers};
use crate::meta::Meta;
use crate::proc::Ctx;
use anyhow::{Context, Result, bail};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

/// A scanner hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Rule id.
    pub rule: &'static str,
    /// Repo-relative file.
    pub file: String,
    /// 1-based line.
    pub line: usize,
    /// Why this is forbidden.
    pub message: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: [{}] {}",
            self.file, self.line, self.rule, self.message
        )
    }
}

struct Rule {
    id: &'static str,
    re: Regex,
    message: &'static str,
    /// Applies to these tiers.
    tiers: &'static [Tier],
    /// Skipped for files under the io-exempt paths (feature-gated native code).
    honours_io_exempt: bool,
}

const FLOAT_FNS: &str = "sin|cos|tan|asin|acos|atan2?|sin_cos|exp2?|exp_m1|ln|log(?:2|10)?|ln_1p|powf|powi|cbrt|hypot|sinh|cosh|tanh|asinh|acosh|atanh|mul_add";

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |id, pattern: String, message, tiers, io| Rule { id, re: Regex::new(&pattern).unwrap_or_else(|_| Regex::new("a^").expect("static regex")), message, tiers, honours_io_exempt: io };
        use Tier::{G, R, S};
        vec![
            r("DET-hash", r"\b(HashMap|HashSet|FxHashMap|FxHashSet|RandomState)\b".into(), "std hash collections iterate in a random order: use flyff_common::det::{DetMap, DetSet}, BTreeMap or slotmap (Plan/02 §11.3)", &[S, G, R], false),
            r("DET-time", r"\b(?:Instant|SystemTime|Utc|Local)::now\b".into(), "game time comes from the tick clock; wall time is an injected per-tick input (Plan/02 §11.3)", &[S, G, R], false),
            r("DET-float", format!(r"\.(?:{FLOAT_FNS})\s*\(|\b(?:f32|f64)::(?:{FLOAT_FNS})\b"), "platform libm/FMA differences: use the libm crate (D4, Plan/02 §11.3)", &[S, G, R], false),
            r("DET-rand", r"\b(?:thread_rng|ThreadRng|OsRng|StdRng|from_entropy|from_os_rng)\b|\brand::random\b".into(), "use the seeded per-world PCG / LegacyLcg (D2)", &[S, G, R], false),
            r("DET-simd", r"\b(?:core::arch|std::arch|std::simd|target_feature|Vec3A|Affine3A)\b".into(), "no SIMD / target features in bit-exact code (D4)", &[S], false),
            r("PANIC", r"\.unwrap\(\)|\.expect\(|\bpanic!\s*\(|\bunreachable!\s*\(|\btodo!\s*\(|\bunimplemented!\s*\(".into(), "wasm-facing code must not panic: return errors (Plan/02 §11.4, X4)", &[S], true),
            r("X2-io", r"\bstd::(?:fs|net|thread|process|env)\b".into(), "wasm-safe crates must not use std::fs/net/thread/process/env (X2, ADR-004)", &[S], true),
            r("G-sync", r"\bstd::sync::(?:Mutex|RwLock)\b|\bparking_lot::(?:Mutex|RwLock)\b".into(), "game state is owned by the game thread: use channels (Plan/02 §11.3 tier G)", &[G], false),
            r("G-thread", r"\bstd::thread::(?:spawn|sleep)\b".into(), "the game thread never spawns or sleeps (Plan/02 §11.3 tier G)", &[G], false),
            r("G-io", r"\bstd::fs::(?:read|read_to_string|write|File)\b|\bstd::env::vars?\b|\bstd::process::exit\b".into(), "all resource access goes through flyff-vfs, persistence through the actor, config through Config (Plan/02 §11.3 tier G)", &[G], false),
        ]
    })
}

/// The one file of `flyff-game` that may sample the wall clock (Plan/10 §4.3, §17).
pub const CLOCK_HOME: &str = "crates/flyff-game/src/tick.rs";
const GAME_SRC: &str = "crates/flyff-game/src/";
const CLOCK_MESSAGE: &str = "wall time enters flyff-game only through the one sample in tick.rs (TickClock); read Telemetry::now_ns() or take the time as a per-tick input. `// det-ok` does not exempt this rule (Plan/10 §17, WP-OPS-15)";

fn clock_rule() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(?:Instant|SystemTime)::now\b")
            .unwrap_or_else(|_| Regex::new("a^").expect("static regex"))
    })
}

/// A file that only exists for tests: below a `tests` or `testkit` directory, or named `tests`, `testkit` or `*_tests`.
fn test_only_path(rel_path: &str) -> bool {
    let mut parts: Vec<&str> = rel_path.split('/').collect();
    let file = parts.pop().unwrap_or_default();
    let stem = file.strip_suffix(".rs").unwrap_or(file);
    parts.iter().any(|p| matches!(*p, "tests" | "testkit"))
        || matches!(stem, "tests" | "testkit")
        || stem.ends_with("_tests")
}

/// Whether rule `G-clock` applies to a file: tier G, the non-test sources of `flyff-game` except `tick.rs`.
fn clock_rule_applies(tier: Tier, rel_path: &str) -> bool {
    tier == Tier::G
        && rel_path.starts_with(GAME_SRC)
        && rel_path != CLOCK_HOME
        && !test_only_path(rel_path)
}

fn x7_source_rule() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\bstd::sync::mpsc\b|\bcrossbeam_channel\b|\btokio::sync\b")
            .unwrap_or_else(|_| Regex::new("a^").expect("static regex"))
    })
}

/// Cuts a trailing `//` comment (only when it starts outside a string literal) and reports whether it
/// carried a `det-ok` marker.
fn split_comment(line: &str) -> (&str, bool) {
    let mut in_str = false;
    let mut prev = '\0';
    for (i, c) in line.char_indices() {
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        }
        if !in_str && c == '/' && prev == '/' {
            let code = &line[..i - 1];
            return (code, line[i - 1..].contains("det-ok:"));
        }
        prev = c;
    }
    (line, false)
}

/// Scans one source file. `io_exempt` marks feature-gated native-only files (`rt/`, `testkit`, `load`).
/// `server_like` marks crates that may use the tokio/mpsc stack (flyff-server).
pub fn scan_source(
    tier: Tier,
    rel_path: &str,
    text: &str,
    io_exempt: bool,
    server_like: bool,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let clock = clock_rule_applies(tier, rel_path);
    for (idx, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            let next = lines
                .iter()
                .skip(idx + 1)
                .map(|l| l.trim_start())
                .find(|l| !l.is_empty() && !l.starts_with("#["));
            if next.is_some_and(|n| n.starts_with("mod ") || n.starts_with("pub mod ")) {
                break;
            }
        }
        if trimmed.starts_with("//") {
            continue;
        }
        let (code, exempt) = split_comment(raw);
        // the clock rule is checked before the exemption: `// det-ok` does not help here
        if clock && clock_rule().is_match(code) {
            out.push(Finding {
                rule: "G-clock",
                file: rel_path.to_owned(),
                line: idx + 1,
                message: CLOCK_MESSAGE.to_owned(),
            });
        }
        if exempt {
            continue;
        }
        for rule in rules() {
            if !rule.tiers.contains(&tier) || (rule.honours_io_exempt && io_exempt) {
                continue;
            }
            if rule.re.is_match(code) {
                out.push(Finding {
                    rule: rule.id,
                    file: rel_path.to_owned(),
                    line: idx + 1,
                    message: rule.message.to_owned(),
                });
            }
        }
        if !server_like && x7_source_rule().is_match(code) {
            out.push(Finding { rule: "X7-src", file: rel_path.to_owned(), line: idx + 1, message: "inter-thread channels are flume only; tokio::sync lives only in flyff-server (X7, ADR-010)".to_owned() });
        }
    }
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry.with_context(|| format!("walk {}", dir.display()))?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "rs") {
            out.push(entry.into_path());
        }
    }
    Ok(())
}

/// Scans the whole workspace.
pub fn run(ctx: &Ctx) -> Result<()> {
    let tiers = load_tiers(ctx)?;
    let rules = dep_rules::load_rules(ctx)?;
    let io_exempt: Vec<String> = std::fs::read_to_string(ctx.path("xtask/lint/deps-allow.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|d| d.get("io_exempt")?.get("paths")?.as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let meta = Meta::load(ctx, None, true)?;
    let mut findings = Vec::new();
    let mut files = 0usize;
    for pkg in meta.members() {
        if !pkg.manifest_path.contains("/crates/") {
            continue;
        }
        let Some(dir) = Path::new(&pkg.manifest_path).parent() else {
            continue;
        };
        let tier = tiers.get(&pkg.name).copied().unwrap_or(Tier::N);
        let server_like = pkg.name == "flyff-server" || rules.x1_allowed.contains(&pkg.name);
        let mut paths = Vec::new();
        collect_rs(&dir.join("src"), &mut paths)?;
        for path in paths {
            let rel = path
                .strip_prefix(&ctx.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).with_context(|| format!("read {rel}"))?;
            let exempt = io_exempt.iter().any(|p| rel.starts_with(p.as_str()));
            findings.extend(scan_source(tier, &rel, &text, exempt, server_like));
            files += 1;
        }
    }
    if findings.is_empty() {
        println!("lint-determinism: OK ({files} source files scanned)");
        Ok(())
    } else {
        for f in &findings {
            eprintln!("lint-determinism: {f}");
        }
        bail!("lint-determinism found {} violation(s)", findings.len())
    }
}
