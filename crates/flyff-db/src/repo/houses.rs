//! SQL of the guild-house, housing, rainbow-race and quiz group (migration `0007`, Plan/13 §12, §16.3, §16.9; rules in
//! Plan/22 §12-§15).
//!
//! **Furniture rows are items, entries are fees and prizes.** A list-up consumes an item from an inventory and adds a
//! row in the same batch, an application takes the fee and adds an entry, a result pays the prizes. If the row write
//! were allowed twice, the second batch would dupe the item or charge the fee twice. The ops therefore enforce the
//! guards below; an illegal edge is `Conflict`, which rolls back the **whole batch including its snapshots and
//! mails**.
//!
//! | row | rules |
//! |---|---|
//! | guild house | `created_at` is immutable: a buy over an existing house is `Conflict`; upkeep and expiry rewrite `upkeep_until` with the stored `created_at` |
//! | guild furniture | `guild_id`, `item_id`, `listed_at` are immutable per `seq` (a reused seq never overwrites another listing); setup, move and expiry rewrite the rest |
//! | housing furniture | `listed_at` and `keep_until` are immutable (the server's keep time wins, 22 §13.2); the list-up batch uses the strict `InsertHousingFurniture`, so an item the owner already lists is `Conflict` |
//! | rainbow entry | a new row only as `N`; `N -> R/F/L`; `R -> W/L/N`; `W`, `L`, `F` are terminal; `N -> N` is refused (a second application is `Conflict` and its fee rolls back) |
//! | quiz question | `validate()` first; `id == 0` is a natural-key upsert on `(quiz_type, question)`, `id > 0` updates that row |
//!
//! Failure mapping: an unknown guild or character is `NotFound` (the `FOREIGN KEY`), an illegal edge or a duplicate
//! `Conflict`, a limit `Invalid` (the second line of defence: the game validates first).

use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{
    GuildHouseFurnitureRecord, GuildHouseLoad, GuildHouseRecord, HousingFurnitureRecord,
    HousingLoad, QuizQuestionRecord, QuizType, RainbowEntryState, RainbowRaceEntryRecord,
    RainbowRaceLoad,
};
use crate::repo::characters::Cols;
use flyff_common::{CharacterId, GuildId, UnixTime, WorldId};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeSet;

fn conflict(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Conflict, what)
}

fn invalid(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Invalid, what)
}

fn bug(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Bug, what)
}

/// Maps the constraint failures of a write to the *Expected* kinds: a primary key clash is `Conflict`, a missing
/// parent row `NotFound`.
fn map_write_error(e: rusqlite::Error, what: &str) -> DbError {
    if is_constraint(&e, "FOREIGN KEY", "") {
        DbError::new(
            DbErrorKind::NotFound,
            format!("{what}: the guild or character does not exist"),
        )
    } else if is_constraint(&e, "UNIQUE", "") {
        conflict(format!("{what}: a row with that key exists"))
    } else {
        e.into()
    }
}

fn finite_placement(what: &str, pos: &[f32; 3], angle: f32) -> Result<(), DbError> {
    if pos.iter().all(|v| v.is_finite()) && angle.is_finite() {
        Ok(())
    } else {
        Err(invalid(format!(
            "{what}: the position {pos:?} or the angle {angle} is not finite"
        )))
    }
}

// ---- the guards (pure, shared by the ops and the tests) -----------------------------------------------------------

/// The guard of a rainbow entry row: may a row currently in `old` be rewritten as `new`? See the table in the module
/// docs. `Err` carries the reason (it becomes the `Conflict`).
pub fn check_rainbow_transition(
    old: Option<RainbowEntryState>,
    new: RainbowEntryState,
) -> Result<(), String> {
    use RainbowEntryState::{Applied, Failed, Finished, Lost, Winner};
    let Some(from) = old else {
        return if new == Applied {
            Ok(())
        } else {
            Err(format!(
                "a new entry starts as N (applied), not {}",
                new.code()
            ))
        };
    };
    match (from, new) {
        (Applied, Finished | Failed | Lost) | (Finished, Winner | Lost | Applied) => Ok(()),
        (Applied, Applied) => Err(
            "the character already applied in this round (a second fee would be lost)".to_owned(),
        ),
        (Winner | Lost | Failed, to) => Err(format!(
            "an entry in state {} is final and cannot become {}",
            from.code(),
            to.code()
        )),
        (from, to) => Err(format!(
            "an entry in state {} cannot become {}",
            from.code(),
            to.code()
        )),
    }
}

