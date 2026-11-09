//! Common rules of the per-character JSON documents (Plan/13 §6.1).
//!
//! Every stored document is a JSON **object** whose first field is `"schema_version"`. Documents are
//! pure data: serialised on the persistence thread, deserialised on reader threads. Unknown fields are
//! ignored on read (no `deny_unknown_fields`); the SQL version guard keeps an older binary from rewriting a
//! newer document. Old versions are upgraded in Rust ([`JsonDoc::upgrade`]) and the next save writes the
//! current version (lazy migration). A `SCHEMA_VERSION` bump must be paired with a (possibly comment-only)
//! SQL migration.

use crate::error::{DbError, DbErrorKind};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// A JSON document stored in a TEXT column (`character_docs.*`, `banks.container`, `guild_banks.container`,
/// `mails.item`, ...).
pub trait JsonDoc: Serialize + DeserializeOwned + Sized {
    /// Column / document name used in error messages (`"inventory"`).
    const KIND: &'static str;
    /// The version written by this binary (13 §6.1: 1 for every document).
    const SCHEMA_VERSION: u32;

    /// Upgrades the JSON of version `from` (`< SCHEMA_VERSION`) to the next version. Called repeatedly by
    /// [`JsonDoc::from_json`] until the current version is reached. The default is the identity, which is
    /// correct while only version 1 exists.
    fn upgrade(value: Value, from: u32) -> Result<Value, DbError> {
        let _ = from;
        Ok(value)
    }

    /// Serialises to compact JSON.
    fn to_json(&self) -> Result<String, DbError> {
        serde_json::to_string(self).map_err(|e| {
            DbError::new(
                DbErrorKind::Bug,
                format!("serialising {} document: {e}", Self::KIND),
            )
        })
    }

    /// Parses stored JSON: reads `schema_version` (a missing field means 1), refuses newer versions
    /// (`NewerSchema`), upgrades older ones and deserialises. Malformed documents are `Bug` errors that name
    /// the document kind.
    fn from_json(text: &str) -> Result<Self, DbError> {
        let ctx = |what: &str, e: &dyn std::fmt::Display| {
            DbError::new(
                DbErrorKind::Bug,
                format!("{} document: {what}: {e}", Self::KIND),
            )
        };
        let mut value: Value = serde_json::from_str(text).map_err(|e| ctx("invalid JSON", &e))?;
        let mut version = document_version(&value, Self::KIND)?;
        if version > Self::SCHEMA_VERSION {
            return Err(DbError::new(
                DbErrorKind::NewerSchema,
                format!(
                    "{} document has schema_version {version}, this binary understands up to {}",
                    Self::KIND,
                    Self::SCHEMA_VERSION
                ),
            ));
        }
        while version < Self::SCHEMA_VERSION {
            value = Self::upgrade(value, version)?;
            version += 1;
            if let Value::Object(map) = &mut value {
                map.insert("schema_version".to_owned(), Value::from(version));
            }
        }
        serde_json::from_value(value).map_err(|e| ctx("does not match the schema", &e))
    }
}

/// `schema_version` of a parsed document (1 when the field is missing); the value must be an object.
fn document_version(value: &Value, kind: &str) -> Result<u32, DbError> {
    let Value::Object(map) = value else {
        return Err(DbError::new(
            DbErrorKind::Bug,
            format!("{kind} document is not a JSON object"),
        ));
    };
    match map.get("schema_version") {
        None => Ok(1),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| {
                DbError::new(
                    DbErrorKind::Bug,
                    format!("{kind} document: schema_version is not a small unsigned integer"),
                )
            }),
    }
}

/// serde default of every document's `schema_version`: a missing field means version 1.
pub(crate) const fn v1() -> u32 {
    1
}

/// `skip_serializing_if` helpers.
pub(crate) mod skip {
    pub(crate) fn is_zero_u8(v: &u8) -> bool {
        *v == 0
    }
    pub(crate) fn is_zero_i32(v: &i32) -> bool {
        *v == 0
    }
    pub(crate) fn is_zero_u32(v: &u32) -> bool {
        *v == 0
    }
    pub(crate) fn is_zero_i64(v: &i64) -> bool {
        *v == 0
    }
    pub(crate) fn is_zero_u64(v: &u64) -> bool {
        *v == 0
    }
    pub(crate) fn is_false(v: &bool) -> bool {
        !*v
    }
    pub(crate) fn is_minus_one(v: &i32) -> bool {
        *v == -1
    }
}

/// serde default helpers.
pub(crate) mod default {
    pub(crate) const fn one_u16() -> u16 {
        1
    }
    pub(crate) const fn minus_one() -> i32 {
        -1
    }
}
