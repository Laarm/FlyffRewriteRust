//! `cargo xtask dep-rules`: the dependency gate (Plan/02 §11.5, Plan/01 §5.3, WP-CORE-03/92).
//!
//! Inputs: `cargo metadata --no-deps` (declared dependencies, incl. optional ones) and `cargo tree`
//! (per-package feature resolution) for the `flyff-game`, `flyff-wasm` (wasm32) and dist graphs. The
//! rules themselves are pure functions over those inputs so the self test can feed fixtures.

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::meta::Meta;
use crate::proc::{Ctx, Run};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Dependency kind of a declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `[dependencies]`.
    Normal,
    /// `[build-dependencies]`.
    Build,
    /// `[dev-dependencies]`.
    Dev,
}

/// One declared dependency edge `pkg -> dep`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    /// Depending package.
    pub pkg: String,
    /// Dependency name.
    pub dep: String,
    /// Kind.
    pub kind: Kind,
    /// Behind a cargo feature.
    pub optional: bool,
}

/// A rule violation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// Rule id (`X1`, `X5`, `internal`, `game-closure`, `wasm-deny`, ...).
    pub rule: &'static str,
    /// Human-readable description naming the rule.
    pub message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.rule, self.message)
    }
}

/// Parsed `xtask/lint/deps-allow.toml`.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    /// Allowed internal edges by crate.
    pub internal: BTreeMap<String, BTreeSet<String>>,
    /// Feature-gated (optional) edges by crate.
    pub feature_gated: BTreeMap<String, BTreeSet<String>>,
    /// X1 crates.
    pub x1_crates: BTreeSet<String>,
    /// Packages allowed to use X1 crates.
    pub x1_allowed: BTreeSet<String>,
    /// Crates forbidden in the `flyff-game` closure.
    pub game_forbidden: BTreeSet<String>,
    /// Only these may link rusqlite (X5).
    pub rusqlite_only_in: BTreeSet<String>,
    /// Only these may use mlua (X6).
    pub mlua_only_in: BTreeSet<String>,
    /// Forbidden channel crates (X7).
    pub x7_forbidden: BTreeSet<String>,
    /// anyhow/clap style crates restricted to tools (rule 4).
    pub tool_only_crates: BTreeSet<String>,
    /// Packages allowed to use them.
    pub tool_only_allowed: BTreeSet<String>,
    /// Root of the wasm closure.
    pub wasm_root: String,
    /// Allowed direct dependencies of workspace crates inside the wasm closure.
    pub wasm_allow: BTreeSet<String>,
    /// Crates that must never appear in the wasm closure.
    pub wasm_deny: BTreeSet<String>,
    /// Tier S crate names.
    pub tier_s: BTreeSet<String>,
}

fn set_of(doc: &toml::Table, table: &str, key: &str) -> BTreeSet<String> {
    doc.get(table)
        .and_then(|t| t.get(key))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

/// Parses the rule file.
pub fn rules_from_text(text: &str) -> Result<Rules> {
    let doc: toml::Table = text.parse().context("deps-allow.toml")?;
    let map_of = |table: &str| -> BTreeMap<String, BTreeSet<String>> {
        doc.get(table)
            .and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| s.as_str().map(str::to_owned))
                        .collect(),
                )
            })
            .collect()
    };
    Ok(Rules {
        internal: map_of("internal"),
        feature_gated: map_of("feature_gated"),
        x1_crates: set_of(&doc, "x1", "crates"),
        x1_allowed: set_of(&doc, "x1", "allowed_in"),
        game_forbidden: set_of(&doc, "game_forbidden", "crates"),
        rusqlite_only_in: set_of(&doc, "x5", "rusqlite_only_in"),
        mlua_only_in: set_of(&doc, "x6", "mlua_only_in"),
        x7_forbidden: set_of(&doc, "x7", "forbidden"),
        tool_only_crates: set_of(&doc, "tool_only", "crates"),
        tool_only_allowed: set_of(&doc, "tool_only", "allowed_in"),
        wasm_root: doc
            .get("wasm")
            .and_then(|w| w.get("root"))
            .and_then(toml::Value::as_str)
            .unwrap_or("flyff-wasm")
            .to_owned(),
        wasm_allow: set_of(&doc, "wasm", "allow"),
        wasm_deny: set_of(&doc, "wasm", "deny"),
        tier_s: set_of(&doc, "tiers", "S"),
    })
}

