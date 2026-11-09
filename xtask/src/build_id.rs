//! Build identity (Plan/02 §8.7) and `Cargo.lock` helpers.

use crate::proc::{Ctx, Run};
use anyhow::{Context, Result};

/// `<version>+<git-sha-12>[.dirty]`.
pub fn format_build_id(version: &str, sha12: Option<&str>, dirty: bool) -> String {
    match sha12 {
        Some(sha) => format!("{version}+{sha}{}", if dirty { ".dirty" } else { "" }),
        None => format!("{version}+dev"),
    }
}

/// The workspace version from the root `Cargo.toml` (`[workspace.package] version`).
pub fn workspace_version(ctx: &Ctx) -> Result<String> {
    let text = std::fs::read_to_string(ctx.path("Cargo.toml")).context("read Cargo.toml")?;
    version_from_manifest(&text).context("[workspace.package] version missing in Cargo.toml")
}

/// Extracts `[workspace.package] version` from manifest text.
pub fn version_from_manifest(text: &str) -> Option<String> {
    let doc: toml::Table = text.parse().ok()?;
    doc.get("workspace")?
        .get("package")?
        .get("version")?
        .as_str()
        .map(str::to_owned)
}

/// Computes the build id with git (falls back to `<version>+dev` outside a git checkout).
pub fn compute(ctx: &Ctx) -> Result<String> {
    let version = workspace_version(ctx)?;
    let sha = Run::new(ctx, "git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .map(|s| s.trim().to_owned());
    let sha = sha.filter(|s| !s.is_empty());
    let dirty = if sha.is_some() {
        Run::new(ctx, "git")
            .args(["status", "--porcelain"])
            .output()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    } else {
        false
    };
    Ok(format_build_id(&version, sha.as_deref(), dirty))
}

/// The resolved version of `package` in `Cargo.lock` text (`None` when absent or ambiguous).
pub fn lock_version(lock_text: &str, package: &str) -> Option<String> {
    let doc: toml::Table = lock_text.parse().ok()?;
    let packages = doc.get("package")?.as_array()?;
    let mut found = packages
        .iter()
        .filter(|p| p.get("name").and_then(toml::Value::as_str) == Some(package))
        .filter_map(|p| p.get("version").and_then(toml::Value::as_str));
    let first = found.next()?;
    found.next().is_none().then(|| first.to_owned())
}

/// `lock_version` for the repository's `Cargo.lock`.
pub fn repo_lock_version(ctx: &Ctx, package: &str) -> Result<String> {
    let text = std::fs::read_to_string(ctx.path("Cargo.lock")).context("read Cargo.lock")?;
    lock_version(&text, package)
        .with_context(|| format!("`{package}` is not (uniquely) resolved in Cargo.lock"))
}
