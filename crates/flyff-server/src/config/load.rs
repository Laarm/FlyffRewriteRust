//! The loader (Plan/10 §8.2–§8.4): `flyff.toml` (with the line of every key) → environment
//! `FLYFF__SECTION__KEY` → CLI flags and programmatic overrides → the retired-key pass → unknown-key check →
//! per-key type check → deserialisation → semantic validation. Every problem is collected; the caller prints
//! them and exits 78.

use super::Config;
use super::env::{EnvName, classify, parse_value, suggest_override};
use super::error::{ConfigError, ConfigWarning, ValueSource};
use super::keys::{Kind, all_keys, is_known_path, is_section, key_info};
use super::retired::RETIRED_KEYS;
use super::validate::{check_tls_files, validate};
use flyff_common::config::clamp_reserved;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Command-line overrides (rank 4 of §8.2).
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    /// `--bind` → `net.bind`.
    pub bind: Option<String>,
    /// `--resource` → `paths.resource_dir`.
    pub resource: Option<PathBuf>,
    /// `--dev` → `debug.dev_surfaces = true`.
    pub dev: bool,
}

/// The result of [`load`].
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// The effective configuration.
    pub config: Config,
    /// Source of every explicitly set leaf key (absent = default).
    pub sources: BTreeMap<String, ValueSource>,
    /// Non-fatal findings (retired keys, reserved keys, odd environment names, …).
    pub warnings: Vec<ConfigWarning>,
}

/// Loads `<home>/flyff.toml` (if present) and applies env, CLI and programmatic overrides. `env` is the
/// list of `(name, value)` pairs to consider (the caller passes the process environment).
pub fn load(
    home: &Path,
    cli: &CliOverrides,
    env: &[(String, String)],
    overrides: &[(String, toml::Value)],
) -> Result<LoadedConfig, Vec<ConfigError>> {
    let path = home.join("flyff.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(vec![ConfigError::new(
                "",
                ValueSource::Default,
                format!("cannot read {}: {e}", path.display()),
            )]);
        }
    };
    load_inner(Some(home), text.as_deref(), cli, env, overrides)
}

/// [`load`] on the file text (unit tests; the TLS files are not checked without a home).
pub fn load_from_str(
    file_text: Option<&str>,
    cli: &CliOverrides,
    env: &[(String, String)],
    overrides: &[(String, toml::Value)],
) -> Result<LoadedConfig, Vec<ConfigError>> {
    load_inner(None, file_text, cli, env, overrides)
}

