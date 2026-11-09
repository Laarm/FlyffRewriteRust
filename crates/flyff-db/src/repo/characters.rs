//! `characters` writes and the column mapping of [`CharacterRow`] (Plan/13 §5.3-§5.4, §14.2, §16.3):
//! `CreateCharacter`, `SoftDeleteCharacter`, `RenameCharacter`, `SetCharacterLoginBlock`,
//! `SetCharacterAuthority`, `SealCharacter`, `UnsealCharacter`, `PurgeCharacters`.
//!
//! Column ownership (13 §4): a snapshot writes the snapshot-owned columns ([`CharacterRow::SNAPSHOT_COLUMNS`])
//! and never an identity column; the ops here write the identity columns (and the initial snapshot columns
//! on creation) and never touch a snapshot column afterwards.

use super::accounts::authority_text;
use super::banks;
use super::meta::{KEY_NEXT_CHARACTER_ID, take_id};
use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{CharacterRow, EQUIPMENT_PREVIEW_LEN, NewCharacterRecord};
use flyff_common::{AccountId, CharacterId, UnixTime};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::sync::LazyLock;

/// Highest valid `characters.slot`.
pub const MAX_SLOT: u8 = 2;

fn int(v: impl Into<i64>) -> Value {
    Value::Integer(v.into())
}

fn real(x: f32, fallback: f32) -> Value {
    Value::Real(f64::from(if x.is_finite() { x } else { fallback }))
}

fn opt_int(v: Option<impl Into<i64>>) -> Value {
    v.map_or(Value::Null, |v| Value::Integer(v.into()))
}

/// The values of the snapshot-owned columns in [`CharacterRow::SNAPSHOT_COLUMNS`] order. A snapshot write
/// never fails because of game values (Plan/13 §16.6), so non-finite floats are replaced by `0.0` (`scale` by
/// `1.0`) instead of reaching a `NOT NULL` column as `NULL`.
pub(crate) fn row_values(row: &CharacterRow) -> Result<Vec<Value>, DbError> {
    let preview = serde_json::to_string(&row.equipment_preview)?;
    Ok(vec![
        int(row.sex),
        int(row.mover_index),
        int(row.skin_set),
        int(row.hair_mesh),
        int(row.hair_color),
        int(row.head_mesh),
        int(row.hide_coat_mask),
        real(row.scale, 1.0),
        int(row.world_id),
        int(row.layer),
        real(row.pos[0], 0.0),
        real(row.pos[1], 0.0),
        real(row.pos[2], 0.0),
        int(row.return_world_id),
        real(row.return_pos[0], 0.0),
        real(row.return_pos[1], 0.0),
        real(row.return_pos[2], 0.0),
        int(row.marking_world_id),
        real(row.marking_pos[0], 0.0),
        real(row.marking_pos[1], 0.0),
        real(row.marking_pos[2], 0.0),
        int(row.motion),
        int(row.act_state),
        int(row.act_state_flags),
        int(row.hp),
        int(row.mp),
        int(row.fp),
        int(row.job),
        int(row.level),
        int(row.max_level),
        int(row.exp),
        int(row.stat_str),
        int(row.stat_sta),
        int(row.stat_dex),
        int(row.stat_int),
        int(row.stat_points),
        int(row.skill_points),
        int(row.skill_level_total),
        int(row.gold),
        int(row.flight_exp),
        int(row.tutorial_state),
        int(row.fuel),
        int(row.fuel_time_acc),
        int(row.mode_flags),
        int(row.fame),
        int(row.pk_value),
        int(row.pk_propensity),
        int(row.pk_exp),
        int(row.death_exp),
        int(row.death_level),
        int(row.angel_exp),
        int(row.angel_level),
        int(row.mute_remaining_s),
        int(row.messenger_state),
        int(row.play_time_s),
        int(row.attendance_bits),
        int(row.attendance_minutes),
        int(row.attendance_elapsed),
        int(row.coupon_count),
        opt_int(row.honor_title_id),
        opt_int(row.summoned_pet_obj_id),
        int(row.rest_points),
        int(row.last_logout_at.0),
        Value::Text(preview),
    ])
}

/// Sequential reader over the columns of a result row (error messages name the column).
pub(crate) struct Cols<'r, 'a> {
    row: &'r Row<'a>,
    names: &'static [&'static str],
    start: usize,
    next: usize,
}

