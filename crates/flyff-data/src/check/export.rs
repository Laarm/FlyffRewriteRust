//! `flyff export gamedata-json` (Plan/14 §9, WP-DATA-17): the loaded [`GameData`] as one pretty JSON file per
//! section. A debug and review aid: two exports of the same data are byte-identical (the test of the corpus suite
//! compares two loads), so a diff between two exports shows exactly what a loader change did.
//!
//! [`export_sections`] is pure (it returns the bytes); the CLI writes them (`<out>/<section>.json`). Sections are the
//! top-level fields of `GameData` plus `meta`, `defines` and `strings` (the two tables `GameData` skips when it
//! is serialised). Conventions:
//!
//! * objects are written with their keys in alphabetical order (the value tree is a `BTreeMap`), arrays and
//!   `IdTable`s in table order, so the output does not depend on any hash order;
//! * floats are the shortest decimal that round-trips the `f32` the loaders read;
//! * `meta.fingerprint` (a 128-bit number) is written as 32 hex digits and `meta.load_ms` is omitted.

use crate::game_data::GameData;
use serde_json::{Map, Value};

/// One file of the export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportSection {
    /// The section name; the file is `<name>.json`.
    pub name: String,
    /// The pretty-printed JSON with a trailing newline.
    pub json: Vec<u8>,
}

/// Why an export failed.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// A value could not be serialised (a map with a non-string key, for example).
    #[error("section {section}: {message}")]
    Serialize {
        /// The section.
        section: String,
        /// The serde message.
        message: String,
    },
}

fn pretty(section: &str, v: &Value) -> Result<ExportSection, ExportError> {
    let mut json = serde_json::to_vec_pretty(v).map_err(|e| ExportError::Serialize {
        section: section.to_owned(),
        message: e.to_string(),
    })?;
    json.push(b'\n');
    Ok(ExportSection {
        name: section.to_owned(),
        json,
    })
}

/// Serialises through text so that an `f32` keeps its shortest decimal form (`to_value` would widen it to an `f64`
/// with its long tail).
fn via_text<T: serde::Serialize>(section: &str, v: &T) -> Result<Value, ExportError> {
    let err = |e: serde_json::Error| ExportError::Serialize {
        section: section.to_owned(),
        message: e.to_string(),
    };
    let text = serde_json::to_string(v).map_err(err)?;
    serde_json::from_str(&text).map_err(err)
}

/// The sections of `g`, in alphabetical order of their names.
pub fn export_sections(g: &GameData) -> Result<Vec<ExportSection>, ExportError> {
    // the fingerprint is a u128 that the value tree cannot hold: serialise a copy without it
    let mut copy = g.clone();
    copy.meta.fingerprint = 0;
    let mut fields = match via_text("gamedata", &copy)? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    // meta: the fingerprint as hex, no load time
    let mut meta = match fields.remove("meta") {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    meta.insert(
        "fingerprint".to_owned(),
        Value::String(format!("{:032x}", g.meta.fingerprint)),
    );
    meta.remove("load_ms");
    fields.insert("meta".to_owned(), Value::Object(meta));
    // the two tables GameData skips
    let defines: Map<String, Value> = g
        .defines
        .iter()
        .map(|(n, v)| (n.to_owned(), Value::from(v)))
        .collect();
    fields.insert("defines".to_owned(), Value::Object(defines));
    let strings: Map<String, Value> = g
        .strings
        .iter()
        .map(|(k, v)| (k.to_owned(), Value::String(v.to_owned())))
        .collect();
    fields.insert("strings".to_owned(), Value::Object(strings));
    let mut out = Vec::with_capacity(fields.len());
    for (name, v) in &fields {
        out.push(pretty(name, v)?);
    }
    Ok(out)
}
