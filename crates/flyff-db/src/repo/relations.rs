//! SQL of the couple and campus group (migration `0004`, Plan/13 §9, §16.3, §16.9; rules in Plan/21 §11-§12).
//!
//! Failure mapping (Plan/13 §16.6): an unknown character or campus (the `FOREIGN KEY`s) is `NotFound`; a character
//! that is already in a couple or another campus, a second campus master and a couple id that is used by other
//! members are `Conflict`; the same character twice in one couple and a negative couple experience are `Invalid`.
//!
//! [`set_campus_points`] writes `campus_points` and nothing else: the points of an offline master change without
//! touching `characters` or `character_docs` (the FIX of the legacy race, ref/05 R9).

use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{
    CampusLoad, CampusMemberRecord, CampusRole, CoupleProposalRecord, CoupleRecord,
};
use crate::repo::characters::Cols;
use flyff_common::{CharacterId, UnixTime};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeMap;

fn invalid(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Invalid, what)
}

fn conflict(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Conflict, what)
}

fn bug(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Bug, what)
}

fn map_write_error(e: rusqlite::Error, what: &str) -> DbError {
    if is_constraint(&e, "FOREIGN KEY", "") {
        DbError::new(
            DbErrorKind::NotFound,
            format!("{what}: a referenced character, couple or campus does not exist"),
        )
    } else if is_constraint(&e, "UNIQUE", "") {
        conflict(format!(
            "{what}: the character is already a member, or the campus already has a master"
        ))
    } else {
        e.into()
    }
}

// ---- couples ----------------------------------------------------------------------------------------------------

/// `UpsertCouple`: inserts the couple and both member rows, or - when the id exists - updates the experience only
/// (the members must be the stored ones, else `Conflict`). A character that is already coupled is `Conflict`
/// (`couple_members.character_id` is the primary key), the same character twice is `Invalid`, an unknown character
/// `NotFound`.
pub fn upsert_couple(conn: &Connection, c: &CoupleRecord) -> Result<(), DbError> {
    let [first, second] = c.members;
    if first == second {
        return Err(invalid(format!(
            "UpsertCouple {}: a character cannot be coupled with itself",
            c.id
        )));
    }
    if c.experience < 0 {
        return Err(invalid(format!(
            "UpsertCouple {}: negative experience {}",
            c.id, c.experience
        )));
    }
    let exists: bool = conn
        .prepare_cached("SELECT 1 FROM couples WHERE id = ?1")?
        .query_row([c.id], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if exists {
        let mut stored: Vec<u32> = Vec::new();
        {
            let mut stmt = conn.prepare_cached(
                "SELECT character_id FROM couple_members WHERE couple_id = ?1 ORDER BY character_id",
            )?;
            let mut rows = stmt.query([c.id])?;
            while let Some(row) = rows.next()? {
                stored.push(row.get(0)?);
            }
        }
        let mut wanted = [first.0, second.0];
        wanted.sort_unstable();
        if stored != wanted {
            return Err(conflict(format!(
                "UpsertCouple {}: the couple has other members ({stored:?}) than {wanted:?}",
                c.id
            )));
        }
        conn.prepare_cached("UPDATE couples SET experience = ?2 WHERE id = ?1")?
            .execute(params![c.id, c.experience])?;
        return Ok(());
    }
    conn.prepare_cached("INSERT INTO couples (id, experience, created_at) VALUES (?1, ?2, ?3)")?
        .execute(params![c.id, c.experience, c.created_at.0])
        .map_err(|e| map_write_error(e, &format!("UpsertCouple {}", c.id)))?;
    let mut member = conn
        .prepare_cached("INSERT INTO couple_members (character_id, couple_id) VALUES (?1, ?2)")?;
    for who in [first, second] {
        member
            .execute(params![who.0, c.id])
            .map_err(|e| map_write_error(e, &format!("UpsertCouple {} member {who}", c.id)))?;
    }
    Ok(())
}

/// `DeleteCouple`: the couple and its member rows (cascade); a missing couple is not an error. Returns whether a
/// row was deleted.
pub fn delete_couple(conn: &Connection, id: u32) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM couples WHERE id = ?1")?
        .execute([id])?
        > 0)
}