/// The guard of a guild furniture row: `guild_id`, `item_id` and `listed_at` of a stored `seq` never change.
pub fn check_guild_furniture_transition(
    old: Option<&GuildHouseFurnitureRecord>,
    new: &GuildHouseFurnitureRecord,
) -> Result<(), String> {
    match old {
        Some(old)
            if old.guild_id != new.guild_id
                || old.item_id != new.item_id
                || old.listed_at != new.listed_at =>
        {
            Err(format!(
                "seq {} belongs to guild {} item {} listed at {}: a reused seq never overwrites another listing",
                old.seq, old.guild_id, old.item_id, old.listed_at.0
            ))
        }
        _ => Ok(()),
    }
}

/// The guard of a housing furniture row: `listed_at` and `keep_until` of a stored piece never change.
pub fn check_housing_transition(
    old: Option<&HousingFurnitureRecord>,
    new: &HousingFurnitureRecord,
) -> Result<(), String> {
    match old {
        Some(old) if old.listed_at != new.listed_at || old.keep_until != new.keep_until => {
            Err("listed_at and keep_until never change (the server's keep time wins)".to_owned())
        }
        _ => Ok(()),
    }
}

/// The guard of a guild house row: `created_at` never changes (a buy over an existing house).
pub fn check_guild_house_transition(
    old: Option<&GuildHouseRecord>,
    new: &GuildHouseRecord,
) -> Result<(), String> {
    match old {
        Some(old) if old.created_at != new.created_at => Err(format!(
            "the guild already owns a house bought at {}",
            old.created_at.0
        )),
        _ => Ok(()),
    }
}

// ---- guild houses -----------------------------------------------------------------------------------------------

const HOUSE_COLUMNS: &[&str] = &["guild_id", "world_id", "upkeep_until", "created_at"];

const HOUSE_SELECT: &str = "SELECT guild_id, world_id, upkeep_until, created_at FROM guild_houses";

fn read_house(row: &rusqlite::Row<'_>) -> Result<GuildHouseRecord, DbError> {
    let mut c = Cols::new(row, HOUSE_COLUMNS, 0);
    Ok(GuildHouseRecord {
        guild_id: GuildId(c.int()?),
        world_id: WorldId(c.int()?),
        upkeep_until: UnixTime(c.int()?),
        created_at: UnixTime(c.int()?),
    })
}

/// The house of a guild.
pub fn house_of(conn: &Connection, guild: GuildId) -> Result<Option<GuildHouseRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{HOUSE_SELECT} WHERE guild_id = ?1"))?;
    let mut rows = stmt.query([guild.0])?;
    rows.next()?.map(read_house).transpose()
}

/// `UpsertGuildHouse`: inserts the house or rewrites world and `upkeep_until` (`created_at` is immutable: a buy over
/// an existing house is `Conflict`). `NotFound` for an unknown guild, `Invalid` for a negative `upkeep_until`.
pub fn upsert_guild_house(conn: &Connection, h: &GuildHouseRecord) -> Result<(), DbError> {
    if h.upkeep_until.0 < 0 {
        return Err(invalid(format!(
            "UpsertGuildHouse {}: negative upkeep_until {}",
            h.guild_id, h.upkeep_until.0
        )));
    }
    let old = house_of(conn, h.guild_id)?;
    check_guild_house_transition(old.as_ref(), h)
        .map_err(|why| conflict(format!("UpsertGuildHouse {}: {why}", h.guild_id)))?;
    conn.prepare_cached(
        "INSERT INTO guild_houses (guild_id, world_id, upkeep_until, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(guild_id) DO UPDATE SET
           world_id = excluded.world_id, upkeep_until = excluded.upkeep_until",
    )?
    .execute(params![
        h.guild_id.0,
        h.world_id.0,
        h.upkeep_until.0,
        h.created_at.0
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("UpsertGuildHouse {}", h.guild_id)))
}

const GUILD_FURNITURE_COLUMNS: &[&str] = &[
    "seq",
    "guild_id",
    "item_id",
    "installed",
    "pos_x",
    "pos_y",
    "pos_z",
    "angle",
    "keep_until",
    "listed_at",
    "installed_at",
];