fn load_inner(
    home: Option<&Path>,
    file_text: Option<&str>,
    cli: &CliOverrides,
    env: &[(String, String)],
    overrides: &[(String, toml::Value)],
) -> Result<LoadedConfig, Vec<ConfigError>> {
    let mut errors: Vec<ConfigError> = Vec::new();
    let mut warnings: Vec<ConfigWarning> = Vec::new();
    let mut sources: BTreeMap<String, ValueSource> = BTreeMap::new();

    // ---- rank 2: the file ----
    let mut table = toml::Table::new();
    if let Some(text) = file_text {
        match text.parse::<toml::Table>() {
            Ok(t) => table = t,
            Err(e) => {
                return Err(vec![ConfigError::new(
                    "",
                    ValueSource::File {
                        line: e.span().map(|s| line_of(text, s.start)),
                    },
                    format!("flyff.toml is not valid TOML: {e}"),
                )]);
            }
        }
        for (key, line) in key_lines(text) {
            sources.insert(key, ValueSource::File { line: Some(line) });
        }
        // Keys the line scan could not see (inline tables) still have the file as source.
        let mut leaves = Vec::new();
        collect_leaves(&table, "", &mut leaves);
        for leaf in leaves {
            sources
                .entry(leaf)
                .or_insert(ValueSource::File { line: None });
        }
    }

    // ---- rank 3: environment ----
    let mut env_sorted: Vec<&(String, String)> = env.iter().collect();
    env_sorted.sort();
    for (name, value) in env_sorted {
        match classify(name) {
            EnvName::Override(parts) => {
                let dotted = parts.join(".");
                let is_str = key_info(&dotted).is_some_and(|k| k.kind == Kind::Str);
                set_path(&mut table, &parts, parse_value(value, is_str));
                sources.insert(dotted, ValueSource::Env { var: name.clone() });
            }
            EnvName::Malformed => errors.push(ConfigError::new(
                name.clone(),
                ValueSource::Env { var: name.clone() },
                "malformed override variable name (expected FLYFF__SECTION__KEY)",
            )),
            EnvName::SingleUnderscore => {
                let hint = suggest_override(name, all_keys().iter().map(|k| k.key))
                    .unwrap_or_else(|| "FLYFF__SECTION__KEY".to_owned());
                warnings.push(ConfigWarning::general(format!(
                    "environment variable {name} is ignored: did you mean {hint}?"
                )));
            }
            EnvName::Ignored => {}
        }
    }

    // ---- rank 4: CLI flags, then programmatic overrides ----
    let mut set = |dotted: &str, value: toml::Value, source: ValueSource| {
        let parts: Vec<String> = dotted.split('.').map(str::to_owned).collect();
        set_path(&mut table, &parts, value);
        sources.insert(dotted.to_owned(), source);
    };
    if let Some(bind) = &cli.bind {
        set(
            "net.bind",
            toml::Value::String(bind.clone()),
            ValueSource::Cli { flag: "--bind" },
        );
    }
    if let Some(res) = &cli.resource {
        set(
            "paths.resource_dir",
            toml::Value::String(res.to_string_lossy().into_owned()),
            ValueSource::Cli { flag: "--resource" },
        );
    }
    if cli.dev {
        set(
            "debug.dev_surfaces",
            toml::Value::Boolean(true),
            ValueSource::Cli { flag: "--dev" },
        );
    }
    for (key, value) in overrides {
        set(
            key,
            value.clone(),
            ValueSource::Override {
                set_by: "options".to_owned(),
            },
        );
    }

    // ---- retired keys: warn and drop ----
    for (old, new) in RETIRED_KEYS {
        if remove_path(&mut table, old) {
            sources.remove(*old);
            warnings.push(ConfigWarning::for_key(
                *old,
                match new {
                    Some(n) => format!("'{old}' is retired; use '{n}' (value ignored)"),
                    None => format!("'{old}' is retired and has no replacement (value ignored)"),
                },
            ));
        }
    }

    // ---- unknown keys ----
    let mut unknown = Vec::new();
    find_unknown(&table, "", &mut unknown);
    for key in &unknown {
        errors.push(ConfigError::new(
            key.clone(),
            sources
                .get(key)
                .cloned()
                .unwrap_or(ValueSource::File { line: None }),
            match suggest(key) {
                Some(s) => format!("unknown key (did you mean '{s}'?)"),
                None => "unknown key".to_owned(),
            },
        ));
        remove_path(&mut table, key);
    }

    // ---- per-key type check, so independent mistakes are all reported ----
    let mut leaves = Vec::new();
    collect_leaves(&table, "", &mut leaves);
    for leaf in leaves {
        let Some(value) = crate::config::lookup(&table, &leaf).cloned() else {
            continue;
        };
        let parts: Vec<String> = leaf.split('.').map(str::to_owned).collect();
        let mut probe = toml::Table::new();
        set_path(&mut probe, &parts, value);
        if let Err(e) = toml::Value::Table(probe).try_into::<Config>() {
            errors.push(ConfigError::new(
                leaf.clone(),
                sources
                    .get(&leaf)
                    .cloned()
                    .unwrap_or(ValueSource::File { line: None }),
                format!("invalid value: {}", first_line(&e.to_string())),
            ));
            remove_path(&mut table, &leaf);
        }
    }

    // ---- deserialise, clamp reserved keys, validate ----
    let mut config: Config = match toml::Value::Table(table).try_into() {
        Ok(c) => c,
        Err(e) => {
            errors.push(ConfigError::new(
                "",
                ValueSource::File { line: None },
                format!("invalid configuration: {}", first_line(&e.to_string())),
            ));
            return Err(errors);
        }
    };
    let mut rules = config.rules();
    for key in clamp_reserved(&mut rules) {
        warnings.push(ConfigWarning::for_key(
            key,
            format!("'{key}' is reserved and not implemented in v1: it behaves as false"),
        ));
    }
    config.features = rules.features;
    if config.auth.second_password {
        config.auth.second_password = false;
        warnings.push(ConfigWarning::for_key(
            "auth.second_password",
            "'auth.second_password' is reserved (F-053) and not implemented: it behaves as false",
        ));
    }
    if config.net.ws_permessage_deflate {
        warnings.push(ConfigWarning::for_key(
            "net.ws_permessage_deflate",
            "'net.ws_permessage_deflate' is not supported yet: compression stays off",
        ));
    }
    let semantic = validate(&config, &sources);
    let semantic_ok = semantic.is_empty();
    errors.extend(semantic);
    if semantic_ok
        && errors.is_empty()
        && let Some(home) = home
    {
        errors.extend(check_tls_files(&config, home, &sources));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(LoadedConfig {
        config,
        sources,
        warnings,
    })
}

// ---------------------------------------------------------------------------------------------
// table helpers
// ---------------------------------------------------------------------------------------------

fn first_line(s: &str) -> &str {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or(s).trim()
}

fn line_of(text: &str, byte: usize) -> u32 {
    let upto = text.get(..byte.min(text.len())).unwrap_or("");
    u32::try_from(upto.matches('\n').count() + 1).unwrap_or(u32::MAX)
}

/// The dotted key and 1-based line of every key written in the file (from the spanned parse).
fn key_lines(text: &str) -> Vec<(String, u32)> {
    fn walk(table: &toml::de::DeTable<'_>, prefix: &str, text: &str, out: &mut Vec<(String, u32)>) {
        for (k, v) in table {
            let name: &str = k.as_ref();
            let dotted = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}.{name}")
            };
            match v.as_ref() {
                toml::de::DeValue::Table(inner) => walk(inner, &dotted, text, out),
                _ => out.push((dotted, line_of(text, k.span().start))),
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(doc) = toml::de::DeTable::parse(text) {
        walk(doc.as_ref(), "", text, &mut out);
    }
    out
}

fn set_path(table: &mut toml::Table, parts: &[String], value: toml::Value) {
    match parts {
        [] => {}
        [last] => {
            table.insert(last.clone(), value);
        }
        [head, rest @ ..] => {
            let entry = table
                .entry(head.clone())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::Table::new());
            }
            if let toml::Value::Table(inner) = entry {
                set_path(inner, rest, value);
            }
        }
    }
}