/// `SetCoupleProposal`: stores the proposer's cooldown (`None` deletes the row; with a 0 s cooldown no row exists).
pub fn set_couple_proposal(
    conn: &Connection,
    proposer: CharacterId,
    cooldown_until: Option<UnixTime>,
) -> Result<(), DbError> {
    match cooldown_until {
        Some(until) => conn
            .prepare_cached(
                "INSERT INTO couple_proposals (proposer_id, cooldown_until) VALUES (?1, ?2)
                 ON CONFLICT(proposer_id) DO UPDATE SET cooldown_until = excluded.cooldown_until",
            )?
            .execute(params![proposer.0, until.0])
            .map(|_| ())
            .map_err(|e| map_write_error(e, &format!("SetCoupleProposal {proposer}"))),
        None => conn
            .prepare_cached("DELETE FROM couple_proposals WHERE proposer_id = ?1")?
            .execute([proposer.0])
            .map(|_| ())
            .map_err(DbError::from),
    }
}

// ---- campuses ---------------------------------------------------------------------------------------------------

/// `UpsertCampus`: creates the campus; an existing one is left as it is (insert or ignore).
pub fn upsert_campus(conn: &Connection, id: u32, created_at: UnixTime) -> Result<(), DbError> {
    conn.prepare_cached("INSERT OR IGNORE INTO campuses (id, created_at) VALUES (?1, ?2)")?
        .execute(params![id, created_at.0])?;
    Ok(())
}

/// `DeleteCampus`: the campus and its member rows (cascade); a missing campus is not an error. Returns whether a row
/// was deleted.
pub fn delete_campus(conn: &Connection, id: u32) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM campuses WHERE id = ?1")?
        .execute([id])?
        > 0)
}

/// `UpsertCampusMember`: adds a member or changes the role of a member of the **same** campus. A character in
/// another campus is `Conflict` (the update only applies `WHERE campus_id = excluded.campus_id`), a second master
/// is `Conflict` (partial unique index), an unknown campus or character `NotFound`.
pub fn upsert_campus_member(conn: &Connection, m: &CampusMemberRecord) -> Result<(), DbError> {
    let changed = conn
        .prepare_cached(
            "INSERT INTO campus_members (character_id, campus_id, role, joined_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(character_id) DO UPDATE SET role = excluded.role
             WHERE campus_members.campus_id = excluded.campus_id",
        )?
        .execute(params![
            m.character_id.0,
            m.campus_id,
            m.role.code(),
            m.joined_at.0
        ])
        .map_err(|e| map_write_error(e, &format!("UpsertCampusMember {}", m.character_id)))?;
    if changed == 0 {
        return Err(conflict(format!(
            "UpsertCampusMember: character {} belongs to another campus than {}",
            m.character_id, m.campus_id
        )));
    }
    Ok(())
}

/// `DeleteCampusMember`: removes the character from its campus (idempotent). Returns whether a row was deleted.
pub fn delete_campus_member(conn: &Connection, character: CharacterId) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM campus_members WHERE character_id = ?1")?
        .execute([character.0])?
        > 0)
}

/// `SetCampusPoints`: the point balance of a character (may be negative). Writes `campus_points` only - never
/// `characters` or `character_docs` - so the master of a pupil who levels up can be rewarded while he is offline.
/// `NotFound` for an unknown character.
pub fn set_campus_points(
    conn: &Connection,
    character: CharacterId,
    points: i32,
) -> Result<(), DbError> {
    conn.prepare_cached(
        "INSERT INTO campus_points (character_id, points) VALUES (?1, ?2)
         ON CONFLICT(character_id) DO UPDATE SET points = excluded.points",
    )?
    .execute(params![character.0, points])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("SetCampusPoints {character}")))
}