impl<'r, 'a> Cols<'r, 'a> {
    /// Reads `row` starting at column `start`; `names` labels the columns for error messages.
    pub(crate) fn new(row: &'r Row<'a>, names: &'static [&'static str], start: usize) -> Self {
        Self {
            row,
            names,
            start,
            next: start,
        }
    }

    fn label(&self, index: usize) -> &'static str {
        self.names
            .get(index.saturating_sub(self.start))
            .copied()
            .unwrap_or("?")
    }

    fn take<T: rusqlite::types::FromSql>(&mut self) -> Result<T, DbError> {
        let index = self.next;
        self.next += 1;
        self.row.get::<_, T>(index).map_err(|e| {
            DbError::new(
                DbErrorKind::Bug,
                format!("column {} ({index}): {e}", self.label(index)),
            )
        })
    }

    /// The next integer column converted to `T`; out-of-range stored values are an error.
    pub(crate) fn int<T: TryFrom<i64>>(&mut self) -> Result<T, DbError> {
        let index = self.next;
        let v: i64 = self.take()?;
        T::try_from(v).map_err(|_| {
            DbError::new(
                DbErrorKind::Bug,
                format!(
                    "column {} ({index}): value {v} out of range",
                    self.label(index)
                ),
            )
        })
    }

    /// A nullable integer column.
    pub(crate) fn opt_int<T: TryFrom<i64>>(&mut self) -> Result<Option<T>, DbError> {
        let index = self.next;
        let v: Option<i64> = self.take()?;
        v.map(|v| {
            T::try_from(v).map_err(|_| {
                DbError::new(
                    DbErrorKind::Bug,
                    format!(
                        "column {} ({index}): value {v} out of range",
                        self.label(index)
                    ),
                )
            })
        })
        .transpose()
    }

    /// A `REAL` column as `f32`.
    pub(crate) fn f32(&mut self) -> Result<f32, DbError> {
        let v: f64 = self.take()?;
        // Stored values were widened from f32 and round-trip exactly.
        Ok(v as f32)
    }

    /// A `TEXT` column.
    pub(crate) fn text(&mut self) -> Result<String, DbError> {
        self.take()
    }

    /// A nullable `TEXT` column.
    pub(crate) fn opt_text(&mut self) -> Result<Option<String>, DbError> {
        self.take()
    }
}

/// Decodes `equipment_preview` (`[u32; 31]` as JSON). A malformed value yields zeros and a warning:
/// the character list must never fail because of a cosmetic column.
pub(crate) fn decode_preview(text: &str, id: CharacterId) -> [u32; EQUIPMENT_PREVIEW_LEN] {
    match serde_json::from_str::<Vec<u32>>(text) {
        Ok(v) if v.len() == EQUIPMENT_PREVIEW_LEN => {
            let mut out = [0u32; EQUIPMENT_PREVIEW_LEN];
            for (slot, value) in out.iter_mut().zip(v) {
                *slot = value;
            }
            out
        }
        _ => {
            tracing::warn!(
                character = id.0,
                "equipment_preview is malformed: using an empty one"
            );
            [0; EQUIPMENT_PREVIEW_LEN]
        }
    }
}

