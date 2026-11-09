//! The commented default template (Plan/10 §8.7, R8) and the effective-configuration view.
//!
//! [`generate_template`] renders `flyff.toml` from the unified key table ([`super::keys`]) and
//! `Config::default()`: a header, then every section in the order of §8.7, every key commented out with its
//! default, preceded by `##` description lines (range, legacy source, ledger row, restart/runtime class).
//! The result is committed as `assets/flyff.toml.example` (`cargo xtask config-docs`); a test keeps the file
//! equal to the generator's output, so docs, defaults and template cannot drift.
//!
//! [`render_effective`] prints the effective values with their source (`# env FLYFF__NET__BIND`) for
//! `flyff config show`, masking secrets.

use super::keys::{KeyInfo, Kind, all_keys, section_of};
use super::load::LoadedConfig;
use super::schema::Reload;
use super::{Config, TEMPLATE, lookup};
use std::fmt::Write as _;

/// The embedded default template (the committed `assets/flyff.toml.example`).
pub fn default_template() -> &'static str {
    TEMPLATE
}

/// Keys whose effective value is never printed (`flyff config show`): `auth.legacy_md5_salt` is part of the
/// password scheme and `backup.hook` may carry credentials in its arguments.
pub const SECRET_KEYS: &[&str] = &["auth.legacy_md5_salt", "backup.hook"];

/// Marker of template lines that show an example for a key that is unset by default.
pub const UNSET_MARKER: &str = "# unset by default";