// ---- reads: the startup load ------------------------------------------------------------------------------------

/// Every couple with exactly two members, ordered by id (`StartupState.couples`), `members = [higher id, lower id]`.
/// A couple with another member count is skipped with a warning.
pub fn load_couples(conn: &Connection) -> Result<Vec<CoupleRecord>, DbError> {
    let mut members: BTreeMap<u32, Vec<CharacterId>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT couple_id, character_id FROM couple_members ORDER BY couple_id, character_id DESC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["couple_id", "character_id"], 0);
            let couple: u32 = c.int()?;
            members
                .entry(couple)
                .or_default()
                .push(CharacterId(c.int()?));
        }
    }
    let mut out = Vec::new();
    let mut stmt =
        conn.prepare_cached("SELECT id, experience, created_at FROM couples ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let mut c = Cols::new(row, &["id", "experience", "created_at"], 0);
        let id: u32 = c.int()?;
        let experience = c.int()?;
        let created_at = UnixTime(c.int()?);
        match members.remove(&id).as_deref() {
            Some(&[first, second]) => out.push(CoupleRecord {
                id,
                experience,
                created_at,
                members: [first, second],
            }),
            other => tracing::warn!(
                couple = id,
                members = other.map_or(0, <[CharacterId]>::len),
                "couple skipped: it does not have exactly two members"
            ),
        }
    }
    Ok(out)
}

/// Every stored proposal cooldown, ordered by proposer.
pub fn load_couple_proposals(conn: &Connection) -> Result<Vec<CoupleProposalRecord>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT proposer_id, cooldown_until FROM couple_proposals ORDER BY proposer_id",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut c = Cols::new(row, &["proposer_id", "cooldown_until"], 0);
        out.push(CoupleProposalRecord {
            proposer_id: CharacterId(c.int()?),
            cooldown_until: UnixTime(c.int()?),
        });
    }
    Ok(out)
}

/// Every campus with its members (master first, then by join time), ordered by id. A campus without members is
/// loaded empty with a warning (the `CampusService` dissolves it).
pub fn load_campuses(conn: &Connection) -> Result<Vec<CampusLoad>, DbError> {
    let mut members: BTreeMap<u32, Vec<CampusMemberRecord>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT campus_id, character_id, role, joined_at FROM campus_members
             ORDER BY campus_id, role, joined_at, character_id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["campus_id", "character_id", "role", "joined_at"], 0);
            let campus_id: u32 = c.int()?;
            let character_id = CharacterId(c.int()?);
            let role_code: u8 = c.int()?;
            let role = CampusRole::from_code(role_code)
                .ok_or_else(|| bug(format!("campus_members.role {role_code} of {character_id}")))?;
            members
                .entry(campus_id)
                .or_default()
                .push(CampusMemberRecord {
                    character_id,
                    campus_id,
                    role,
                    joined_at: UnixTime(c.int()?),
                });
        }
    }
    let mut out = Vec::new();
    let mut stmt = conn.prepare_cached("SELECT id, created_at FROM campuses ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let mut c = Cols::new(row, &["id", "created_at"], 0);
        let id: u32 = c.int()?;
        let created_at = UnixTime(c.int()?);
        let members = members.remove(&id).unwrap_or_default();
        if members.is_empty() {
            tracing::warn!(campus = id, "campus without members");
        }
        out.push(CampusLoad {
            id,
            created_at,
            members,
        });
    }
    Ok(out)
}

/// Every stored campus point balance, ordered by character.
pub fn load_campus_points(conn: &Connection) -> Result<Vec<(CharacterId, i32)>, DbError> {
    let mut stmt = conn
        .prepare_cached("SELECT character_id, points FROM campus_points ORDER BY character_id")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut c = Cols::new(row, &["character_id", "points"], 0);
        out.push((CharacterId(c.int()?), c.int()?));
    }
    Ok(out)
}