/// Removes the dotted path; tables left empty under a name that is not a section are removed too.
fn remove_path(table: &mut toml::Table, dotted: &str) -> bool {
    fn go(table: &mut toml::Table, parts: &[&str], prefix: &str) -> bool {
        match parts {
            [] => false,
            [last] => table.remove(*last).is_some(),
            [head, rest @ ..] => {
                let path = if prefix.is_empty() {
                    (*head).to_owned()
                } else {
                    format!("{prefix}.{head}")
                };
                let removed = match table.get_mut(*head) {
                    Some(toml::Value::Table(inner)) => go(inner, rest, &path),
                    _ => false,
                };
                if removed
                    && !is_section(&path)
                    && matches!(table.get(*head), Some(toml::Value::Table(t)) if t.is_empty())
                {
                    table.remove(*head);
                }
                removed
            }
        }
    }
    let parts: Vec<&str> = dotted.split('.').collect();
    go(table, &parts, "")
}

fn collect_leaves(table: &toml::Table, prefix: &str, out: &mut Vec<String>) {
    for (k, v) in table {
        let dotted = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(inner) if is_known_path(&dotted) && key_info(&dotted).is_none() => {
                collect_leaves(inner, &dotted, out);
            }
            _ => out.push(dotted),
        }
    }
}

/// Paths that name nothing: every leaf under an unknown table, and unknown keys of known tables (so each
/// error can name the file line or the environment variable it came from).
fn find_unknown(table: &toml::Table, prefix: &str, out: &mut Vec<String>) {
    fn leaves_below(table: &toml::Table, prefix: &str, out: &mut Vec<String>) {
        for (k, v) in table {
            let dotted = format!("{prefix}.{k}");
            match v {
                toml::Value::Table(inner) if !inner.is_empty() => leaves_below(inner, &dotted, out),
                _ => out.push(dotted),
            }
        }
    }
    for (k, v) in table {
        let dotted = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(inner) if is_section(&dotted) => find_unknown(inner, &dotted, out),
            _ if key_info(&dotted).is_some() => {}
            toml::Value::Table(inner) if !inner.is_empty() => leaves_below(inner, &dotted, out),
            _ => out.push(dotted),
        }
    }
}

/// The closest known key or section (edit distance <= 3).
fn suggest(unknown: &str) -> Option<String> {
    let sections = super::keys::SECTION_ORDER.iter().copied();
    all_keys()
        .iter()
        .map(|k| k.key)
        .chain(sections)
        .map(|k| (edit_distance(unknown, k), k))
        .filter(|(d, _)| *d <= 3)
        .min()
        .map(|(_, k)| k.to_owned())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let sub = prev.get(j).copied().unwrap_or(0) + cost;
            let del = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let ins = cur.get(j).copied().unwrap_or(0) + 1;
            cur.push(sub.min(del).min(ins));
        }
        prev = cur;
    }
    prev.get(b.len()).copied().unwrap_or(0)
}
