//! Environment overrides `FLYFF__SECTION__KEY` (Plan/10 §8.3).

/// Environment variables with a single underscore that are NOT config overrides (Plan/02 §2.6, 10 §8.2):
/// they take part in the home/Resource resolution, the build, or are read by tests only. They are ignored
/// without a warning.
pub const RESERVED_ENV: &[&str] = &[
    "FLYFF_HOME",
    "FLYFF_RESOURCE",
    "FLYFF_REQUIRE_RESOURCE",
    "FLYFF_SOURCE",
    "FLYFF_BUILD_ID",
    "FLYFF_PLAN",
    "FLYFF_E2E_URL",
    "FLYFF_DEV_PORT",
    "FLYFF_PROTO_CORPUS",
    "FLYFF_WEB_DIST_DIR",
    "FLYFF_TARGET",
    "FLYFF_PROFILE",
    "FLYFF_WEB_FINGERPRINT",
    // The backup hook receives these (13 §21).
    "FLYFF_BACKUP_PATH",
    "FLYFF_BACKUP_KIND",
    "FLYFF_DB_PATH",
];

/// What an environment variable name means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvName {
    /// `FLYFF__NET__BIND` → `["net", "bind"]`.
    Override(Vec<String>),
    /// `FLYFF__…` with an empty part (`FLYFF____X`, `FLYFF__NET__`).
    Malformed,
    /// `FLYFF_…` (single underscore) that is not reserved: the operator probably meant an override.
    SingleUnderscore,
    /// Not ours (or reserved).
    Ignored,
}

/// Classifies an environment variable name.
pub fn classify(name: &str) -> EnvName {
    if let Some(rest) = name.strip_prefix("FLYFF__") {
        let parts: Vec<String> = rest.split("__").map(str::to_ascii_lowercase).collect();
        if parts.iter().any(String::is_empty) {
            return EnvName::Malformed;
        }
        return EnvName::Override(parts);
    }
    if name.starts_with("FLYFF_") && !RESERVED_ENV.contains(&name) {
        return EnvName::SingleUnderscore;
    }
    EnvName::Ignored
}

/// The value of an override: parsed as a TOML value (`'["http://a:1"]'`, `true`, `2.0`); when that fails
/// the raw text is used as a string (`Europe/Berlin`). For a key whose type is `string`, the raw text is
/// used unless it is a quoted TOML string, so `FLYFF__WEB__PAGE_TITLE=2024` stays text.
pub fn parse_value(raw: &str, key_is_string: bool) -> toml::Value {
    let parsed = format!("v = {raw}")
        .parse::<toml::Table>()
        .ok()
        .and_then(|mut t| t.remove("v"));
    match parsed {
        Some(v @ toml::Value::String(_)) => v,
        Some(_) if key_is_string => toml::Value::String(raw.to_owned()),
        Some(v) => v,
        None => toml::Value::String(raw.to_owned()),
    }
}

/// The canonical override name of a dotted key: `net.tls.cert` → `FLYFF__NET__TLS__CERT`.
pub fn env_name_of(dotted: &str) -> String {
    format!(
        "FLYFF__{}",
        dotted
            .split('.')
            .map(str::to_ascii_uppercase)
            .collect::<Vec<_>>()
            .join("__")
    )
}

/// Suggests the `FLYFF__…` form of a single-underscore name among `keys` (dotted): `FLYFF_NET_BIND` and
/// `FLYFF_BIND` both give `FLYFF__NET__BIND`; `None` when nothing matches uniquely.
pub fn suggest_override<'a>(name: &str, keys: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let rest = name.strip_prefix("FLYFF_")?.to_ascii_lowercase();
    let mut by_last: Vec<&str> = Vec::new();
    for key in keys {
        if key.replace('.', "_") == rest {
            return Some(env_name_of(key));
        }
        if key.rsplit('.').next() == Some(rest.as_str()) {
            by_last.push(key);
        }
    }
    match by_last.as_slice() {
        [only] => Some(env_name_of(only)),
        _ => None,
    }
}