const GUILD_FURNITURE_SELECT: &str = "SELECT seq, guild_id, item_id, installed, pos_x, pos_y, pos_z, angle, \
     keep_until, listed_at, installed_at FROM guild_house_furniture";

fn read_guild_furniture(row: &rusqlite::Row<'_>) -> Result<GuildHouseFurnitureRecord, DbError> {
    let mut c = Cols::new(row, GUILD_FURNITURE_COLUMNS, 0);
    Ok(GuildHouseFurnitureRecord {
        seq: c.int()?,
        guild_id: GuildId(c.int()?),
        item_id: c.int()?,
        installed: c.int::<i64>()? != 0,
        pos: [c.f32()?, c.f32()?, c.f32()?],
        angle: c.f32()?,
        keep_until: UnixTime(c.int()?),
        listed_at: UnixTime(c.int()?),
        installed_at: c.opt_int::<i64>()?.map(UnixTime),
    })
}

/// One piece of guild furniture by `seq`.
pub fn guild_furniture_by_seq(
    conn: &Connection,
    seq: u32,
) -> Result<Option<GuildHouseFurnitureRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{GUILD_FURNITURE_SELECT} WHERE seq = ?1"))?;
    let mut rows = stmt.query([seq])?;
    rows.next()?.map(read_guild_furniture).transpose()
}

/// `UpsertGuildHouseFurniture`: inserts the piece or rewrites `installed`, position, angle, keep time and
/// `installed_at`; `guild_id`, `item_id` and `listed_at` are immutable per `seq` (`Conflict`). `NotFound` without a
/// house, `Invalid` for seq 0 and a non-finite position or angle.
pub fn upsert_guild_furniture(
    conn: &Connection,
    f: &GuildHouseFurnitureRecord,
) -> Result<(), DbError> {
    let label = || format!("UpsertGuildHouseFurniture seq {}", f.seq);
    if f.seq == 0 {
        return Err(invalid(format!("{}: seq 0 is not allocated", label())));
    }
    finite_placement(&label(), &f.pos, f.angle)?;
    let old = guild_furniture_by_seq(conn, f.seq)?;
    check_guild_furniture_transition(old.as_ref(), f)
        .map_err(|why| conflict(format!("{}: {why}", label())))?;
    conn.prepare_cached(
        "INSERT INTO guild_house_furniture (seq, guild_id, item_id, installed, pos_x, pos_y, pos_z, angle,
                                            keep_until, listed_at, installed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(seq) DO UPDATE SET
           installed = excluded.installed, pos_x = excluded.pos_x, pos_y = excluded.pos_y,
           pos_z = excluded.pos_z, angle = excluded.angle, keep_until = excluded.keep_until,
           installed_at = excluded.installed_at",
    )?
    .execute(params![
        f.seq,
        f.guild_id.0,
        f.item_id,
        i64::from(f.installed),
        f64::from(f.pos[0]),
        f64::from(f.pos[1]),
        f64::from(f.pos[2]),
        f64::from(f.angle),
        f.keep_until.0,
        f.listed_at.0,
        f.installed_at.map(|t| t.0),
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &label()))
}

/// `DeleteGuildHouseFurniture`: removes one piece (a missing `seq` is not an error). Returns whether a row was
/// deleted.
pub fn delete_guild_furniture(conn: &Connection, seq: u32) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM guild_house_furniture WHERE seq = ?1")?
        .execute([seq])?
        > 0)
}

// ---- housing ----------------------------------------------------------------------------------------------------

const HOUSING_COLUMNS: &[&str] = &[
    "character_id",
    "item_id",
    "keep_until",
    "installed",
    "pos_x",
    "pos_y",
    "pos_z",
    "angle",
    "listed_at",
];

const HOUSING_SELECT: &str = "SELECT character_id, item_id, keep_until, installed, pos_x, pos_y, pos_z, angle, \
     listed_at FROM housing_furniture";

fn read_housing(row: &rusqlite::Row<'_>) -> Result<HousingFurnitureRecord, DbError> {
    let mut c = Cols::new(row, HOUSING_COLUMNS, 0);
    Ok(HousingFurnitureRecord {
        character_id: CharacterId(c.int()?),
        item_id: c.int()?,
        keep_until: UnixTime(c.int()?),
        installed: c.int::<i64>()? != 0,
        pos: [c.f32()?, c.f32()?, c.f32()?],
        angle: c.f32()?,
        listed_at: UnixTime(c.int()?),
    })
}

