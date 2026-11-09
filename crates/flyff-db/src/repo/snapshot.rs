//! Snapshot application (Plan/13 §16.2, §16.5): the generation guard, the `characters` snapshot columns, the
//! hash-skipping `character_docs` write and the banks.

use super::banks::write_snapshot_banks;
use super::characters::{SNAPSHOT_UPDATE_SQL, row_values};
use crate::error::DbError;
use crate::model::{CharacterDocs, CharacterSnapshot};
use crate::ops::ApplyCtx;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};
use xxhash_rust::xxh3::xxh3_64;

/// xxh3 of each serialised `character_docs` column, in [`CharacterDocs::COLUMNS`] order. The writer remembers
/// the hashes it last wrote for a character (reset on restart) and skips unchanged documents.
pub type DocHashes = [u64; 9];

/// What [`apply_snapshot`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapshotEffect {
    /// Older than (or equal to) an already written generation of the account: nothing was written.
    Stale,
    /// Written. `character_found` is `false` when the character row was not active any more (sealed or
    /// deleted in the same batch): a warning, not an error; the generation still counts as handled.
    Written {
        /// The `characters` row matched.
        character_found: bool,
        /// Number of `character_docs` columns written (0 when every hash was unchanged).
        doc_columns: usize,
    },
}

/// Applies one snapshot inside the caller's savepoint.
///
/// Gen guard: applied only if `gen > last_written_gen[account]`. A skipped snapshot is covered by a newer one.
pub(crate) fn apply_snapshot(
    conn: &Connection,
    ctx: &mut ApplyCtx<'_>,
    snap: &CharacterSnapshot,
) -> Result<SnapshotEffect, DbError> {
    let account = snap.account_id;
    if snap.r#gen <= ctx.gen_of(account) {
        ctx.own.snapshots_stale += 1;
        tracing::debug!(
            account = account.0,
            character = snap.character_id.0,
            gen = snap.r#gen.0,
            "stale snapshot skipped"
        );
        return Ok(SnapshotEffect::Stale);
    }

    let mut values = row_values(&snap.row)?;
    values.push(Value::Integer(ctx.now.0));
    values.push(Value::Integer(i64::from(snap.character_id.0)));
    values.push(Value::Integer(i64::from(account.0)));
    let changed = conn
        .prepare_cached(&SNAPSHOT_UPDATE_SQL)?
        .execute(params_from_iter(values))?;
    let mut doc_columns = 0;
    if changed == 0 {
        tracing::warn!(
            account = account.0,
            character = snap.character_id.0,
            gen = snap.r#gen.0,
            "snapshot of a character that is not active any more: columns and documents skipped"
        );
    } else {
        doc_columns = write_docs(conn, ctx, snap)?;
    }
    write_snapshot_banks(conn, account, &snap.banks)?;
    ctx.own.gens.insert(account, snap.r#gen);
    ctx.own.snapshots_written += 1;
    Ok(SnapshotEffect::Written {
        character_found: changed > 0,
        doc_columns,
    })
}

/// Serialises the nine documents, compares their hashes with the last written ones and updates the columns
/// that changed. Returns the number of columns written.
fn write_docs(
    conn: &Connection,
    ctx: &mut ApplyCtx<'_>,
    snap: &CharacterSnapshot,
) -> Result<usize, DbError> {
    let json = snap.docs.to_json_columns()?;
    let hashes: DocHashes = json.each_ref().map(|s| xxh3_64(s.as_bytes()));
    let previous = ctx.hashes_of(snap.character_id);
    let changed: Vec<usize> = (0..CharacterDocs::COLUMNS.len())
        .filter(|&i| previous.is_none_or(|p| p.get(i) != hashes.get(i)))
        .collect();
    if changed.is_empty() {
        return Ok(0);
    }
    let sets: Vec<String> = changed
        .iter()
        .filter_map(|&i| CharacterDocs::COLUMNS.get(i).map(|c| format!("{c} = ?")))
        .collect();
    let mut values: Vec<Value> = changed
        .iter()
        .filter_map(|&i| json.get(i).map(|s| Value::Text(s.clone())))
        .collect();
    values.push(Value::Integer(i64::from(snap.character_id.0)));
    let sql = format!(
        "UPDATE character_docs SET {} WHERE character_id = ?",
        sets.join(", ")
    );
    let n = conn
        .prepare_cached(&sql)?
        .execute(params_from_iter(values))?;
    if n == 0 {
        tracing::warn!(
            character = snap.character_id.0,
            "snapshot without a character_docs row"
        );
        return Ok(0);
    }
    ctx.own.hashes.insert(snap.character_id, hashes);
    Ok(changed.len())
}
