//! `party_names` (Plan/13 §7, 21 §4.4): the stored advanced-party name of a leader, one per character, unique
//! case-insensitively (`COLLATE NOCASE`). The party itself is never persisted (KQ-21).

use crate::error::{DbError, DbErrorKind, is_constraint};
use flyff_common::CharacterId;
use rusqlite::{Connection, params};

/// `SetPartyName`: stores `name` for `character` and replaces the previous name of that character. `PartyNameTaken`
/// when another character owns the name (case-insensitively); the game checks its own index first, so this only
/// fires for a bug or a race with another writer.
pub fn set(conn: &Connection, character: CharacterId, name: &str) -> Result<(), DbError> {
    conn.prepare_cached(
        "INSERT INTO party_names (character_id, name) VALUES (?1, ?2) \
         ON CONFLICT(character_id) DO UPDATE SET name = excluded.name",
    )?
    .execute(params![character.0, name])
    .map(|_| ())
    .map_err(|e| {
        if is_constraint(&e, "UNIQUE", "party_names.name") {
            DbError::new(
                DbErrorKind::PartyNameTaken,
                format!("party name '{name}' is taken"),
            )
        } else {
            e.into()
        }
    })
}

/// `DeletePartyName`: forgets the stored name of `character` (no row is fine).
pub fn delete(conn: &Connection, character: CharacterId) -> Result<(), DbError> {
    conn.prepare_cached("DELETE FROM party_names WHERE character_id = ?1")?
        .execute(params![character.0])?;
    Ok(())
}