/// One housing piece by owner and item id.
pub fn housing_by_key(
    conn: &Connection,
    owner: CharacterId,
    item_id: u32,
) -> Result<Option<HousingFurnitureRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{HOUSING_SELECT} WHERE character_id = ?1 AND item_id = ?2"
    ))?;
    let mut rows = stmt.query(params![owner.0, item_id])?;
    rows.next()?.map(read_housing).transpose()
}

fn housing_params(f: &HousingFurnitureRecord) -> [rusqlite::types::Value; 9] {
    use rusqlite::types::Value;
    [
        Value::Integer(i64::from(f.character_id.0)),
        Value::Integer(i64::from(f.item_id)),
        Value::Integer(f.keep_until.0),
        Value::Integer(i64::from(f.installed)),
        Value::Real(f64::from(f.pos[0])),
        Value::Real(f64::from(f.pos[1])),
        Value::Real(f64::from(f.pos[2])),
        Value::Real(f64::from(f.angle)),
        Value::Integer(f.listed_at.0),
    ]
}

/// `UpsertHousingFurniture` (setup, remove, move): inserts the piece or rewrites `installed`, position and angle;
/// `listed_at` and `keep_until` are immutable (`Conflict`). `NotFound` for an unknown character, `Invalid` for a
/// non-finite position or angle.
pub fn upsert_housing(conn: &Connection, f: &HousingFurnitureRecord) -> Result<(), DbError> {
    let label = || {
        format!(
            "UpsertHousingFurniture {} item {}",
            f.character_id, f.item_id
        )
    };
    finite_placement(&label(), &f.pos, f.angle)?;
    let old = housing_by_key(conn, f.character_id, f.item_id)?;
    check_housing_transition(old.as_ref(), f)
        .map_err(|why| conflict(format!("{}: {why}", label())))?;
    conn.prepare_cached(
        "INSERT INTO housing_furniture (character_id, item_id, keep_until, installed, pos_x, pos_y, pos_z,
                                        angle, listed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(character_id, item_id) DO UPDATE SET
           installed = excluded.installed, pos_x = excluded.pos_x, pos_y = excluded.pos_y,
           pos_z = excluded.pos_z, angle = excluded.angle",
    )?
    .execute(rusqlite::params_from_iter(housing_params(f)))
    .map(|_| ())
    .map_err(|e| map_write_error(e, &label()))
}

/// `InsertHousingFurniture`: the strict insert of the list-up batch. An item the owner already lists is `Conflict`
/// (the consumed item rolls back; an upsert could not refuse a duplicate of the same second). `NotFound` for an
/// unknown character, `Invalid` for a non-finite position or angle.
pub fn insert_housing(conn: &Connection, f: &HousingFurnitureRecord) -> Result<(), DbError> {
    let label = || {
        format!(
            "InsertHousingFurniture {} item {}",
            f.character_id, f.item_id
        )
    };
    finite_placement(&label(), &f.pos, f.angle)?;
    conn.prepare_cached(
        "INSERT INTO housing_furniture (character_id, item_id, keep_until, installed, pos_x, pos_y, pos_z,
                                        angle, listed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?
    .execute(rusqlite::params_from_iter(housing_params(f)))
    .map(|_| ())
    .map_err(|e| {
        if is_constraint(&e, "UNIQUE", "") {
            conflict(format!(
                "{}: the owner already lists the item (the consumed item would be lost)",
                label()
            ))
        } else {
            map_write_error(e, &label())
        }
    })
}

/// `DeleteHousingFurniture`: removes one piece (a missing piece is not an error). Returns whether a row was deleted.
pub fn delete_housing(
    conn: &Connection,
    owner: CharacterId,
    item_id: u32,
) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM housing_furniture WHERE character_id = ?1 AND item_id = ?2")?
        .execute(params![owner.0, item_id])?
        > 0)
}

