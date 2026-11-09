//! `cargo xtask gen [--check]`: the lint-configuration generator (Plan/02 §11.3, WP-CORE-03).
//!
//! Concatenates `xtask/lint/clippy-base.toml` + `clippy-det.toml` (tiers S, G, R) + `clippy-game.toml`
//! (tier G) into `crates/<crate>/clippy.toml` (clippy reads only the nearest file) and writes the base
//! alone to the root `clippy.toml`. A `disallowed-*` entry marked `crate = "x"` is emitted only when the
//! crate depends on `x` directly, so clippy never sees an unresolvable path. Generators registered by
//! other work packages (`gen-proto`, `gen-data-types`, ...) are added to [`run`] when they land.

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::meta::Meta;
use crate::proc::Ctx;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Determinism tier of a crate (Plan/02 §11.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Shared bit-exact (native + wasm32).
    S,
    /// Game thread.
    G,
    /// Reproducible builder.
    R,
    /// Normal.
    N,
}

/// One `disallowed-*` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Rust path.
    pub path: String,
    /// Why.
    pub reason: String,
    /// Only emit when the crate depends on this crate directly.
    pub needs_crate: Option<String>,
}

/// Parsed `xtask/lint/*.toml`.
#[derive(Clone, Debug, Default)]
pub struct Fragments {
    /// Base key/values (`allow-*-in-tests`), rendered verbatim.
    pub base: Vec<(String, toml::Value)>,
    /// Tier S/G/R type bans.
    pub det_types: Vec<Entry>,
    /// Tier S/G/R method bans (float methods expanded).
    pub det_methods: Vec<Entry>,
    /// Tier G additions.
    pub game_types: Vec<Entry>,
    /// Tier G additions.
    pub game_methods: Vec<Entry>,
}

/// The generated tier table: crate name → tier.
pub fn load_tiers(ctx: &Ctx) -> Result<BTreeMap<String, Tier>> {
    let text = std::fs::read_to_string(ctx.path("xtask/lint/deps-allow.toml"))
        .context("read deps-allow.toml")?;
    tiers_from_text(&text)
}

/// Parses the `[tiers]` table.
pub fn tiers_from_text(text: &str) -> Result<BTreeMap<String, Tier>> {
    let doc: toml::Table = text.parse().context("deps-allow.toml")?;
    let tiers = doc
        .get("tiers")
        .and_then(toml::Value::as_table)
        .context("[tiers] missing in deps-allow.toml")?;
    let mut out = BTreeMap::new();
    for (key, tier) in [("S", Tier::S), ("G", Tier::G), ("R", Tier::R)] {
        for v in tiers
            .get(key)
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = v.as_str() {
                out.insert(name.to_owned(), tier);
            }
        }
    }
    Ok(out)
}

