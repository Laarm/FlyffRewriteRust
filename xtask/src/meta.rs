//! `cargo metadata` access shared by `gen`, `dep-rules` and `dist`.

use crate::proc::{Ctx, Run};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// The subset of `cargo metadata` we use.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Meta {
    /// All packages (workspace members and dependencies).
    pub packages: Vec<Package>,
    /// Package ids of the workspace members.
    #[serde(default)]
    pub workspace_members: Vec<String>,
    /// Resolved graph (absent with `--no-deps`).
    #[serde(default)]
    pub resolve: Option<Resolve>,
    /// The cargo target directory.
    #[serde(default)]
    pub target_directory: String,
}

/// A package.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Package {
    /// Unique id.
    pub id: String,
    /// Crate name.
    pub name: String,
    /// Version.
    #[serde(default)]
    pub version: String,
    /// License expression.
    #[serde(default)]
    pub license: Option<String>,
    /// Manifest path.
    #[serde(default)]
    pub manifest_path: String,
    /// Declared dependencies.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

/// A declared dependency.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Dependency {
    /// Crate name.
    pub name: String,
    /// `None` = normal, `Some("dev")`, `Some("build")`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Behind a feature.
    #[serde(default)]
    pub optional: bool,
}

/// The resolve section.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Resolve {
    /// Nodes by package.
    pub nodes: Vec<Node>,
}

/// One resolved package.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Node {
    /// Package id.
    pub id: String,
    /// Resolved dependencies.
    #[serde(default)]
    pub deps: Vec<NodeDep>,
}

/// An edge of the resolve graph.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeDep {
    /// Target package id.
    pub pkg: String,
    /// Kinds of the edge.
    #[serde(default)]
    pub dep_kinds: Vec<DepKind>,
}

/// Kind of an edge.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DepKind {
    /// `None` = normal.
    #[serde(default)]
    pub kind: Option<String>,
}

impl Meta {
    /// Runs `cargo metadata` (optionally filtered to a platform; `no_deps` skips the resolve).
    pub fn load(ctx: &Ctx, filter_platform: Option<&str>, no_deps: bool) -> Result<Meta> {
        let mut run = Run::new(ctx, "cargo").args(["metadata", "--format-version", "1"]);
        if no_deps {
            run = run.arg("--no-deps");
        }
        if let Some(p) = filter_platform {
            run = run.args(["--filter-platform", p]);
        }
        let json = run.output().context("cargo metadata")?;
        serde_json::from_str(&json).context("parse cargo metadata output")
    }

    /// Workspace member packages.
    pub fn members(&self) -> impl Iterator<Item = &Package> {
        let ids: BTreeSet<&str> = self.workspace_members.iter().map(String::as_str).collect();
        self.packages
            .iter()
            .filter(move |p| ids.contains(p.id.as_str()))
    }

    /// Names of all packages reachable from `root` over **normal** (non-dev, non-build) edges,
    /// including `root` itself.
    pub fn normal_closure(&self, root: &str) -> BTreeSet<String> {
        let Some(resolve) = &self.resolve else {
            return BTreeSet::new();
        };
        let by_id: BTreeMap<&str, &Node> =
            resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let names: BTreeMap<&str, &str> = self
            .packages
            .iter()
            .map(|p| (p.id.as_str(), p.name.as_str()))
            .collect();
        let Some(start) = self.packages.iter().find(|p| p.name == root) else {
            return BTreeSet::new();
        };
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut stack = vec![start.id.as_str()];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = by_id.get(id) {
                for d in &node.deps {
                    if d.dep_kinds.iter().any(|k| k.kind.is_none()) {
                        stack.push(d.pkg.as_str());
                    }
                }
            }
        }
        seen.into_iter()
            .filter_map(|id| names.get(id).map(|n| (*n).to_owned()))
            .collect()
    }
}