/// `SetHousingVisitor`: `allowed` inserts the visitor, otherwise deletes it (both idempotent). Owner and visitor must
/// differ (`Invalid`); an unknown character is `NotFound` when inserting.
pub fn set_housing_visitor(
    conn: &Connection,
    owner: CharacterId,
    visitor: CharacterId,
    allowed: bool,
) -> Result<(), DbError> {
    if owner == visitor {
        return Err(invalid(format!(
            "SetHousingVisitor: character {owner} cannot visit its own room"
        )));
    }
    if allowed {
        conn.prepare_cached(
            "INSERT OR IGNORE INTO housing_visitors (character_id, visitor_id) VALUES (?1, ?2)",
        )?
        .execute(params![owner.0, visitor.0])
        .map(|_| ())
        .map_err(|e| {
            map_write_error(
                e,
                &format!("SetHousingVisitor owner {owner} visitor {visitor}"),
            )
        })
    } else {
        conn.prepare_cached(
            "DELETE FROM housing_visitors WHERE character_id = ?1 AND visitor_id = ?2",
        )?
        .execute(params![owner.0, visitor.0])?;
        Ok(())
    }
}

/// `DeleteHousingVisitsOf`: the character stops being a visitor of every room; returns how many rows were removed.
pub fn delete_housing_visits_of(conn: &Connection, character: CharacterId) -> Result<u32, DbError> {
    let n = conn
        .prepare_cached("DELETE FROM housing_visitors WHERE visitor_id = ?1")?
        .execute([character.0])?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

// ---- rainbow race -----------------------------------------------------------------------------------------------

const ENTRY_COLUMNS: &[&str] = &[
    "round",
    "character_id",
    "state",
    "ranking",
    "applied_at",
    "finished_at",
];

const ENTRY_SELECT: &str = "SELECT round, character_id, state, ranking, applied_at, finished_at \
     FROM rainbow_race_entries";

fn read_entry(row: &rusqlite::Row<'_>) -> Result<RainbowRaceEntryRecord, DbError> {
    let mut c = Cols::new(row, ENTRY_COLUMNS, 0);
    let round: u32 = c.int()?;
    let character_id = CharacterId(c.int()?);
    let state_text = c.text()?;
    Ok(RainbowRaceEntryRecord {
        round,
        character_id,
        state: RainbowEntryState::parse(&state_text).ok_or_else(|| {
            bug(format!(
                "rainbow_race_entries.state '{state_text}' of round {round} character {character_id}"
            ))
        })?,
        ranking: c.int()?,
        applied_at: UnixTime(c.int()?),
        finished_at: c.opt_int::<i64>()?.map(UnixTime),
    })
}

/// One entry by round and character.
pub fn entry_by_key(
    conn: &Connection,
    round: u32,
    character: CharacterId,
) -> Result<Option<RainbowRaceEntryRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{ENTRY_SELECT} WHERE round = ?1 AND character_id = ?2"
    ))?;
    let mut rows = stmt.query(params![round, character.0])?;
    rows.next()?.map(read_entry).transpose()
}

/// `UpsertRainbowRaceEntry`: inserts the entry or rewrites state, ranking and `finished_at`, subject to
/// [`check_rainbow_transition`] (`Conflict`). `Invalid` for a ranking of 0 or less on `R`/`W` and a ranking other
/// than 0 on `N`/`L`/`F`.
pub fn upsert_rainbow_entry(conn: &Connection, e: &RainbowRaceEntryRecord) -> Result<(), DbError> {
    let label = || {
        format!(
            "UpsertRainbowRaceEntry round {} character {}",
            e.round, e.character_id
        )
    };
    let ranked = matches!(
        e.state,
        RainbowEntryState::Finished | RainbowEntryState::Winner
    );
    if ranked && e.ranking <= 0 {
        return Err(invalid(format!(
            "{}: state {} needs a ranking above 0, not {}",
            label(),
            e.state.code(),
            e.ranking
        )));
    }
    if !ranked && e.ranking != 0 {
        return Err(invalid(format!(
            "{}: state {} has no ranking, not {}",
            label(),
            e.state.code(),
            e.ranking
        )));
    }
    let old = entry_by_key(conn, e.round, e.character_id)?.map(|r| r.state);
    check_rainbow_transition(old, e.state)
        .map_err(|why| conflict(format!("{}: {why}", label())))?;
    conn.prepare_cached(
        "INSERT INTO rainbow_race_entries (round, character_id, state, ranking, applied_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(round, character_id) DO UPDATE SET
           state = excluded.state, ranking = excluded.ranking, finished_at = excluded.finished_at",
    )?
    .execute(params![
        e.round,
        e.character_id.0,
        e.state.as_str(),
        e.ranking,
        e.applied_at.0,
        e.finished_at.map(|t| t.0),
    ])
    .map(|_| ())
    .map_err(|err| map_write_error(err, &label()))
}

