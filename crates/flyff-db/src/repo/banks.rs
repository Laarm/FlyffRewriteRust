//! `banks` writes (Plan/13 §5.6): the row created with a character, the PIN op and the bank writes of a
//! snapshot.
//!
//! All banks of the account's active characters are loaded at enter-world and written by every snapshot
//! (race-free: one character per account is online, and it happens inside the snapshot's transaction).
//! The PIN is never part of a snapshot; it changes only through `SetBankPin`.

use crate::error::{DbError, DbErrorKind};
use crate::model::{BankDoc, BankSnapshot, JsonDoc};
use flyff_common::{AccountId, CharacterId};
use rusqlite::{Connection, params};

/// The four ASCII digits of a PIN as column text; anything else is a programming error.
pub(crate) fn pin_text(pin: [u8; 4]) -> Result<String, DbError> {
    if pin.iter().all(u8::is_ascii_digit) {
        Ok(pin.iter().map(|&b| char::from(b)).collect())
    } else {
        Err(DbError::new(
            DbErrorKind::Bug,
            "bank PIN must be four ASCII digits",
        ))
    }
}

/// Inserts the bank row of a new character.
pub(crate) fn insert(
    conn: &Connection,
    id: CharacterId,
    pin: [u8; 4],
    gold: i64,
    doc: &BankDoc,
) -> Result<(), DbError> {
    conn.prepare_cached(
        "INSERT INTO banks (character_id, pin, gold, container) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![id.0, pin_text(pin)?, gold, doc.to_json()?])?;
    Ok(())
}

/// `SetBankPin`.
pub fn set_pin(conn: &Connection, id: CharacterId, pin: [u8; 4]) -> Result<(), DbError> {
    let n = conn
        .prepare_cached("UPDATE banks SET pin = ?2 WHERE character_id = ?1")?
        .execute(params![id.0, pin_text(pin)?])?;
    if n == 0 {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("character {id} has no bank"),
        ));
    }
    Ok(())
}

/// The bank part of a snapshot: `UPDATE banks SET gold, container WHERE character_id = ? AND the character
/// belongs to the snapshot's account and is active` (Plan/13 §16.2). Banks of characters that are no longer
/// active (sealed/deleted in the meantime) are skipped silently. Returns the number of rows written.
pub(crate) fn write_snapshot_banks(
    conn: &Connection,
    account: AccountId,
    banks: &[BankSnapshot],
) -> Result<usize, DbError> {
    let mut written = 0;
    for bank in banks {
        written += conn
            .prepare_cached(
                "UPDATE banks SET gold = ?2, container = ?3
                 WHERE character_id = ?1
                   AND character_id IN (SELECT id FROM characters WHERE account_id = ?4 AND state = 0)",
            )?
            .execute(params![bank.character_id.0, bank.gold, bank.doc.to_json()?, account.0])?;
    }
    Ok(written)
}