/// Reads the snapshot-owned columns in [`CharacterRow::SNAPSHOT_COLUMNS`] order from `cols`.
pub(crate) fn read_row(cols: &mut Cols<'_, '_>, id: CharacterId) -> Result<CharacterRow, DbError> {
    let sex = cols.int()?;
    let mover_index = cols.int()?;
    let skin_set = cols.int()?;
    let hair_mesh = cols.int()?;
    let hair_color = cols.int()?;
    let head_mesh = cols.int()?;
    let hide_coat_mask = cols.int()?;
    let scale = cols.f32()?;
    let world_id = cols.int()?;
    let layer = cols.int()?;
    let pos = [cols.f32()?, cols.f32()?, cols.f32()?];
    let return_world_id = cols.int()?;
    let return_pos = [cols.f32()?, cols.f32()?, cols.f32()?];
    let marking_world_id = cols.int()?;
    let marking_pos = [cols.f32()?, cols.f32()?, cols.f32()?];
    let motion = cols.int()?;
    let act_state = cols.int()?;
    let act_state_flags = cols.int()?;
    let hp = cols.int()?;
    let mp = cols.int()?;
    let fp = cols.int()?;
    let job = cols.int()?;
    let level = cols.int()?;
    let max_level = cols.int()?;
    let exp = cols.int()?;
    let stat_str = cols.int()?;
    let stat_sta = cols.int()?;
    let stat_dex = cols.int()?;
    let stat_int = cols.int()?;
    let stat_points = cols.int()?;
    let skill_points = cols.int()?;
    let skill_level_total = cols.int()?;
    let gold = cols.int()?;
    let flight_exp = cols.int()?;
    let tutorial_state = cols.int()?;
    let fuel = cols.int()?;
    let fuel_time_acc = cols.int()?;
    let mode_flags = cols.int()?;
    let fame = cols.int()?;
    let pk_value = cols.int()?;
    let pk_propensity = cols.int()?;
    let pk_exp = cols.int()?;
    let death_exp = cols.int()?;
    let death_level = cols.int()?;
    let angel_exp = cols.int()?;
    let angel_level = cols.int()?;
    let mute_remaining_s = cols.int()?;
    let messenger_state = cols.int()?;
    let play_time_s = cols.int()?;
    let attendance_bits = cols.int()?;
    let attendance_minutes = cols.int()?;
    let attendance_elapsed = cols.int()?;
    let coupon_count = cols.int()?;
    let honor_title_id = cols.opt_int()?;
    let summoned_pet_obj_id = cols.opt_int()?;
    let rest_points = cols.int()?;
    let last_logout_at = UnixTime(cols.int()?);
    let equipment_preview = decode_preview(&cols.text()?, id);
    Ok(CharacterRow {
        sex,
        mover_index,
        skin_set,
        hair_mesh,
        hair_color,
        head_mesh,
        hide_coat_mask,
        scale,
        world_id,
        layer,
        pos,
        return_world_id,
        return_pos,
        marking_world_id,
        marking_pos,
        motion,
        act_state,
        act_state_flags,
        hp,
        mp,
        fp,
        job,
        level,
        max_level,
        exp,
        stat_str,
        stat_sta,
        stat_dex,
        stat_int,
        stat_points,
        skill_points,
        skill_level_total,
        gold,
        flight_exp,
        tutorial_state,
        fuel,
        fuel_time_acc,
        mode_flags,
        fame,
        pk_value,
        pk_propensity,
        pk_exp,
        death_exp,
        death_level,
        angel_exp,
        angel_level,
        mute_remaining_s,
        messenger_state,
        play_time_s,
        attendance_bits,
        attendance_minutes,
        attendance_elapsed,
        coupon_count,
        honor_title_id,
        summoned_pet_obj_id,
        rest_points,
        last_logout_at,
        equipment_preview,
    })
}

/// The snapshot `UPDATE`: every snapshot-owned column, `max_level = MAX(max_level, ?)`, `saved_at`, guarded
/// by `id`, `account_id` and `state = 0` (Plan/13 §16.2).
pub(crate) static SNAPSHOT_UPDATE_SQL: LazyLock<String> = LazyLock::new(|| {
    let sets: Vec<String> = CharacterRow::SNAPSHOT_COLUMNS
        .iter()
        .map(|c| {
            if *c == "max_level" {
                "max_level = MAX(max_level, ?)".to_owned()
            } else {
                format!("{c} = ?")
            }
        })
        .collect();
    format!(
        "UPDATE characters SET {}, saved_at = ? WHERE id = ? AND account_id = ? AND state = 0",
        sets.join(", ")
    )
});

static INSERT_SQL: LazyLock<String> = LazyLock::new(|| {
    let cols = CharacterRow::SNAPSHOT_COLUMNS.join(", ");
    let marks = vec!["?"; CharacterRow::SNAPSHOT_COLUMNS.len() + 6].join(", ");
    format!(
        "INSERT INTO characters (id, account_id, slot, name, created_at, {cols}, saved_at) \
         VALUES ({marks})"
    )
});

fn map_name_slot_violation(
    e: rusqlite::Error,
    name: &str,
    account: AccountId,
    slot: u8,
) -> DbError {
    if is_constraint(&e, "UNIQUE", "characters.name") {
        DbError::new(
            DbErrorKind::NameTaken,
            format!("character name '{name}' is taken"),
        )
    } else if is_constraint(&e, "UNIQUE", "characters.account_id") {
        DbError::new(
            DbErrorKind::SlotTaken,
            format!("slot {slot} of account {account} is taken"),
        )
    } else if is_constraint(&e, "FOREIGN KEY", "") {
        DbError::new(
            DbErrorKind::NotFound,
            format!("account {account} does not exist"),
        )
    } else {
        e.into()
    }
}