// ---- quiz -------------------------------------------------------------------------------------------------------

const QUIZ_COLUMNS: &[&str] = &[
    "id",
    "quiz_type",
    "question",
    "choices",
    "answer",
    "prize_item_id",
    "prize_count",
    "enabled",
    "used_at",
];

const QUIZ_SELECT: &str = "SELECT id, quiz_type, question, choices, answer, prize_item_id, prize_count, \
     enabled, used_at FROM quiz_questions";

fn read_quiz(row: &rusqlite::Row<'_>) -> Result<QuizQuestionRecord, DbError> {
    let mut c = Cols::new(row, QUIZ_COLUMNS, 0);
    let id: i64 = c.int()?;
    let code: u8 = c.int()?;
    let question = c.text()?;
    let choices_text = c.text()?;
    Ok(QuizQuestionRecord {
        id,
        quiz_type: QuizType::from_code(code)
            .ok_or_else(|| bug(format!("quiz_questions.quiz_type {code} of question {id}")))?,
        question,
        choices: serde_json::from_str::<Vec<String>>(&choices_text)
            .map_err(|e| bug(format!("quiz_questions.choices of question {id}: {e}")))?,
        answer: c.int()?,
        prize_item_id: c.int()?,
        prize_count: c.int()?,
        enabled: c.int::<i64>()? != 0,
        used_at: c.opt_int::<i64>()?.map(UnixTime),
    })
}

/// One quiz question by id.
pub fn quiz_by_id(conn: &Connection, id: i64) -> Result<Option<QuizQuestionRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{QUIZ_SELECT} WHERE id = ?1"))?;
    let mut rows = stmt.query([id])?;
    rows.next()?.map(read_quiz).transpose()
}

/// The first row (lowest id) of a natural key `(quiz_type, question)`; an unreadable row counts as different.
fn quiz_by_natural_key(
    conn: &Connection,
    quiz_type: QuizType,
    question: &str,
) -> Result<Option<(i64, Option<QuizQuestionRecord>)>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{QUIZ_SELECT} WHERE quiz_type = ?1 AND question = ?2 ORDER BY id LIMIT 1"
    ))?;
    let mut rows = stmt.query(params![quiz_type.code(), question])?;
    match rows.next()? {
        None => Ok(None),
        Some(row) => {
            let id: i64 = row.get(0)?;
            Ok(Some((id, read_quiz(row).ok())))
        }
    }
}

/// What [`upsert_quiz_question`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuizUpsert {
    /// A new row with this id.
    Inserted(i64),
    /// An existing row was rewritten.
    Updated,
    /// An existing row already had these values.
    Unchanged,
}

fn same_content(old: &QuizQuestionRecord, new: &QuizQuestionRecord) -> bool {
    old.choices == new.choices
        && old.answer == new.answer
        && old.prize_item_id == new.prize_item_id
        && old.prize_count == new.prize_count
        && old.enabled == new.enabled
}

fn write_quiz_content(conn: &Connection, id: i64, q: &QuizQuestionRecord) -> Result<(), DbError> {
    conn.prepare_cached(
        "UPDATE quiz_questions SET quiz_type = ?2, question = ?3, choices = ?4, answer = ?5,
                                   prize_item_id = ?6, prize_count = ?7, enabled = ?8
         WHERE id = ?1",
    )?
    .execute(params![
        id,
        q.quiz_type.code(),
        q.question,
        serde_json::to_string(&q.choices)?,
        q.answer,
        q.prize_item_id,
        q.prize_count,
        i64::from(q.enabled),
    ])?;
    Ok(())
}