/// Loads the repository's rule file.
pub fn load_rules(ctx: &Ctx) -> Result<Rules> {
    rules_from_text(
        &std::fs::read_to_string(ctx.path("xtask/lint/deps-allow.toml"))
            .context("read deps-allow.toml")?,
    )
}

/// Checks declared dependencies (internal matrix, tier-S purity, X1, X5, X6, X7, rule 4).
/// `workspace` = names of all workspace members.
pub fn check_declared(
    rules: &Rules,
    decls: &[Decl],
    workspace: &BTreeSet<String>,
) -> Vec<Violation> {
    let mut v = Vec::new();
    for d in decls.iter().filter(|d| d.kind != Kind::Dev) {
        // Internal matrix (01 §5.3): only for the crates listed in it.
        if workspace.contains(&d.dep) {
            if let Some(allowed) = rules.internal.get(&d.pkg) {
                let gated_ok = d.optional
                    && rules
                        .feature_gated
                        .get(&d.pkg)
                        .is_some_and(|g| g.contains(&d.dep));
                if !allowed.contains(&d.dep) && !gated_ok {
                    v.push(Violation {
                        rule: "internal",
                        message: format!(
                            "`{}` must not depend on `{}` (Plan/01 §5.3 allows: {})",
                            d.pkg,
                            d.dep,
                            allowed.iter().cloned().collect::<Vec<_>>().join(", ")
                        ),
                    });
                }
            }
            // Tier S crates depend only on tier S workspace crates (+ feature-gated exceptions).
            if rules.tier_s.contains(&d.pkg) && !rules.tier_s.contains(&d.dep) {
                let gated_ok = d.optional
                    && rules
                        .feature_gated
                        .get(&d.pkg)
                        .is_some_and(|g| g.contains(&d.dep));
                if !gated_ok {
                    v.push(Violation {
                        rule: "tier-S",
                        message: format!(
                            "tier-S crate `{}` depends on non-tier-S `{}` (Plan/02 §11.5 rule 3)",
                            d.pkg, d.dep
                        ),
                    });
                }
            }
        }
        if rules.x1_crates.contains(&d.dep) && !rules.x1_allowed.contains(&d.pkg) {
            v.push(Violation { rule: "X1", message: format!("`{}` must not depend on `{}`: the async/network stack lives only in flyff-server and flyff-e2e (Plan/01 §5.3 X1)", d.pkg, d.dep) });
        }
        if d.dep == "rusqlite" && !rules.rusqlite_only_in.contains(&d.pkg) {
            v.push(Violation {
                rule: "X5",
                message: format!(
                    "`{}` links rusqlite; SQL lives only in flyff-db (X5)",
                    d.pkg
                ),
            });
        }
        if d.dep == "mlua" {
            let gated_ok = d.optional
                && rules
                    .feature_gated
                    .get(&d.pkg)
                    .is_some_and(|g| g.contains("mlua"));
            if !rules.mlua_only_in.contains(&d.pkg) || (d.pkg == "flyff-data" && !gated_ok) {
                v.push(Violation { rule: "X6", message: format!("`{}` uses mlua; only flyff-lua (and flyff-data behind feature `load`) may (X6)", d.pkg) });
            }
        }
        if rules.x7_forbidden.contains(&d.dep) {
            v.push(Violation {
                rule: "X7",
                message: format!(
                    "`{}` depends on `{}`; inter-thread channels are flume only (X7, ADR-010)",
                    d.pkg, d.dep
                ),
            });
        }
        if rules.tool_only_crates.contains(&d.dep) && !rules.tool_only_allowed.contains(&d.pkg) {
            v.push(Violation { rule: "rule-4", message: format!("`{}` depends on `{}`; anyhow/clap only in flyff-server, xtask, tools/* and tests/ (Plan/02 §11.5 rule 4)", d.pkg, d.dep) });
        }
    }
    v
}