fn name_taken(conn: &Connection, name: &str, except: Option<CharacterId>) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM characters WHERE name = ?1 AND state <> 1 AND id <> ?2)",
        )?
        .query_row(params![name, except.map_or(-1, |c| i64::from(c.0))], |r| {
            r.get(0)
        })?)
}

fn slot_taken(conn: &Connection, account: AccountId, slot: u8) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM characters
                            WHERE account_id = ?1 AND slot = ?2 AND state = 0)",
        )?
        .query_row(params![account.0, slot], |r| r.get(0))?)
}

/// `CreateCharacter`: allocates the id from `meta.next_character_id` inside the transaction and inserts the
/// `characters`, `character_docs` and `banks` rows. `NameTaken` / `SlotTaken` / `NotFound` (no such account).
pub fn create_character(
    conn: &Connection,
    rec: &NewCharacterRecord,
) -> Result<CharacterId, DbError> {
    if rec.slot > MAX_SLOT {
        return Err(DbError::new(
            DbErrorKind::SlotTaken,
            format!("slot {} is outside 0..={MAX_SLOT}", rec.slot),
        ));
    }
    if name_taken(conn, &rec.name, None)? {
        return Err(DbError::new(
            DbErrorKind::NameTaken,
            format!("character name '{}' is taken", rec.name),
        ));
    }
    if slot_taken(conn, rec.account_id, rec.slot)? {
        return Err(DbError::new(
            DbErrorKind::SlotTaken,
            format!("slot {} of account {} is taken", rec.slot, rec.account_id),
        ));
    }
    let raw_id = take_id(conn, KEY_NEXT_CHARACTER_ID)?;
    let id = u32::try_from(raw_id).map(CharacterId).map_err(|_| {
        DbError::new(
            DbErrorKind::Bug,
            format!("character id {raw_id} exceeds u32"),
        )
    })?;

    let mut values: Vec<Value> = vec![
        int(id.0),
        int(rec.account_id.0),
        int(rec.slot),
        Value::Text(rec.name.clone()),
        int(rec.created_at.0),
    ];
    values.extend(row_values(&rec.row)?);
    values.push(int(rec.created_at.0));
    conn.prepare_cached(&INSERT_SQL)?
        .execute(rusqlite::params_from_iter(values))
        .map_err(|e| map_name_slot_violation(e, &rec.name, rec.account_id, rec.slot))?;

    let docs = rec.docs.to_json_columns()?;
    conn.prepare_cached(
        "INSERT INTO character_docs (character_id, inventory, pockets, skills, quests, taskbar,
                                     buffs, sm_timers, honor, settings)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?
    .execute(params![
        id.0, docs[0], docs[1], docs[2], docs[3], docs[4], docs[5], docs[6], docs[7], docs[8]
    ])?;
    banks::insert(conn, id, rec.bank_pin, 0, &rec.bank)?;
    Ok(id)
}

fn character_state(conn: &Connection, id: CharacterId) -> Result<Option<i64>, DbError> {
    Ok(conn
        .prepare_cached("SELECT state FROM characters WHERE id = ?1")?
        .query_row([id.0], |r| r.get(0))
        .optional()?)
}

fn not_found(id: CharacterId) -> DbError {
    DbError::new(
        DbErrorKind::NotFound,
        format!("character {id} does not exist"),
    )
}

/// `SoftDeleteCharacter`: `state = 1, deleted_at = at`. The name and the slot are free from this moment on
/// (the unique indexes exclude deleted rows; FIX PL-057). `NotFound` for a missing or already deleted
/// character, `Conflict` for a sealed one.
pub fn soft_delete(conn: &Connection, id: CharacterId, at: UnixTime) -> Result<(), DbError> {
    match character_state(conn, id)? {
        None | Some(1) => Err(not_found(id)),
        Some(2) => Err(DbError::new(
            DbErrorKind::Conflict,
            format!("character {id} is sealed and cannot be deleted"),
        )),
        Some(_) => {
            conn.prepare_cached(
                "UPDATE characters SET state = 1, deleted_at = ?2 WHERE id = ?1 AND state = 0",
            )?
            .execute(params![id.0, at.0])?;
            Ok(())
        }
    }
}