/// `UpsertQuizQuestion`: [`QuizQuestionRecord::validate`] first (`Invalid`). `id == 0` is a natural-key upsert on
/// `(quiz_type, question)`: a new row is inserted, an existing one gets choices, answer, prizes and `enabled`
/// rewritten and keeps its `id` and `used_at`. `id > 0` updates that row (`NotFound` when missing, `Conflict` when it
/// would duplicate the natural key of another row); `used_at` is only written by [`mark_quiz_used`].
pub fn upsert_quiz_question(
    conn: &Connection,
    q: &QuizQuestionRecord,
) -> Result<QuizUpsert, DbError> {
    q.validate()
        .map_err(|why| invalid(format!("UpsertQuizQuestion: {why}")))?;
    if q.id == 0 {
        return match quiz_by_natural_key(conn, q.quiz_type, &q.question)? {
            None => {
                conn.prepare_cached(
                    "INSERT INTO quiz_questions (quiz_type, question, choices, answer, prize_item_id,
                                                 prize_count, enabled, used_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )?
                .execute(params![
                    q.quiz_type.code(),
                    q.question,
                    serde_json::to_string(&q.choices)?,
                    q.answer,
                    q.prize_item_id,
                    q.prize_count,
                    i64::from(q.enabled),
                    q.used_at.map(|t| t.0),
                ])?;
                Ok(QuizUpsert::Inserted(conn.last_insert_rowid()))
            }
            Some((_, Some(old))) if same_content(&old, q) => Ok(QuizUpsert::Unchanged),
            Some((id, _)) => {
                write_quiz_content(conn, id, q)?;
                Ok(QuizUpsert::Updated)
            }
        };
    }
    let Some(old) = quiz_by_id(conn, q.id)? else {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("UpsertQuizQuestion: question {} does not exist", q.id),
        ));
    };
    let clash: Option<i64> = conn
        .prepare_cached(
            "SELECT id FROM quiz_questions WHERE quiz_type = ?1 AND question = ?2 AND id <> ?3 LIMIT 1",
        )?
        .query_row(params![q.quiz_type.code(), q.question, q.id], |r| r.get(0))
        .optional()?;
    if let Some(other) = clash {
        return Err(conflict(format!(
            "UpsertQuizQuestion {}: question {other} already has this type and text",
            q.id
        )));
    }
    if old.quiz_type == q.quiz_type && old.question == q.question && same_content(&old, q) {
        return Ok(QuizUpsert::Unchanged);
    }
    write_quiz_content(conn, q.id, q)?;
    Ok(QuizUpsert::Updated)
}

/// `MarkQuizQuestionsUsed`: stamps `used_at = at` on the listed questions (unknown ids are ignored); returns how many
/// rows were stamped.
pub fn mark_quiz_used(conn: &Connection, ids: &[i64], at: UnixTime) -> Result<u32, DbError> {
    let mut stmt = conn.prepare_cached("UPDATE quiz_questions SET used_at = ?2 WHERE id = ?1")?;
    let mut done = BTreeSet::new();
    let mut changed = 0_u32;
    for id in ids {
        if done.insert(*id) {
            changed = changed
                .saturating_add(u32::try_from(stmt.execute(params![id, at.0])?).unwrap_or(0));
        }
    }
    Ok(changed)
}