/// TOML text of a value; `f32` keys print the shortest `f32` form (`0.2`, not `0.20000000298023224`).
pub fn format_value(v: &toml::Value, ty: &str) -> String {
    match v {
        toml::Value::Float(f) if ty.contains("f32") => format!("{:?}", *f as f32),
        toml::Value::Float(f) => format!("{f:?}"),
        other => other.to_string(),
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn doc_lines(k: &KeyInfo, out: &mut String) {
    let mut push = |text: &str| {
        for line in wrap(text, 96) {
            let _ = writeln!(out, "## {line}");
        }
    };
    push(k.doc);
    let mut facts: Vec<String> = Vec::new();
    let range = k.range.describe();
    if !range.is_empty() {
        facts.push(format!("Values: {range}."));
    }
    if let Some(r) = k.remark {
        facts.push(r.to_owned());
    }
    if let Some(l) = k.legacy {
        facts.push(format!("Legacy: {l}."));
    }
    if let Some(l) = k.ledger {
        facts.push(format!("Ledger: {l}."));
    }
    let owner = if k.owner.chars().all(|c| c.is_ascii_digit()) {
        format!("Owner: doc {}.", k.owner)
    } else {
        let nums: Vec<&str> = k
            .owner
            .split('/')
            .filter_map(|d| d.split('-').next())
            .collect();
        format!("Owner: doc {}.", nums.join("/"))
    };
    facts.push(owner);
    facts.push(
        match (k.runtime, k.reload) {
            (true, _) => "Runtime key: GM/admin may change it live (kept in runtime_override).",
            (_, Reload::Yes) => "Applied by reload without a restart.",
            (_, Reload::No) => "Restart required.",
        }
        .to_owned(),
    );
    push(&facts.join(" "));
    if k.key == "gm.grants" {
        out.push_str(GM_GRANTS_EXAMPLE);
    }
}

/// The commented example of `[[gm.grants]]` (23 §1.3; PD-21: the live `test`/`test1` grants are not carried
/// over). `##` lines are never uncommented by the template tests.
const GM_GRANTS_EXAMPLE: &str = "## Example (exactly one of account / character / ip per entry; level is a letter F..P, an AUTH_* token\n\
     ## or player / logchat / journalist / helper / gm1 / gm2 / gm3 / operator / admin):\n\
     ##   [[gm.grants]]\n\
     ##   account = \"test\"\n\
     ##   level = \"admin\"\n\
     ##   [[gm.grants]]\n\
     ##   character = \"Tester\"\n\
     ##   level = \"gm3\"\n";

/// The sample shown for a key that is unset by default.
fn unset_sample(k: &KeyInfo) -> &'static str {
    match k.kind {
        Kind::Float => "1.0",
        Kind::Bool => "false",
        Kind::Str => "\"\"",
        _ => "0",
    }
}

/// Renders the commented default template from the key tables.
pub fn generate_template() -> String {
    let defaults = Config::default().to_table();
    let mut out = String::new();
    out.push_str(
        "# Flyff V15 server configuration (flyff.toml).\n\
         #\n\
         # Every key is optional: an empty file (or a file with everything commented out) reproduces this repack,\n\
         # and later changes of a default take effect after an upgrade. Lines of the form \"# key = value\" show the\n\
         # built-in default; remove the leading \"# \" to change a value.\n\
         #\n\
         # Precedence (later wins): built-in defaults, this file, environment variables FLYFF__SECTION__KEY\n\
         # (for example FLYFF__NET__BIND=127.0.0.1:9000; arrays as TOML: FLYFF__NET__ALLOWED_ORIGINS='[\"http://a:1\"]'),\n\
         # command-line flags (--bind, --resource, --dev). Rates and events can also be changed live by GMs and\n\
         # are then remembered in the database (runtime_override). Unknown keys are errors; retired keys are warnings.\n\
         # `flyff config show` prints the effective values and where they came from; `flyff config default`\n\
         # prints this file.\n\
         #\n\
         # Generated by `cargo xtask config-docs` from the key tables of docs 03, 10 and 13: do not edit by hand.\n",
    );
    let mut current: Option<&str> = None;
    for k in all_keys() {
        let section = section_of(k.key);
        if current != Some(section) {
            let _ = write!(out, "\n[{section}]\n");
            current = Some(section);
        }
        doc_lines(k, &mut out);
        let name = k.key.rsplit('.').next().unwrap_or(k.key);
        match lookup(&defaults, k.key) {
            Some(v) => {
                let _ = writeln!(out, "# {name} = {}", format_value(v, k.ty));
            }
            None => {
                let _ = writeln!(out, "# {name} = {}  {UNSET_MARKER}", unset_sample(k));
            }
        }
    }
    out
}

/// Effective values with their sources, for `flyff config show`. With `only_changed` only keys that were
/// set explicitly (file, env, flag, override) appear. Secrets are masked.
pub fn render_effective(loaded: &LoadedConfig, only_changed: bool) -> String {
    let table = loaded.config.to_table();
    let mut out = String::new();
    let mut current: Option<&str> = None;
    let mut pending_header = false;
    for k in all_keys() {
        let Some(value) = lookup(&table, k.key) else {
            continue;
        };
        let source = loaded.sources.get(k.key);
        if only_changed && source.is_none() {
            continue;
        }
        let section = section_of(k.key);
        if current != Some(section) {
            if current.is_some() && !pending_header {
                out.push('\n');
            }
            let _ = writeln!(out, "[{section}]");
            current = Some(section);
            pending_header = false;
        }
        let name = k.key.rsplit('.').next().unwrap_or(k.key);
        let text = if SECRET_KEYS.contains(&k.key) {
            if k.kind == Kind::StrArray {
                "[\"***\"]".to_owned()
            } else {
                "\"***\"".to_owned()
            }
        } else {
            format_value(value, k.ty)
        };
        match source.map(|s| s.annotation()).filter(|a| !a.is_empty()) {
            Some(a) => {
                let _ = writeln!(out, "{name} = {text}  {a}");
            }
            None => {
                let _ = writeln!(out, "{name} = {text}");
            }
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// `flyff config show` (the name of the skeleton; see [`render_effective`]).
pub fn render_show(loaded: &LoadedConfig, only_changed: bool) -> String {
    render_effective(loaded, only_changed)
}

/// `true` when `key` appears in the template as a (commented) key line.
pub fn template_has_key(template: &str, key: &str) -> bool {
    let name = key.rsplit('.').next().unwrap_or(key);
    let section = section_of(key);
    let mut in_section = false;
    for line in template.lines() {
        if let Some(h) = line.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            in_section = h == section;
            continue;
        }
        if in_section && line.starts_with(&format!("# {name} = ")) {
            return true;
        }
    }
    false
}

/// Every section header of the template in order (tests).
pub fn template_sections(template: &str) -> Vec<&str> {
    template
        .lines()
        .filter_map(|l| l.strip_prefix('[').and_then(|r| r.strip_suffix(']')))
        .collect()
}