fn entries(doc: &toml::Table, key: &str) -> Vec<Entry> {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let t = v.as_table()?;
            Some(Entry {
                path: t.get("path")?.as_str()?.to_owned(),
                reason: t
                    .get("reason")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                needs_crate: t
                    .get("crate")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// Reads the three fragment files.
pub fn load_fragments(ctx: &Ctx) -> Result<Fragments> {
    let read = |name: &str| -> Result<toml::Table> {
        std::fs::read_to_string(ctx.path(&format!("xtask/lint/{name}")))
            .with_context(|| format!("read xtask/lint/{name}"))?
            .parse::<toml::Table>()
            .with_context(|| format!("parse xtask/lint/{name}"))
    };
    fragments_from_tables(
        &read("clippy-base.toml")?,
        &read("clippy-det.toml")?,
        &read("clippy-game.toml")?,
    )
}

/// Builds [`Fragments`] from parsed tables (also used by the self test).
pub fn fragments_from_tables(
    base: &toml::Table,
    det: &toml::Table,
    game: &toml::Table,
) -> Result<Fragments> {
    let mut f = Fragments {
        base: base.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        det_types: entries(det, "disallowed-types"),
        det_methods: entries(det, "disallowed-methods"),
        game_types: entries(game, "disallowed-types"),
        game_methods: entries(game, "disallowed-methods"),
    };
    if let Some(float) = det.get("float").and_then(toml::Value::as_table) {
        let reason = float
            .get("reason")
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .to_owned();
        for prim in ["f32", "f64"] {
            for m in float
                .get("methods")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
            {
                f.det_methods.push(Entry {
                    path: format!("{prim}::{m}"),
                    reason: reason.clone(),
                    needs_crate: None,
                });
            }
        }
    }
    Ok(f)
}

fn render_entries(out: &mut String, key: &str, items: &[&Entry]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(out, "{key} = [");
    for e in items {
        // A crate-gated path may name an item behind a feature the workspace does not enable (for
        // example `chrono::Utc::now` without `clock`): tell clippy not to warn about it.
        let allow = if e.needs_crate.is_some() {
            ", allow-invalid = true"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  {{ path = {:?}, reason = {:?}{allow} }},",
            e.path, e.reason
        );
    }
    out.push_str("]\n");
}

const HEADER: &str =
    "# GENERATED by `cargo xtask gen` from xtask/lint/*.toml (Plan/02 §11.3). Do not edit.\n";

/// Renders the `clippy.toml` of a crate with `tier` and the given direct dependencies; the root file
/// is `render(.., Tier::N, ..)`.
pub fn render(frag: &Fragments, tier: Tier, direct_deps: &BTreeSet<String>, msrv: &str) -> String {
    let mut out = String::from(HEADER);
    let _ = writeln!(out, "msrv = {msrv:?}");
    for (k, v) in &frag.base {
        let _ = writeln!(out, "{k} = {v}");
    }
    let usable = |e: &&Entry| {
        e.needs_crate
            .as_ref()
            .is_none_or(|c| direct_deps.contains(c))
    };
    let mut types: Vec<&Entry> = Vec::new();
    let mut methods: Vec<&Entry> = Vec::new();
    if matches!(tier, Tier::S | Tier::G | Tier::R) {
        types.extend(frag.det_types.iter().filter(usable));
        methods.extend(frag.det_methods.iter().filter(usable));
    }
    if tier == Tier::G {
        types.extend(frag.game_types.iter().filter(usable));
        methods.extend(frag.game_methods.iter().filter(usable));
    }
    render_entries(&mut out, "disallowed-types", &types);
    render_entries(&mut out, "disallowed-methods", &methods);
    out
}

fn msrv(ctx: &Ctx) -> Result<String> {
    let text = std::fs::read_to_string(ctx.path("rust-toolchain.toml"))
        .context("read rust-toolchain.toml")?;
    let doc: toml::Table = text.parse().context("rust-toolchain.toml")?;
    doc.get("toolchain")
        .and_then(|t| t.get("channel"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .context("channel missing in rust-toolchain.toml")
}

/// All generated files: repo-relative path → content.
pub fn generate(ctx: &Ctx) -> Result<BTreeMap<String, String>> {
    let frag = load_fragments(ctx)?;
    let tiers = load_tiers(ctx)?;
    let msrv = msrv(ctx)?;
    let meta = Meta::load(ctx, None, true)?;
    let mut files = BTreeMap::new();
    files.insert(
        "clippy.toml".to_owned(),
        render(&frag, Tier::N, &BTreeSet::new(), &msrv),
    );
    for pkg in meta.members() {
        let Some(tier) = tiers.get(&pkg.name).copied() else {
            continue;
        };
        let dir = std::path::Path::new(&pkg.manifest_path)
            .parent()
            .and_then(|p| p.strip_prefix(&ctx.root).ok())
            .map(|p| p.to_string_lossy().into_owned());
        let Some(dir) = dir else { continue };
        let deps: BTreeSet<String> = pkg.dependencies.iter().map(|d| d.name.clone()).collect();
        files.insert(
            format!("{dir}/clippy.toml"),
            render(&frag, tier, &deps, &msrv),
        );
    }
    Ok(files)
}

/// Writes (or with `check` verifies) the generated files.
pub fn run(ctx: &Ctx, check: bool) -> Result<()> {
    let files = generate(ctx)?;
    let mut stale = Vec::new();
    for (rel, content) in &files {
        let path = ctx.path(rel);
        let current = std::fs::read_to_string(&path).ok();
        if current.as_deref() == Some(content.as_str()) {
            continue;
        }
        if check {
            stale.push(rel.clone());
        } else {
            std::fs::write(&path, content).with_context(|| format!("write {rel}"))?;
            println!("wrote {rel}");
        }
    }
    if check {
        if stale.is_empty() {
            println!("gen --check: {} generated file(s) up to date", files.len());
        } else {
            bail!(
                "generated files are stale (run `cargo xtask gen`): {}",
                stale.join(", ")
            );
        }
    } else if stale.is_empty() {
        println!("gen: {} generated file(s) written / unchanged", files.len());
    }
    Ok(())
}