/// The dependency tree printed by `cargo tree --prefix depth -f "{p}|{f}"`.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// First line's package.
    pub root: String,
    /// Direct children by parent name.
    pub edges: BTreeMap<String, BTreeSet<String>>,
    /// Enabled features by package name (union over occurrences).
    pub features: BTreeMap<String, BTreeSet<String>>,
}

/// Parses `cargo tree --prefix depth -f "{p}|{f}"` output.
pub fn parse_tree(text: &str) -> Tree {
    let mut tree = Tree::default();
    let mut stack: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end().trim_end_matches("(*)").trim_end();
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let Ok(depth) = line[..digits].parse::<usize>() else {
            continue;
        };
        let rest = &line[digits..];
        let (pkg, feats) = rest.split_once('|').unwrap_or((rest, ""));
        let Some(name) = pkg.split_whitespace().next().map(str::to_owned) else {
            continue;
        };
        stack.truncate(depth);
        if let Some(parent) = stack.last() {
            tree.edges
                .entry(parent.clone())
                .or_default()
                .insert(name.clone());
        } else {
            tree.root.clone_from(&name);
        }
        let set = tree.features.entry(name.clone()).or_default();
        for f in feats.split(',').map(str::trim).filter(|f| !f.is_empty()) {
            set.insert(f.to_owned());
        }
        stack.push(name);
    }
    tree
}

impl Tree {
    /// All package names in the tree.
    pub fn names(&self) -> BTreeSet<String> {
        let mut s: BTreeSet<String> = self.features.keys().cloned().collect();
        s.insert(self.root.clone());
        s
    }