/// `PruneQuizQuestions`: deletes the rows of `quiz_type` whose question is not in `keep` (the `--replace` of
/// `flyff quiz import`: only the given type, the ids of the questions that stay are kept); returns the number of
/// deleted rows.
pub fn prune_quiz_questions(
    conn: &Connection,
    quiz_type: QuizType,
    keep: &[String],
) -> Result<u32, DbError> {
    let keep: BTreeSet<&str> = keep.iter().map(String::as_str).collect();
    let doomed: Vec<i64> = {
        let mut stmt = conn.prepare_cached(
            "SELECT id, question FROM quiz_questions WHERE quiz_type = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([quiz_type.code()], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut doomed = Vec::new();
        for row in rows {
            let (id, question) = row?;
            if !keep.contains(question.as_str()) {
                doomed.push(id);
            }
        }
        doomed
    };
    let mut delete = conn.prepare_cached("DELETE FROM quiz_questions WHERE id = ?1")?;
    let mut removed = 0_u32;
    for id in doomed {
        removed = removed.saturating_add(u32::try_from(delete.execute([id])?).unwrap_or(0));
    }
    Ok(removed)
}

// ---- reads ------------------------------------------------------------------------------------------------------

/// The guild houses at boot (`StartupState.guild_houses`): every house by guild id with its furniture in `seq` order.
pub fn load_guild_houses(conn: &Connection) -> Result<Vec<GuildHouseLoad>, DbError> {
    let mut houses: Vec<GuildHouseLoad> = Vec::new();
    {
        let mut stmt = conn.prepare_cached(&format!("{HOUSE_SELECT} ORDER BY guild_id"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            houses.push(GuildHouseLoad {
                house: read_house(row)?,
                furniture: Vec::new(),
            });
        }
    }
    if houses.is_empty() {
        return Ok(houses);
    }
    let mut stmt =
        conn.prepare_cached(&format!("{GUILD_FURNITURE_SELECT} ORDER BY guild_id, seq"))?;
    let mut rows = stmt.query([])?;
    let mut cursor = 0;
    while let Some(row) = rows.next()? {
        let piece = read_guild_furniture(row)?;
        // both scans are ordered by guild: walk the houses forward instead of searching them
        while houses
            .get(cursor)
            .is_some_and(|h| h.house.guild_id < piece.guild_id)
        {
            cursor += 1;
        }
        match houses.get_mut(cursor) {
            Some(h) if h.house.guild_id == piece.guild_id => h.furniture.push(piece),
            _ => {
                return Err(bug(format!(
                    "guild_house_furniture seq {} has no house of guild {}",
                    piece.seq, piece.guild_id
                )));
            }
        }
    }
    Ok(houses)
}

/// The housing of one character (`CharacterLoad.housing`): the raw furniture rows (expired ones included) by
/// `listed_at` then item id, and the characters allowed to visit by id.
pub fn load_housing(conn: &Connection, character: CharacterId) -> Result<HousingLoad, DbError> {
    let mut items = Vec::new();
    {
        let mut stmt = conn.prepare_cached(&format!(
            "{HOUSING_SELECT} WHERE character_id = ?1 ORDER BY listed_at, item_id"
        ))?;
        let mut rows = stmt.query([character.0])?;
        while let Some(row) = rows.next()? {
            items.push(read_housing(row)?);
        }
    }
    let mut visitors = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT visitor_id FROM housing_visitors WHERE character_id = ?1 ORDER BY visitor_id",
        )?;
        let mut rows = stmt.query([character.0])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["visitor_id"], 0);
            visitors.push(CharacterId(c.int()?));
        }
    }
    Ok(HousingLoad { items, visitors })
}

/// The rainbow-race state at boot (`StartupState.rainbow_race`): the entries of `round` (= `meta.rainbow_race_round`)
/// and the previous ranking (the first five winners with a ranking above 0 of the newest earlier round that has any
/// entry row).
pub fn load_rainbow_race(conn: &Connection, round: u32) -> Result<RainbowRaceLoad, DbError> {
    let mut entries = Vec::new();
    {
        let mut stmt = conn.prepare_cached(&format!(
            "{ENTRY_SELECT} WHERE round = ?1 ORDER BY applied_at, character_id"
        ))?;
        let mut rows = stmt.query([round])?;
        while let Some(row) = rows.next()? {
            entries.push(read_entry(row)?);
        }
    }
    let previous: Option<u32> = conn
        .prepare_cached("SELECT MAX(round) FROM rainbow_race_entries WHERE round < ?1")?
        .query_row([round], |r| r.get::<_, Option<u32>>(0))?;
    let mut prev_ranking = Vec::new();
    if let Some(previous) = previous {
        let mut stmt = conn.prepare_cached(
            "SELECT character_id FROM rainbow_race_entries
             WHERE round = ?1 AND state = 'W' AND ranking > 0 ORDER BY ranking, character_id LIMIT 5",
        )?;
        let mut rows = stmt.query([previous])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["character_id"], 0);
            prev_ranking.push(CharacterId(c.int()?));
        }
    }
    Ok(RainbowRaceLoad {
        round,
        entries,
        prev_ranking,
    })
}

/// `GameReadQuery::QuizQuestions`: the enabled questions of `quiz_type` by id, with `used_at` (the DB hides no used
/// question, the quiz service chooses). A row that cannot be read (broken `choices`, unknown type or answer out of
/// range) is skipped with an error log.
pub fn load_quiz_questions(
    conn: &Connection,
    quiz_type: QuizType,
) -> Result<Vec<QuizQuestionRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{QUIZ_SELECT} WHERE quiz_type = ?1 AND enabled = 1 ORDER BY id"
    ))?;
    let mut rows = stmt.query([quiz_type.code()])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        match read_quiz(row) {
            Ok(q) => out.push(q),
            Err(e) => {
                let id = row.get::<_, i64>(0).unwrap_or(0);
                tracing::error!(question = id, error = %e, "a quiz question cannot be read and is skipped");
            }
        }
    }
    Ok(out)
}