/// `RenameCharacter`: `NameTaken` when another non-deleted character has the name (case-insensitively).
pub fn rename(conn: &Connection, id: CharacterId, name: &str) -> Result<(), DbError> {
    if name_taken(conn, name, Some(id))? {
        return Err(DbError::new(
            DbErrorKind::NameTaken,
            format!("character name '{name}' is taken"),
        ));
    }
    let n = conn
        .prepare_cached("UPDATE characters SET name = ?2 WHERE id = ?1 AND state <> 1")?
        .execute(params![id.0, name])
        .map_err(|e| {
            if is_constraint(&e, "UNIQUE", "characters.name") {
                DbError::new(
                    DbErrorKind::NameTaken,
                    format!("character name '{name}' is taken"),
                )
            } else {
                e.into()
            }
        })?;
    if n == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// `SetCharacterPosition` (Plan/25 §11.6, only from the game thread under an admin hold): moves an **active**
/// character to `pos` in world `world_id`, layer 0. A snapshot-owned column set written by an op, safe because
/// the account is held offline by the console while the op is queued (a snapshot of the character cannot be
/// queued after it). `NotFound` for a missing or deleted character, `Conflict` for a sealed one, `Invalid` for a
/// non-finite coordinate.
pub fn set_position(
    conn: &Connection,
    id: CharacterId,
    world_id: u32,
    pos: [f32; 3],
) -> Result<(), DbError> {
    if pos.iter().any(|c| !c.is_finite()) {
        return Err(DbError::new(
            DbErrorKind::Invalid,
            format!("position {pos:?} is not finite"),
        ));
    }
    let n = conn
        .prepare_cached(
            "UPDATE characters SET world_id = ?2, layer = 0, pos_x = ?3, pos_y = ?4, pos_z = ?5
             WHERE id = ?1 AND state = 0",
        )?
        .execute(params![
            id.0,
            world_id,
            f64::from(pos[0]),
            f64::from(pos[1]),
            f64::from(pos[2])
        ])?;
    if n == 1 {
        return Ok(());
    }
    match character_state(conn, id)? {
        Some(2) => Err(DbError::new(
            DbErrorKind::Conflict,
            format!("character {id} is sealed"),
        )),
        _ => Err(not_found(id)),
    }
}

/// `UndeleteCharacter` (Plan/25 §4.6): `state = 1` -> `0` in `slot` of the owning account. The checks are the
/// CLI's (`flyff character undelete`, [`crate::maintenance::undelete_character`]): `NotFound` (missing, e.g.
/// already purged), `Conflict` (not deleted), `NameTaken` (a live character uses the name now), `SlotTaken`
/// (occupied or outside `0..=2`). Friends, guild and campus memberships removed at deletion are not restored.
pub fn undelete(conn: &Connection, id: CharacterId, slot: u8) -> Result<(), DbError> {
    if slot > MAX_SLOT {
        return Err(DbError::new(
            DbErrorKind::SlotTaken,
            format!("slot {slot} does not exist (slots are 0, 1 and 2)"),
        ));
    }
    let row: Option<(u32, String, i64)> = conn
        .prepare_cached("SELECT account_id, name, state FROM characters WHERE id = ?1")?
        .query_row([id.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((account, name, state)) = row else {
        return Err(not_found(id));
    };
    match state {
        1 => {}
        2 => {
            return Err(DbError::new(
                DbErrorKind::Conflict,
                format!("character {id} is sealed, not deleted"),
            ));
        }
        _ => {
            return Err(DbError::new(
                DbErrorKind::Conflict,
                format!("character {id} is not deleted"),
            ));
        }
    }
    let account = AccountId(account);
    if name_taken(conn, &name, Some(id))? {
        return Err(DbError::new(
            DbErrorKind::NameTaken,
            format!(
                "the name '{name}' is used by another character now: rename that character first"
            ),
        ));
    }
    if slot_taken(conn, account, slot)? {
        return Err(DbError::new(
            DbErrorKind::SlotTaken,
            format!("slot {slot} of account {account} is occupied"),
        ));
    }
    let n = conn
        .prepare_cached(
            "UPDATE characters SET state = 0, deleted_at = NULL, slot = ?2 WHERE id = ?1 AND state = 1",
        )?
        .execute(params![id.0, slot])
        .map_err(|e| map_name_slot_violation(e, &name, account, slot))?;
    if n == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// `SetCharacterLoginBlock`.
pub fn set_login_block(
    conn: &Connection,
    id: CharacterId,
    until: Option<UnixTime>,
) -> Result<(), DbError> {
    let n = conn
        .prepare_cached(
            "UPDATE characters SET login_blocked_until = ?2 WHERE id = ?1 AND state <> 1",
        )?
        .execute(params![id.0, until.map(|t| t.0)])?;
    if n == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// `SetCharacterAuthority`.
pub fn set_authority(conn: &Connection, id: CharacterId, authority: u8) -> Result<(), DbError> {
    let letter = authority_text(authority)?;
    let n = conn
        .prepare_cached("UPDATE characters SET authority = ?2 WHERE id = ?1 AND state <> 1")?
        .execute(params![id.0, letter])?;
    if n == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// `SealCharacter`: active -> sealed. The character keeps its name and occupies no slot.
pub fn seal(conn: &Connection, id: CharacterId, at: UnixTime) -> Result<(), DbError> {
    match character_state(conn, id)? {
        None | Some(1) => Err(not_found(id)),
        Some(0) => {
            conn.prepare_cached(
                "UPDATE characters SET state = 2, sealed_at = ?2 WHERE id = ?1 AND state = 0",
            )?
            .execute(params![id.0, at.0])?;
            Ok(())
        }
        Some(_) => Err(DbError::new(
            DbErrorKind::Conflict,
            format!("character {id} is already sealed"),
        )),
    }
}

/// `UnsealCharacter`: sealed -> active in `slot` of `account`. `SlotTaken` when the slot is occupied (or
/// outside `0..=2`), `NotFound` for a missing character, an unknown account or a character that is not sealed.
pub fn unseal(
    conn: &Connection,
    id: CharacterId,
    account: AccountId,
    slot: u8,
) -> Result<(), DbError> {
    if slot > MAX_SLOT || slot_taken(conn, account, slot)? {
        return Err(DbError::new(
            DbErrorKind::SlotTaken,
            format!("slot {slot} of account {account} is taken"),
        ));
    }
    let n = conn
        .prepare_cached(
            "UPDATE characters SET state = 0, sealed_at = NULL, account_id = ?2, slot = ?3
             WHERE id = ?1 AND state = 2",
        )?
        .execute(params![id.0, account.0, slot])
        .map_err(|e| map_name_slot_violation(e, "", account, slot))?;
    if n == 0 {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("character {id} is not sealed"),
        ));
    }
    Ok(())
}

/// `PurgeCharacters`: hard-deletes soft-deleted characters (`state = 1`); the foreign keys cascade to the
/// documents, the bank and the satellite tables. Couples and campuses left without members are removed in
/// the same transaction (Plan/13 §14.2). Ids are never handed out again (the `meta` counter only grows).
/// Returns the number of purged characters; ids that are missing or not deleted are skipped.
pub fn purge(conn: &Connection, ids: &[CharacterId]) -> Result<usize, DbError> {
    let mut purged = 0;
    for id in ids {
        let couples: Vec<i64> = conn
            .prepare_cached("SELECT couple_id FROM couple_members WHERE character_id = ?1")?
            .query_map([id.0], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let campuses: Vec<i64> = conn
            .prepare_cached("SELECT campus_id FROM campus_members WHERE character_id = ?1")?
            .query_map([id.0], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let n = conn
            .prepare_cached("DELETE FROM characters WHERE id = ?1 AND state = 1")?
            .execute([id.0])?;
        if n == 0 {
            continue;
        }
        purged += n;
        for couple in couples {
            conn.prepare_cached(
                "DELETE FROM couples WHERE id = ?1
                   AND (SELECT COUNT(*) FROM couple_members WHERE couple_id = ?1) < 2",
            )?
            .execute([couple])?;
        }
        for campus in campuses {
            conn.prepare_cached(
                "DELETE FROM campuses WHERE id = ?1
                   AND NOT EXISTS (SELECT 1 FROM campus_members WHERE campus_id = ?1)",
            )?
            .execute([campus])?;
        }
    }
    Ok(purged)
}
