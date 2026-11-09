//! JSON Schemas and TypeScript declarations generated from the game data types.

use super::schema::bundle_schemas;
use super::tsgen::generate;

/// The artifacts in a fixed order.
pub fn artifacts() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, doc) in bundle_schemas() {
        let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
        text.push('\n');
        out.push((format!("schemas/data/{name}.schema.json"), text));
    }
    for f in generate() {
        out.push((format!("web/src/gen/bundles/{}", f.name), f.text));
    }
    out
}
