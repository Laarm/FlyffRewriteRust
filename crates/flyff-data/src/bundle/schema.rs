//! JSON Schemas of the bundles (Plan/14 §10.7, feature `schema`): derived with `schemars` from the DTOs of
//! [`super::types`], so the schema, the serde output and the TypeScript types ([`super::tsgen`]) come from the
//! one Rust definition.
//!
//! * [`bundle_schemas`] gives one document per bundle: the envelope `{bundle: const, schema: const, data}` whose
//!   `data` is the schema of the bundle's data type, plus `manifest`.
//! * A [`RowTable<T>`] is described as an object `{columns: string[], rows: array[]}` with the extension keys
//!   `x-flyff-row` (the schema of `T`), `x-flyff-columns` (the field names of `T` in declaration order); the
//!   property that holds it may add `x-flyff-omit` (columns that are not written). [`validate`] understands
//!   them and checks rows as typed tuples.
//! * [`validate`] is a small validator for exactly the subset of JSON Schema that these documents use (the
//!   workspace has no general validator; it exists so that tests check exported bundles against the generated
//!   schemas).

use super::rowtable::{RowTable, deserialize_fields};
use super::types::{
    BundleData, BundleManifest, DefinesBundle, HelpBundle, HonorBundle, ItemExtBundle, ItemsBundle,
    ModelBoundsBundle, ModelsBundle, MotionsBundle, MoversBundle, MusicBundle, NamesBundle,
    NpcsBundle, QuestsBundle, ResdataBundle, SfxTableBundle, SkillsBundle, SoundsBundle,
    TablesBundle, TerrainBundle, TextsBundle, UiAtlasesBundle, WorldsBundle,
};
use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use serde_json::{Value, json};
use std::borrow::Cow;

impl<T: JsonSchema + for<'de> Deserialize<'de>> JsonSchema for RowTable<T> {
    fn schema_name() -> Cow<'static, str> {
        format!("RowTable_{}", T::schema_name()).into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let row = generator.subschema_for::<T>().to_value();
        let columns = deserialize_fields::<T>().unwrap_or(&[]);
        let v = json!({
            "description": "A columnar table: `columns` names the fields of each row, `rows` holds one array of cells per row (14 §10.2).",
            "type": "object",
            "properties": {
                "columns": { "type": "array", "items": { "type": "string" } },
                "rows": { "type": "array", "items": { "type": "array" } }
            },
            "required": ["columns", "rows"],
            "x-flyff-row": row,
            "x-flyff-columns": columns,
        });
        Schema::try_from(v).unwrap_or_default()
    }
}

/// The generator settings: draft 2020-12, describing what is **written** (serialisation contract).
fn generator() -> SchemaGenerator {
    SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
}

fn data_schema<T: JsonSchema>() -> Value {
    generator().into_root_schema_for::<T>().to_value()
}

/// The envelope document of a bundle: `{bundle, schema, data}` with the constants and the data schema.
fn envelope_schema<T: JsonSchema + BundleData>() -> (String, Value) {
    let mut data = data_schema::<T>();
    let defs = data
        .as_object_mut()
        .and_then(|o| o.remove("$defs"))
        .unwrap_or_else(|| json!({}));
    if let Some(o) = data.as_object_mut() {
        o.remove("$schema");
    }
    let title = T::schema_name().into_owned();
    let mut defs_map = defs.as_object().cloned().unwrap_or_default();
    defs_map.insert(title.clone(), data);
    let doc = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": format!("{title}Envelope"),
        "description": format!("`{}.<h16>.json`: the envelope of the `{}` bundle (schema version {}).", T::NAME, T::NAME, T::SCHEMA),
        "type": "object",
        "properties": {
            "bundle": { "const": T::NAME },
            "schema": { "const": T::SCHEMA },
            "data": { "$ref": format!("#/$defs/{title}") }
        },
        "required": ["bundle", "schema", "data"],
        "$defs": Value::Object(defs_map),
    });
    (T::NAME.to_owned(), doc)
}