    /// A dependency path `root -> ... -> target`, if reachable.
    pub fn path_to(&self, target: &str) -> Option<Vec<String>> {
        let mut prev: BTreeMap<&str, &str> = BTreeMap::new();
        let mut queue: VecDeque<&str> = VecDeque::from([self.root.as_str()]);
        let mut seen: BTreeSet<&str> = BTreeSet::from([self.root.as_str()]);
        while let Some(n) = queue.pop_front() {
            if n == target {
                let mut path = vec![n.to_owned()];
                let mut cur = n;
                while let Some(p) = prev.get(cur) {
                    path.push((*p).to_owned());
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            for c in self.edges.get(n).into_iter().flatten() {
                if seen.insert(c.as_str()) {
                    prev.insert(c.as_str(), n);
                    queue.push_back(c.as_str());
                }
            }
        }
        None
    }
}

/// Rule 1: the `flyff-game` closure contains none of the forbidden crates.
pub fn check_game_closure(rules: &Rules, tree: &Tree) -> Vec<Violation> {
    rules
        .game_forbidden
        .iter()
        .filter_map(|bad| {
            tree.path_to(bad).map(|p| Violation {
                rule: "game-closure",
                message: format!("`{bad}` is reachable from flyff-game ({}); flyff-game must not depend on tokio/axum (Plan/02 §11.5 rule 1)", p.join(" -> ")),
            })
        })
        .collect()
}

/// Rule 2: the wasm closure. `workspace` = workspace member names.
pub fn check_wasm_closure(
    rules: &Rules,
    tree: &Tree,
    workspace: &BTreeSet<String>,
) -> Vec<Violation> {
    let mut v = Vec::new();
    for bad in &rules.wasm_deny {
        if let Some(p) = tree.path_to(bad) {
            v.push(Violation { rule: "wasm-deny", message: format!("`{bad}` reaches the {} wasm graph ({}); never allowed there (Plan/02 §11.5 rule 2)", rules.wasm_root, p.join(" -> ")) });
        }
    }
    for (parent, children) in &tree.edges {
        if !workspace.contains(parent) {
            continue;
        }
        for c in children {
            if !workspace.contains(c) && !rules.wasm_allow.contains(c) {
                v.push(Violation { rule: "wasm-allow", message: format!("`{c}` is a direct dependency of `{parent}` in the {} wasm graph but not in xtask/lint/deps-allow.toml [wasm].allow (Plan/02 §11.5 rule 2)", rules.wasm_root) });
            }
        }
    }
    v
}

/// Rule 5: `writers` / `testkit` features must not be active for workspace crates in a shipped graph.
pub fn check_shipped_features(
    graph: &str,
    tree: &Tree,
    workspace: &BTreeSet<String>,
) -> Vec<Violation> {
    let mut v = Vec::new();
    for (name, feats) in &tree.features {
        if !workspace.contains(name) {
            continue;
        }
        for bad in ["writers", "testkit", "test-hooks", "testing"] {
            if feats.contains(bad) {
                v.push(Violation { rule: "shipped-features", message: format!("feature `{bad}` of `{name}` is active in the {graph} graph; test features never ship (Plan/02 §11.5 rule 5)") });
            }
        }
    }
    v
}

/// Every workspace manifest must opt into the workspace lints (`[lints] workspace = true`).
pub fn check_lints_opt_in(manifests: &[(String, String)]) -> Vec<Violation> {
    manifests
        .iter()
        .filter(|(_, text)| {
            let Ok(doc) = text.parse::<toml::Table>() else {
                return true;
            };
            doc.get("lints")
                .and_then(|l| l.get("workspace"))
                .and_then(toml::Value::as_bool)
                != Some(true)
        })
        .map(|(name, _)| Violation {
            rule: "lints",
            message: format!("`{name}` must set `[lints] workspace = true` (Plan/02 §1.3)"),
        })
        .collect()
}

fn declared(meta: &Meta) -> (Vec<Decl>, BTreeSet<String>) {
    let workspace: BTreeSet<String> = meta.members().map(|p| p.name.clone()).collect();
    let mut decls = Vec::new();
    for p in meta.members() {
        for d in &p.dependencies {
            let kind = match d.kind.as_deref() {
                Some("dev") => Kind::Dev,
                Some("build") => Kind::Build,
                _ => Kind::Normal,
            };
            decls.push(Decl {
                pkg: p.name.clone(),
                dep: d.name.clone(),
                kind,
                optional: d.optional,
            });
        }
    }
    (decls, workspace)
}

fn tree_of(ctx: &Ctx, args: &[&str]) -> Result<Tree> {
    let out = Run::new(ctx, "cargo")
        .args([
            "tree",
            "--prefix",
            "depth",
            "--edges",
            "normal,build",
            "-f",
            "{p}|{f}",
        ])
        .args(args)
        .output()
        .context("cargo tree")?;
    Ok(parse_tree(&out))
}

/// Runs every rule against the real workspace.
pub fn run(ctx: &Ctx) -> Result<()> {
    let rules = load_rules(ctx)?;
    let meta = Meta::load(ctx, None, true)?;
    let (decls, workspace) = declared(&meta);
    let mut violations = check_declared(&rules, &decls, &workspace);

    let manifests: Vec<(String, String)> = meta
        .members()
        .map(|p| {
            (
                p.name.clone(),
                std::fs::read_to_string(&p.manifest_path).unwrap_or_default(),
            )
        })
        .collect();
    violations.extend(check_lints_opt_in(&manifests));

    let game = tree_of(ctx, &["-p", "flyff-game"])?;
    violations.extend(check_game_closure(&rules, &game));

    let wasm = tree_of(
        ctx,
        &["-p", &rules.wasm_root, "--target", "wasm32-unknown-unknown"],
    )?;
    violations.extend(check_wasm_closure(&rules, &wasm, &workspace));
    violations.extend(check_shipped_features("wasm-release", &wasm, &workspace));

    let dist = tree_of(
        ctx,
        &[
            "-p",
            "flyff-server",
            "--target",
            "x86_64-unknown-linux-musl",
            "--features",
            "embed-web",
        ],
    )?;
    violations.extend(check_shipped_features("dist", &dist, &workspace));

    if violations.is_empty() {
        println!(
            "dep-rules: OK ({} declared edges, game closure {} crates, wasm closure {} crates, dist closure {} crates)",
            decls.len(),
            game.names().len(),
            wasm.names().len(),
            dist.names().len()
        );
        Ok(())
    } else {
        for v in &violations {
            eprintln!("dep-rules: {v}");
        }
        bail!("dep-rules found {} violation(s)", violations.len())
    }
}
