//! The unified key table: every key of `flyff.toml`, from the three documentation sources of Plan/10 §8.7
//! (`OPS_KEYS` of this crate, `flyff_db::config::KEY_DOCS` of 13, `flyff_common::config::KEY_REGISTRY` of
//! 03), in template order. The loader (unknown keys, env typing, ranges), the template generator and the
//! docs all read this one table, so adding a key to its owner's table is all it takes.

use super::schema::{OPS_KEYS, Range, Reload};
use flyff_common::config::{KEY_REGISTRY, KeyClass};
use flyff_db::config::KEY_DOCS;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The TOML kind of a key's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Text.
    Str,
    /// Integer.
    Int,
    /// Float.
    Float,
    /// Boolean.
    Bool,
    /// Array of strings.
    StrArray,
    /// Array of tables (`gm.grants`).
    TableArray,
    /// A table of strings (`admin.capabilities`).
    Table,
}

impl Kind {
    fn of_rust(ty: &str) -> Kind {
        let t: String = ty.chars().filter(|c| !c.is_whitespace()).collect();
        match t.as_str() {
            "bool" => Kind::Bool,
            "f32" | "f64" | "Option<f32>" => Kind::Float,
            "String" | "HitAttrRule" => Kind::Str,
            "Vec<String>" => Kind::StrArray,
            "Vec<toml::Table>" => Kind::TableArray,
            "std::collections::BTreeMap<String,String>" | "BTreeMap<String,String>" => Kind::Table,
            _ => Kind::Int,
        }
    }

    fn of_toml(ty: &str) -> Kind {
        match ty {
            "bool" => Kind::Bool,
            "float" => Kind::Float,
            "string" => Kind::Str,
            "array of strings" => Kind::StrArray,
            _ => Kind::Int,
        }
    }
}

/// Which table documents a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// `OPS_KEYS` (doc 10, plus the keys registered for 12, 24, 21).
    Ops,
    /// `flyff_db::config::KEY_DOCS` (doc 13).
    Db,
    /// `flyff_common::config::KEY_REGISTRY` (doc 03).
    Registry,
}

/// One key of `flyff.toml`.
#[derive(Clone, Debug)]
pub struct KeyInfo {
    /// Dotted key (`net.tls.cert`).
    pub key: &'static str,
    /// Value kind.
    pub kind: Kind,
    /// The Rust type spelling (`u32`, `Option<f32>`) or the TOML kind of 13's table.
    pub ty: &'static str,
    /// Accepted values (ops keys; the registry's keys are free-form).
    pub range: Range,
    /// Applied by `reload` (ops) / changeable live (registry class R).
    pub reload: Reload,
    /// Runtime class (`[rates]`, `[events]`): changeable by GM/admin, persisted in `runtime_override`.
    pub runtime: bool,
    /// Description (one line).
    pub doc: &'static str,
    /// Owning plan document(s) (`"10"`, `"16-movers-stats-progression"`).
    pub owner: &'static str,
    /// Legacy reference (registry keys).
    pub legacy: Option<&'static str>,
    /// Ledger row (registry keys).
    pub ledger: Option<&'static str>,
    /// Extra remark for the template (`init-only`, `reserved`, …).
    pub remark: Option<&'static str>,
    /// Documented default as TOML text, `None` for keys that are unset by default (`rates.monster_exp`).
    pub default_text: Option<&'static str>,
    /// Which table documents the key.
    pub origin: Origin,
}

/// The sections in template order (Plan/10 §8.7). `net.tls` is a nested table.
pub const SECTION_ORDER: &[&str] = &[
    "server",
    "net",
    "net.tls",
    "auth",
    "web",
    "assets",
    "paths",
    "data",
    "runtime",
    "log",
    "admin",
    "admin.alerts",
    "debug",
    "database",
    "save",
    "backup",
    "audit",
    "retention",
    "rates",
    "events",
    "gameplay",
    "features",
    "fidelity",
    "guild",
    "gm",
];

/// The section of a dotted key (`net.tls.cert` → `net.tls`, `net.bind` → `net`).
pub fn section_of(key: &str) -> &str {
    key.rsplit_once('.').map_or(key, |(s, _)| s)
}

fn build() -> Vec<KeyInfo> {
    let mut out: Vec<KeyInfo> = Vec::new();
    for k in OPS_KEYS {
        out.push(KeyInfo {
            key: k.key,
            kind: Kind::of_rust(k.ty),
            ty: k.ty,
            range: k.range,
            reload: k.reload,
            runtime: false,
            doc: k.description,
            owner: k.owner,
            legacy: None,
            ledger: None,
            remark: None,
            default_text: Some(k.default),
            origin: Origin::Ops,
        });
    }
    for k in KEY_DOCS {
        out.push(KeyInfo {
            key: k.key,
            kind: Kind::of_toml(k.ty),
            ty: k.ty,
            range: if k.range.is_empty() {
                Range::Any
            } else {
                Range::Note(k.range)
            },
            reload: Reload::No,
            runtime: false,
            doc: k.description,
            owner: "13",
            legacy: None,
            ledger: None,
            remark: None,
            default_text: Some(k.default),
            origin: Origin::Db,
        });
    }
    for m in KEY_REGISTRY {
        let runtime = m.class == KeyClass::Runtime;
        out.push(KeyInfo {
            key: m.key,
            kind: Kind::of_rust(m.ty),
            ty: m.ty,
            range: Range::Any,
            reload: if runtime { Reload::Yes } else { Reload::No },
            runtime,
            doc: m.doc,
            owner: m.owner_doc,
            legacy: Some(m.legacy),
            ledger: m.ledger,
            remark: match (m.class, m.reserved) {
                (_, true) => Some(
                    "Reserved: not implemented in v1; true logs a warning and behaves as false.",
                ),
                (KeyClass::InitOnly, _) => Some(
                    "Init-only: recorded in the database when it is created; a different value later refuses to start.",
                ),
                _ => None,
            },
            default_text: None,
            origin: Origin::Registry,
        });
    }
    // Template order: by section, keeping the order inside each table.
    let rank = |k: &KeyInfo| {
        SECTION_ORDER
            .iter()
            .position(|s| *s == section_of(k.key))
            .unwrap_or(SECTION_ORDER.len())
    };
    out.sort_by_key(rank); // stable
    out
}

static KEYS: LazyLock<Vec<KeyInfo>> = LazyLock::new(build);
static INDEX: LazyLock<BTreeMap<&'static str, usize>> =
    LazyLock::new(|| KEYS.iter().enumerate().map(|(i, k)| (k.key, i)).collect());

/// Every key of `flyff.toml` in template order.
pub fn all_keys() -> &'static [KeyInfo] {
    &KEYS
}

/// The key `dotted`, if it exists.
pub fn key_info(dotted: &str) -> Option<&'static KeyInfo> {
    INDEX.get(dotted).and_then(|i| KEYS.get(*i))
}

/// `true` when `dotted` is a section or sub-table name (`net`, `net.tls`).
pub fn is_section(dotted: &str) -> bool {
    SECTION_ORDER.contains(&dotted)
}

/// `true` when `dotted` is a key, a section, or a prefix of a key (so a table at that path is legitimate).
pub fn is_known_path(dotted: &str) -> bool {
    INDEX.contains_key(dotted) || is_section(dotted)
}