/// One JSON Schema document per bundle (by name, catalogue order) and the manifest schema (`"manifest"`).
pub fn bundle_schemas() -> Vec<(String, Value)> {
    let mut v = vec![
        envelope_schema::<ItemsBundle>(),
        envelope_schema::<ItemExtBundle>(),
        envelope_schema::<SkillsBundle>(),
        envelope_schema::<MoversBundle>(),
        envelope_schema::<NpcsBundle>(),
        envelope_schema::<TextsBundle>(),
        envelope_schema::<HelpBundle>(),
        envelope_schema::<WorldsBundle>(),
        envelope_schema::<ModelsBundle>(),
        envelope_schema::<SfxTableBundle>(),
        envelope_schema::<MotionsBundle>(),
        envelope_schema::<ModelBoundsBundle>(),
        envelope_schema::<TerrainBundle>(),
        envelope_schema::<ResdataBundle>(),
        envelope_schema::<UiAtlasesBundle>(),
        envelope_schema::<SoundsBundle>(),
        envelope_schema::<MusicBundle>(),
        envelope_schema::<HonorBundle>(),
        envelope_schema::<TablesBundle>(),
        envelope_schema::<NamesBundle>(),
        envelope_schema::<QuestsBundle>(),
        envelope_schema::<DefinesBundle>(),
    ];
    v.push(("manifest".to_owned(), data_schema::<BundleManifest>()));
    v
}

// ---- the validator ---------------------------------------------------------------------------------------------

/// Resolves a local `$ref` (`#/$defs/Name`).
fn resolve<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let path = reference.strip_prefix("#/")?;
    let mut cur = root;
    for seg in path.split('/') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

fn type_matches(ty: &str, v: &Value) -> bool {
    match ty {
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "number" => v.is_number(),
        "integer" => {
            v.is_i64()
                || v.is_u64()
                || v.as_f64().is_some_and(|f| {
                    f.fract() == 0.0 && v.is_number() && !v.to_string().contains('.')
                })
        }
        _ => true,
    }
}

/// Validates `value` against `schema` (a document of [`bundle_schemas`]). The error is the first violation with
/// its JSON path. Supported: `$ref`, `type` (string or list), `const`, `enum`, `properties`, `required`,
/// `additionalProperties` (schema), `items`, `prefixItems`, `minItems`/`maxItems`, `anyOf`/`oneOf`, `allOf` and
/// the `x-flyff-*` row-table extensions.
pub fn validate(root: &Value, schema: &Value, value: &Value) -> Result<(), String> {
    check(root, schema, value, "$", true)
}

/// `rows`: whether a row table found at this node is checked here (a `$ref` checks its target without the table
/// rules and then applies them once with the siblings of the `$ref`, which carry `x-flyff-omit`).
fn check(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    rows: bool,
) -> Result<(), String> {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let target = resolve(root, r).ok_or_else(|| format!("{path}: unresolved {r}"))?;
        check(root, target, value, path, false)?;
        return check_row_table(root, schema, target, value, path);
    }
    if let Some(c) = schema.get("const")
        && c != value
    {
        return Err(format!("{path}: expected the constant {c}, got {value}"));
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array)
        && !e.contains(value)
    {
        return Err(format!("{path}: {value} is not one of {e:?}"));
    }
    if let Some(t) = schema.get("type") {
        let ok = match t {
            Value::String(s) => type_matches(s, value),
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_str)
                .any(|s| type_matches(s, value)),
            _ => true,
        };
        if !ok {
            return Err(format!("{path}: type {t} does not match {}", short(value)));
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(alts) = schema.get(key).and_then(Value::as_array) {
            let mut errs = Vec::new();
            let mut any = false;
            for a in alts {
                match check(root, a, value, path, true) {
                    Ok(()) => {
                        any = true;
                        break;
                    }
                    Err(e) => errs.push(e),
                }
            }
            if !any {
                return Err(format!(
                    "{path}: no alternative matches ({})",
                    errs.join("; ")
                ));
            }
        }
    }
    if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
        for a in all {
            check(root, a, value, path, true)?;
        }
    }
    if let Some(obj) = value.as_object() {
        if let Some(req) = schema.get("required").and_then(Value::as_array) {
            for r in req.iter().filter_map(Value::as_str) {
                if !obj.contains_key(r) {
                    return Err(format!("{path}: missing property `{r}`"));
                }
            }
        }
        let props = schema.get("properties").and_then(Value::as_object);
        let additional = schema.get("additionalProperties");
        for (k, v) in obj {
            let p = format!("{path}.{k}");
            match props.and_then(|m| m.get(k)) {
                Some(s) => check(root, s, v, &p, true)?,
                None => {
                    if let Some(a) = additional.filter(|a| a.is_object()) {
                        check(root, a, v, &p, true)?;
                    }
                }
            }
        }
    }
    if let Some(arr) = value.as_array() {
        if let Some(min) = schema.get("minItems").and_then(Value::as_u64)
            && (arr.len() as u64) < min
        {
            return Err(format!(
                "{path}: {} items, at least {min} wanted",
                arr.len()
            ));
        }
        if let Some(max) = schema.get("maxItems").and_then(Value::as_u64)
            && arr.len() as u64 > max
        {
            return Err(format!("{path}: {} items, at most {max} wanted", arr.len()));
        }
        let prefix = schema.get("prefixItems").and_then(Value::as_array);
        for (i, v) in arr.iter().enumerate() {
            let p = format!("{path}[{i}]");
            if let Some(s) = prefix.and_then(|pi| pi.get(i)) {
                check(root, s, v, &p, true)?;
            } else if let Some(s) = schema.get("items").filter(|s| s.is_object()) {
                check(root, s, v, &p, true)?;
            }
        }
    }
    if rows {
        check_row_table(root, schema, schema, value, path)?;
    }
    Ok(())
}

/// `x-flyff-row` tables: `columns` must be the declared fields minus the omitted ones, and every row an array of
/// exactly that many cells, each matching the field schema of its column.
fn check_row_table(
    root: &Value,
    site: &Value,
    table: &Value,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    let (Some(row), Some(declared)) = (table.get("x-flyff-row"), table.get("x-flyff-columns"))
    else {
        return Ok(());
    };
    let omit: Vec<&str> = site
        .get("x-flyff-omit")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let obj = value
        .as_object()
        .ok_or_else(|| format!("{path}: a row table must be an object"))?;
    let cols: Vec<&str> = obj
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .ok_or_else(|| format!("{path}: no columns"))?;
    let rows = obj
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{path}: no rows"))?;
    if rows.is_empty() && cols.is_empty() {
        return Ok(());
    }
    let want: Vec<&str> = declared
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|c| !omit.contains(c))
                .collect()
        })
        .unwrap_or_default();
    if cols != want {
        return Err(format!(
            "{path}.columns: {cols:?} differs from the declared {want:?}"
        ));
    }
    let row_schema = match row.get("$ref").and_then(Value::as_str) {
        Some(r) => resolve(root, r).ok_or_else(|| format!("{path}: unresolved {r}"))?,
        None => row,
    };
    let props = row_schema.get("properties").and_then(Value::as_object);
    for (i, r) in rows.iter().enumerate() {
        let cells = r
            .as_array()
            .ok_or_else(|| format!("{path}.rows[{i}]: a row must be an array"))?;
        if cells.len() != cols.len() {
            return Err(format!(
                "{path}.rows[{i}]: {} cells for {} columns",
                cells.len(),
                cols.len()
            ));
        }
        for (c, cell) in cols.iter().zip(cells) {
            if let Some(s) = props.and_then(|p| p.get(*c)) {
                check(root, s, cell, &format!("{path}.rows[{i}].{c}"), true)?;
            }
        }
    }
    Ok(())
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 60 {
        format!("{}...", s.chars().take(60).collect::<String>())
    } else {
        s
    }
}
