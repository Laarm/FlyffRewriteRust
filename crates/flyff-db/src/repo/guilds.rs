//! SQL of the guild group (migration `0003`, Plan/13 §8, §16.3, §16.9; rules in Plan/21 §5-§6): guilds, members,
//! the bank document, the bank log, wars, rejoin cooldowns and guild quests.
//!
//! The write functions run on the writer connection inside the command savepoint (`DbOp` dispatch in
//! [`crate::ops`]). Failures map to the *Expected* kinds of Plan/13 §16.6:
//!
//! * a missing guild or character (the `FOREIGN KEY`s) is `NotFound`,
//! * a guild name that is taken (`guilds.name`, case-insensitive, `''` exempt) is `GuildNameTaken`,
//! * a second master (`guild_members.guild_id`) or a character that already belongs to another guild is
//!   `Conflict`, as is an illegal change of a war,
//! * a value outside its documented limit is `Invalid` (the game validates first; this is the second line).
//!
//! The read functions run on reader connections: the set-based startup load ([`load_guilds`] and friends, one query
//! per table, grouped in memory) and the two pages of the admin drawer ([`bank_log_page`], [`wars_of`]).

use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{
    GUILD_ALIAS_MAX_BYTES, GUILD_LOG_TAIL, GUILD_LOGO_MAX, GUILD_MAX_LEVEL, GUILD_NAME_MAX_CHARS,
    GUILD_NOTICE_MAX_BYTES, GUILD_RANKS, GuildBankDoc, GuildBankLogKind, GuildBankLogRecord,
    GuildBankLogTail, GuildLoad, GuildMemberRecord, GuildQuestRecord, GuildRecord, GuildWarRecord,
    JsonDoc, RANK_MASTER, WAR_RESULT_MAX, WarSideRecord,
};
use crate::repo::characters::Cols;
use flyff_common::{CharacterId, GuildId, GuildWarId, UnixTime};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

/// `GuildBankDoc` capacity: the guild bank has 42 slots.
const BANK_SLOTS: u16 = GuildBankDoc::CAPACITY;
/// Largest page of the admin pages (`limit` 0 means [`DEFAULT_PAGE`]).
pub const MAX_PAGE: u32 = 500;
/// The page size when `limit` is 0.
pub const DEFAULT_PAGE: u32 = 100;

pub(crate) fn invalid(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Invalid, what)
}

pub(crate) fn conflict(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Conflict, what)
}

fn bug(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Bug, what)
}

/// Maps the constraint failures a guild write can hit to *Expected* kinds.
fn map_write_error(e: rusqlite::Error, what: &str) -> DbError {
    if is_constraint(&e, "FOREIGN KEY", "") {
        DbError::new(
            DbErrorKind::NotFound,
            format!("{what}: the guild or the character does not exist"),
        )
    } else if is_constraint(&e, "UNIQUE", "guilds.name") {
        DbError::new(
            DbErrorKind::GuildNameTaken,
            format!("{what}: the guild name is taken"),
        )
    } else if is_constraint(&e, "UNIQUE", "guild_members.guild_id") {
        conflict(format!("{what}: the guild already has a master"))
    } else if is_constraint(&e, "UNIQUE", "") {
        conflict(format!("{what}: a row with that key exists"))
    } else {
        e.into()
    }
}

fn u32_of(v: i64, what: &str) -> Result<u32, DbError> {
    u32::try_from(v).map_err(|_| bug(format!("{what} holds {v}, out of range")))
}

// ---- guild rows -------------------------------------------------------------------------------------------------

fn validate_guild(g: &GuildRecord) -> Result<(), DbError> {
    if !(1..=GUILD_MAX_LEVEL).contains(&g.level) {
        return Err(invalid(format!(
            "UpsertGuild {}: level {} is outside 1..={GUILD_MAX_LEVEL}",
            g.id, g.level
        )));
    }
    if g.name.chars().count() > GUILD_NAME_MAX_CHARS {
        return Err(invalid(format!(
            "UpsertGuild {}: the name is longer than {GUILD_NAME_MAX_CHARS} characters",
            g.id
        )));
    }
    if g.notice.len() > GUILD_NOTICE_MAX_BYTES {
        return Err(invalid(format!(
            "UpsertGuild {}: the notice is longer than {GUILD_NOTICE_MAX_BYTES} bytes",
            g.id
        )));
    }
    if g.logo > GUILD_LOGO_MAX {
        return Err(invalid(format!(
            "UpsertGuild {}: logo {} is above {GUILD_LOGO_MAX}",
            g.id, g.logo
        )));
    }
    Ok(())
}

/// `UpsertGuild`: inserts the guild or rewrites every column except `created_at`. `GuildNameTaken` when another
/// named guild has the name (case-insensitively); a rename that only changes the case of the guild's own name works.
pub fn upsert_guild(conn: &Connection, g: &GuildRecord) -> Result<(), DbError> {
    validate_guild(g)?;
    let powers = serde_json::to_string(&g.rank_powers)?;
    let salaries = serde_json::to_string(&g.rank_salaries)?;
    conn.prepare_cached(
        "INSERT INTO guilds (id, name, level, gold, pxp, logo, notice, rank_powers, rank_salaries,
                             war_wins, war_losses, war_surrenders, war_points, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name, level = excluded.level, gold = excluded.gold, pxp = excluded.pxp,
           logo = excluded.logo, notice = excluded.notice, rank_powers = excluded.rank_powers,
           rank_salaries = excluded.rank_salaries, war_wins = excluded.war_wins,
           war_losses = excluded.war_losses, war_surrenders = excluded.war_surrenders,
           war_points = excluded.war_points",
    )?
    .execute(params![
        g.id.0,
        g.name,
        g.level,
        g.gold,
        g.pxp,
        g.logo,
        g.notice,
        powers,
        salaries,
        g.war_wins,
        g.war_losses,
        g.war_surrenders,
        g.war_points,
        g.created_at.0,
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("UpsertGuild {}", g.id)))
}

/// `DeleteGuild`: removes the guild; members, bank, log, quests and (0007) the house with its furniture go with it
/// by cascade. War rows, siege rows and `pending_removals.guild_id` survive (history, no foreign key). A missing
/// guild is not an error. Returns whether a row was deleted.
pub fn delete_guild(conn: &Connection, id: GuildId) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM guilds WHERE id = ?1")?
        .execute([id.0])?
        > 0)
}

// ---- members ----------------------------------------------------------------------------------------------------

fn validate_member(m: &GuildMemberRecord) -> Result<(), DbError> {
    if usize::from(m.rank) >= GUILD_RANKS {
        return Err(invalid(format!(
            "UpsertGuildMember {}: rank {} is outside 0..=4",
            m.character_id, m.rank
        )));
    }
    if m.class > 2 {
        return Err(invalid(format!(
            "UpsertGuildMember {}: class {} is outside 0..=2",
            m.character_id, m.class
        )));
    }
    if m.alias.len() > GUILD_ALIAS_MAX_BYTES {
        return Err(invalid(format!(
            "UpsertGuildMember {}: the alias is longer than {GUILD_ALIAS_MAX_BYTES} bytes",
            m.character_id
        )));
    }
    Ok(())
}

/// `UpsertGuildMember`: inserts the member or rewrites rank, class, alias, contributions, surrenders and war
/// participation (`joined_at` is kept). A character that belongs to **another** guild is `Conflict` (the update
/// only applies `WHERE guild_id = excluded.guild_id`), and so is a second rank-0 member of a guild (partial unique
/// index): a master change demotes the old master first ([`crate::ops::DbOp::guild_master_change_ops`]). `NotFound`
/// for an unknown guild or character.
pub fn upsert_member(conn: &Connection, m: &GuildMemberRecord) -> Result<(), DbError> {
    validate_member(m)?;
    let changed = conn
        .prepare_cached(
            "INSERT INTO guild_members (character_id, guild_id, rank, class, alias, contributed_gold,
                                        contributed_pxp, surrenders, war_id, joined_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(character_id) DO UPDATE SET
               rank = excluded.rank, class = excluded.class, alias = excluded.alias,
               contributed_gold = excluded.contributed_gold, contributed_pxp = excluded.contributed_pxp,
               surrenders = excluded.surrenders, war_id = excluded.war_id
             WHERE guild_members.guild_id = excluded.guild_id",
        )?
        .execute(params![
            m.character_id.0,
            m.guild_id.0,
            m.rank,
            m.class,
            m.alias,
            m.contributed_gold,
            m.contributed_pxp,
            m.surrenders,
            m.war_id,
            m.joined_at.0,
        ])
        .map_err(|e| map_write_error(e, &format!("UpsertGuildMember {}", m.character_id)))?;
    if changed == 0 {
        return Err(conflict(format!(
            "UpsertGuildMember: character {} belongs to another guild than {}",
            m.character_id, m.guild_id
        )));
    }
    Ok(())
}

/// `DeleteGuildMember`: removes the character from its guild (idempotent). Returns whether a row was deleted.
pub fn delete_member(conn: &Connection, character: CharacterId) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM guild_members WHERE character_id = ?1")?
        .execute([character.0])?
        > 0)
}

/// `SetGuildWarOfMembers`: sets `guild_members.war_id` of every member of the two guilds in one statement (a war
/// start sets the id, the end sets 0). Returns the number of rows changed.
pub fn set_war_of_members(
    conn: &Connection,
    guilds: [GuildId; 2],
    war_id: u32,
) -> Result<u32, DbError> {
    let n = conn
        .prepare_cached("UPDATE guild_members SET war_id = ?3 WHERE guild_id IN (?1, ?2)")?
        .execute(params![guilds[0].0, guilds[1].0, war_id])?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

// ---- bank and log -----------------------------------------------------------------------------------------------

/// `SetGuildBank`: replaces the 42-slot bank document. A container that is not valid for 42 slots is `Invalid`
/// (the guild service repairs at load; a stored bank is always valid), an unknown guild `NotFound`. The penya pool is
/// `guilds.gold`, not part of the document.
pub fn set_bank(conn: &Connection, guild: GuildId, doc: &GuildBankDoc) -> Result<(), DbError> {
    if !doc.container.is_valid(BANK_SLOTS) {
        return Err(invalid(format!(
            "SetGuildBank {guild}: the container is not a valid {BANK_SLOTS}-slot bank"
        )));
    }
    let text = doc.to_json()?;
    conn.prepare_cached(
        "INSERT INTO guild_banks (guild_id, container) VALUES (?1, ?2)
         ON CONFLICT(guild_id) DO UPDATE SET container = excluded.container",
    )?
    .execute(params![guild.0, text])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("SetGuildBank {guild}")))
}

/// `InsertGuildBankLog`: appends one log row (id allocated by the database when the record's id is 0) and returns
/// the row id. `NotFound` for an unknown guild.
pub fn insert_bank_log(conn: &Connection, r: &GuildBankLogRecord) -> Result<i64, DbError> {
    conn.prepare_cached(
        "INSERT INTO guild_bank_log (id, guild_id, character_id, kind, item_id, upgrade_level, count, at)
         VALUES (NULLIF(?1, 0), ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?
    .execute(params![
        r.id,
        r.guild_id.0,
        r.character_id.0,
        r.kind.as_str(),
        r.item_id,
        r.upgrade_level,
        r.count,
        r.at.0,
    ])
    .map_err(|e| map_write_error(e, &format!("InsertGuildBankLog {}", r.guild_id)))?;
    Ok(conn.last_insert_rowid())
}

// ---- rejoin cooldowns and quests --------------------------------------------------------------------------------

/// `SetGuildRejoinCooldown`: stores the end of the cooldown of a character (`None` deletes the row). `NotFound`
/// for an unknown character.
pub fn set_rejoin_cooldown(
    conn: &Connection,
    character: CharacterId,
    until: Option<UnixTime>,
) -> Result<(), DbError> {
    match until {
        Some(until) => conn
            .prepare_cached(
                "INSERT INTO guild_rejoin_cooldowns (character_id, until) VALUES (?1, ?2)
                 ON CONFLICT(character_id) DO UPDATE SET until = excluded.until",
            )?
            .execute(params![character.0, until.0])
            .map(|_| ())
            .map_err(|e| map_write_error(e, &format!("SetGuildRejoinCooldown {character}"))),
        None => conn
            .prepare_cached("DELETE FROM guild_rejoin_cooldowns WHERE character_id = ?1")?
            .execute([character.0])
            .map(|_| ())
            .map_err(DbError::from),
    }
}

/// `UpsertGuildQuest`: one row per guild **and** quest (FIX of the legacy key). `started_at` is kept on update.
pub fn upsert_quest(conn: &Connection, q: &GuildQuestRecord) -> Result<(), DbError> {
    conn.prepare_cached(
        "INSERT INTO guild_quests (guild_id, quest_id, state, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(guild_id, quest_id) DO UPDATE SET state = excluded.state, ended_at = excluded.ended_at",
    )?
    .execute(params![
        q.guild_id.0,
        q.quest_id,
        q.state,
        q.started_at.0,
        q.ended_at.map(|t| t.0),
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("UpsertGuildQuest {}", q.guild_id)))
}

/// `DeleteGuildQuest`: removes one quest row (idempotent).
pub fn delete_quest(conn: &Connection, guild: GuildId, quest_id: u16) -> Result<(), DbError> {
    conn.prepare_cached("DELETE FROM guild_quests WHERE guild_id = ?1 AND quest_id = ?2")?
        .execute(params![guild.0, quest_id])?;
    Ok(())
}

// ---- wars -------------------------------------------------------------------------------------------------------

const WAR_COLUMNS: &[&str] = &[
    "id",
    "declarer_guild_id",
    "acceptor_guild_id",
    "started_at",
    "ended_at",
    "result",
    "decl_members",
    "decl_deaths",
    "decl_surrenders",
    "decl_absent_s",
    "acpt_members",
    "acpt_deaths",
    "acpt_surrenders",
    "acpt_absent_s",
    "decl_points_after",
    "acpt_points_after",
];

const WAR_SELECT: &str = "SELECT id, declarer_guild_id, acceptor_guild_id, started_at, ended_at, result, \
     decl_members, decl_deaths, decl_surrenders, decl_absent_s, acpt_members, acpt_deaths, acpt_surrenders, \
     acpt_absent_s, decl_points_after, acpt_points_after FROM guild_wars";

fn read_war(row: &rusqlite::Row<'_>) -> Result<GuildWarRecord, DbError> {
    let mut c = Cols::new(row, WAR_COLUMNS, 0);
    Ok(GuildWarRecord {
        id: GuildWarId(c.int()?),
        declarer: GuildId(c.int()?),
        acceptor: GuildId(c.int()?),
        started_at: UnixTime(c.int()?),
        ended_at: c.opt_int::<i64>()?.map(UnixTime),
        result: c.opt_int()?,
        decl: WarSideRecord {
            members: c.int()?,
            deaths: c.int()?,
            surrenders: c.int()?,
            absent_s: c.int()?,
        },
        acpt: WarSideRecord {
            members: c.int()?,
            deaths: c.int()?,
            surrenders: c.int()?,
            absent_s: c.int()?,
        },
        decl_points_after: c.opt_int()?,
        acpt_points_after: c.opt_int()?,
    })
}

/// One war by id (any state).
pub fn war_by_id(conn: &Connection, id: GuildWarId) -> Result<Option<GuildWarRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{WAR_SELECT} WHERE id = ?1"))?;
    let mut rows = stmt.query([id.0])?;
    rows.next()?.map(read_war).transpose()
}

/// `UpsertGuildWar`: stores the war with the exact `WR_*` code `0..=9` (FIX: the legacy collapsed AB and DD).
///
/// * Declarer, acceptor, start time and the two member counts are immutable after the insert (`Conflict`).
/// * A war with a `result` is final: rewriting it identically is accepted, anything else is `Conflict`.
/// * `Invalid` for a result above 9 or a war against itself.
pub fn upsert_war(conn: &Connection, w: &GuildWarRecord) -> Result<(), DbError> {
    if w.result.is_some_and(|r| r > WAR_RESULT_MAX) {
        return Err(invalid(format!(
            "UpsertGuildWar {}: result {:?} is not a WR_* code 0..=9",
            w.id, w.result
        )));
    }
    if w.declarer == w.acceptor {
        return Err(invalid(format!(
            "UpsertGuildWar {}: guild {} cannot fight itself",
            w.id, w.declarer
        )));
    }
    if let Some(old) = war_by_id(conn, w.id)? {
        if old.declarer != w.declarer
            || old.acceptor != w.acceptor
            || old.started_at != w.started_at
            || old.decl.members != w.decl.members
            || old.acpt.members != w.acpt.members
        {
            return Err(conflict(format!(
                "UpsertGuildWar {}: declarer, acceptor, start and member counts cannot change",
                w.id
            )));
        }
        if old.result.is_some() {
            return if old == *w {
                Ok(())
            } else {
                Err(conflict(format!(
                    "UpsertGuildWar {}: the war ended with result {:?} and is final",
                    w.id, old.result
                )))
            };
        }
    }
    conn.prepare_cached(
        "INSERT INTO guild_wars (id, declarer_guild_id, acceptor_guild_id, started_at, ended_at, result,
                                 decl_members, decl_deaths, decl_surrenders, decl_absent_s,
                                 acpt_members, acpt_deaths, acpt_surrenders, acpt_absent_s,
                                 decl_points_after, acpt_points_after)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(id) DO UPDATE SET
           ended_at = excluded.ended_at, result = excluded.result,
           decl_deaths = excluded.decl_deaths, decl_surrenders = excluded.decl_surrenders,
           decl_absent_s = excluded.decl_absent_s, acpt_deaths = excluded.acpt_deaths,
           acpt_surrenders = excluded.acpt_surrenders, acpt_absent_s = excluded.acpt_absent_s,
           decl_points_after = excluded.decl_points_after, acpt_points_after = excluded.acpt_points_after",
    )?
    .execute(params![
        w.id.0,
        w.declarer.0,
        w.acceptor.0,
        w.started_at.0,
        w.ended_at.map(|t| t.0),
        w.result,
        w.decl.members,
        w.decl.deaths,
        w.decl.surrenders,
        w.decl.absent_s,
        w.acpt.members,
        w.acpt.deaths,
        w.acpt.surrenders,
        w.acpt.absent_s,
        w.decl_points_after,
        w.acpt_points_after,
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("UpsertGuildWar {}", w.id)))
}

/// A page limit: 0 = [`DEFAULT_PAGE`], above [`MAX_PAGE`] is `Invalid`.
fn page_limit(limit: u32) -> Result<i64, DbError> {
    match limit {
        0 => Ok(i64::from(DEFAULT_PAGE)),
        1..=MAX_PAGE => Ok(i64::from(limit)),
        _ => Err(invalid(format!("page limit {limit} is above {MAX_PAGE}"))),
    }
}

/// The wars a guild took part in (as declarer or acceptor), newest first, `id < before_id` when given: the war
/// history of the admin drawer (Plan/25 §6.1). `limit` 0 = 100, at most 500.
pub fn wars_of(
    conn: &Connection,
    guild: GuildId,
    before_id: Option<GuildWarId>,
    limit: u32,
) -> Result<Vec<GuildWarRecord>, DbError> {
    let limit = page_limit(limit)?;
    let mut stmt = conn.prepare_cached(&format!(
        "{WAR_SELECT} WHERE (declarer_guild_id = ?1 OR acceptor_guild_id = ?1) AND id < ?2
         ORDER BY id DESC LIMIT ?3"
    ))?;
    let mut rows = stmt.query(params![
        guild.0,
        before_id.map_or(i64::MAX, |id| i64::from(id.0)),
        limit
    ])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_war(row)?);
    }
    Ok(out)
}

// ---- reads: the startup load ------------------------------------------------------------------------------------

const GUILD_COLUMNS: &[&str] = &[
    "id",
    "name",
    "level",
    "gold",
    "pxp",
    "logo",
    "notice",
    "rank_powers",
    "rank_salaries",
    "war_wins",
    "war_losses",
    "war_surrenders",
    "war_points",
    "created_at",
];

fn json_array5(text: &str, what: &str) -> Result<[u32; GUILD_RANKS], DbError> {
    serde_json::from_str::<[u32; GUILD_RANKS]>(text)
        .map_err(|e| bug(format!("guilds.{what} '{text}': {e}")))
}

fn read_guild(row: &rusqlite::Row<'_>) -> Result<GuildRecord, DbError> {
    let mut c = Cols::new(row, GUILD_COLUMNS, 0);
    let id = GuildId(c.int()?);
    let name = c.text()?;
    let level = c.int()?;
    let gold = c.int()?;
    let pxp = c.int()?;
    let logo = c.int()?;
    let notice = c.text()?;
    let powers = c.text()?;
    let salaries = c.text()?;
    Ok(GuildRecord {
        id,
        name,
        level,
        gold,
        pxp,
        logo,
        notice,
        rank_powers: json_array5(&powers, "rank_powers")?,
        rank_salaries: json_array5(&salaries, "rank_salaries")?,
        war_wins: c.int()?,
        war_losses: c.int()?,
        war_surrenders: c.int()?,
        war_points: c.int()?,
        created_at: UnixTime(c.int()?),
    })
}

const MEMBER_COLUMNS: &[&str] = &[
    "character_id",
    "guild_id",
    "rank",
    "class",
    "alias",
    "contributed_gold",
    "contributed_pxp",
    "surrenders",
    "war_id",
    "joined_at",
];

fn read_member(row: &rusqlite::Row<'_>) -> Result<GuildMemberRecord, DbError> {
    let mut c = Cols::new(row, MEMBER_COLUMNS, 0);
    Ok(GuildMemberRecord {
        character_id: CharacterId(c.int()?),
        guild_id: GuildId(c.int()?),
        rank: c.int()?,
        class: c.int()?,
        alias: c.text()?,
        contributed_gold: c.int()?,
        contributed_pxp: c.int()?,
        surrenders: c.int()?,
        war_id: c.int()?,
        joined_at: UnixTime(c.int()?),
    })
}

const LOG_COLUMNS: &[&str] = &[
    "id",
    "guild_id",
    "character_id",
    "kind",
    "item_id",
    "upgrade_level",
    "count",
    "at",
];

fn read_log(row: &rusqlite::Row<'_>) -> Result<GuildBankLogRecord, DbError> {
    let mut c = Cols::new(row, LOG_COLUMNS, 0);
    let id = c.int()?;
    let guild_id = GuildId(c.int()?);
    let character_id = CharacterId(c.int()?);
    let kind_text = c.text()?;
    let kind = GuildBankLogKind::parse(&kind_text)
        .ok_or_else(|| bug(format!("guild_bank_log.kind '{kind_text}' of row {id}")))?;
    Ok(GuildBankLogRecord {
        id,
        guild_id,
        character_id,
        kind,
        item_id: c.int()?,
        upgrade_level: c.int()?,
        count: c.int()?,
        at: UnixTime(c.int()?),
    })
}

/// Every guild with its roster, bank, log tail and quests, ordered by id (`StartupState.guilds`).
///
/// Set-based: one query per table, grouped in memory (no per-guild queries). The log tail is the newest
/// [`GUILD_LOG_TAIL`] rows of each of the kinds `A`, `D`, `I`, `O` per guild (an ordered scan of `guild_bank_log_view`,
/// cut in memory).
/// A guild without a `guild_banks` row gets an empty bank; a bank is repaired with
/// [`GuildBankDoc::validate_and_repair`] (the notes are returned and logged); a bank document that cannot be parsed
/// is logged with its text and replaced by an empty bank, a newer document version refuses the load. A guild without
/// a master is logged at error and kept.
pub fn load_guilds(conn: &Connection) -> Result<Vec<GuildLoad>, DbError> {
    let mut guilds: Vec<GuildRecord> = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT id, name, level, gold, pxp, logo, notice, rank_powers, rank_salaries, war_wins,
                    war_losses, war_surrenders, war_points, created_at FROM guilds ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            guilds.push(read_guild(row)?);
        }
    }

    let mut members: BTreeMap<u32, Vec<GuildMemberRecord>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT character_id, guild_id, rank, class, alias, contributed_gold, contributed_pxp,
                    surrenders, war_id, joined_at
             FROM guild_members ORDER BY guild_id, rank, joined_at, character_id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let m = read_member(row)?;
            members.entry(m.guild_id.0).or_default().push(m);
        }
    }

    let mut banks: BTreeMap<u32, String> = BTreeMap::new();
    {
        let mut stmt = conn.prepare_cached("SELECT guild_id, container FROM guild_banks")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let text: String = row.get(1)?;
            banks.insert(u32_of(id, "guild_banks.guild_id")?, text);
        }
    }

    // the log tail: ONE ordered scan of `guild_bank_log_view` (guild, kind, newest first); the cut to the newest
    // `GUILD_LOG_TAIL` rows per (guild, kind) is made while reading, so a row beyond the cut costs two column reads.
    // (A `ROW_NUMBER()` window over the same index needs three times as long: 170-200 ms against 60 ms for 200 000
    // rows, and the 500-guild budget of WP-DB-11 is 200 ms for the whole startup state.)
    let mut tails: BTreeMap<u32, GuildBankLogTail> = BTreeMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT id, guild_id, character_id, kind, item_id, upgrade_level, count, at FROM guild_bank_log
             WHERE kind IN ('A', 'D', 'I', 'O') ORDER BY guild_id, kind, id DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut group: Option<(i64, String)> = None;
        let mut taken = 0_usize;
        while let Some(row) = rows.next()? {
            let guild_col: i64 = row.get(1)?;
            let kind_col = row.get_ref(3)?.as_str().map_err(rusqlite::Error::from)?;
            match &group {
                Some((g, k)) if *g == guild_col && k == kind_col => taken += 1,
                _ => {
                    group = Some((guild_col, kind_col.to_owned()));
                    taken = 1;
                }
            }
            if taken > GUILD_LOG_TAIL {
                continue;
            }
            let rec = read_log(row)?;
            // newest first within a kind: the rows arrive with `id DESC`, so append instead of `push`
            let tail = tails.entry(rec.guild_id.0).or_default();
            let list = match rec.kind {
                GuildBankLogKind::ItemPut => &mut tail.put,
                GuildBankLogKind::ItemTaken => &mut tail.taken,
                GuildBankLogKind::PenyaIn => &mut tail.penya_in,
                _ => &mut tail.penya_out,
            };
            list.push(rec);
        }
    }

    let mut quests: BTreeMap<u32, Vec<GuildQuestRecord>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT guild_id, quest_id, state, started_at, ended_at FROM guild_quests
             ORDER BY guild_id, quest_id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(
                row,
                &["guild_id", "quest_id", "state", "started_at", "ended_at"],
                0,
            );
            let q = GuildQuestRecord {
                guild_id: GuildId(c.int()?),
                quest_id: c.int()?,
                state: c.int()?,
                started_at: UnixTime(c.int()?),
                ended_at: c.opt_int::<i64>()?.map(UnixTime),
            };
            quests.entry(q.guild_id.0).or_default().push(q);
        }
    }

    let mut out = Vec::with_capacity(guilds.len());
    for guild in guilds {
        let id = guild.id.0;
        let roster = members.remove(&id).unwrap_or_default();
        let mut repairs = Vec::new();
        let bank = match banks.remove(&id) {
            None => GuildBankDoc::empty(),
            Some(text) => match GuildBankDoc::from_json(&text) {
                Ok(mut doc) => {
                    repairs = doc.validate_and_repair();
                    for note in &repairs {
                        tracing::warn!(guild = id, ?note, "guild bank repaired at load");
                    }
                    doc
                }
                Err(e) if e.kind == DbErrorKind::NewerSchema => return Err(e),
                Err(e) => {
                    let raw: String = text.chars().take(512).collect();
                    tracing::error!(guild = id, error = %e, raw, "guild bank is unreadable: the guild starts with an empty bank");
                    GuildBankDoc::empty()
                }
            },
        };
        let load = GuildLoad {
            guild,
            members: roster,
            bank,
            repairs,
            log_tail: tails.remove(&id).unwrap_or_default(),
            quests: quests.remove(&id).unwrap_or_default(),
        };
        if load.master().is_none() {
            tracing::error!(
                guild = id,
                members = load.members.len(),
                "guild has no master (rank {RANK_MASTER}); it is kept"
            );
        }
        out.push(load);
    }
    Ok(out)
}

/// The wars without a result, ordered by id, whose two guilds are in `known` (a war whose guild vanished is logged
/// and skipped).
pub fn load_running_wars(
    conn: &Connection,
    known: &BTreeSet<u32>,
) -> Result<Vec<GuildWarRecord>, DbError> {
    let mut stmt =
        conn.prepare_cached(&format!("{WAR_SELECT} WHERE result IS NULL ORDER BY id"))?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let war = read_war(row)?;
        if known.contains(&war.declarer.0) && known.contains(&war.acceptor.0) {
            out.push(war);
        } else {
            tracing::error!(
                war = war.id.0,
                declarer = war.declarer.0,
                acceptor = war.acceptor.0,
                "running guild war skipped: one of its guilds does not exist any more"
            );
        }
    }
    Ok(out)
}

/// The rejoin cooldowns that end after `now`, by character.
pub fn load_rejoin_cooldowns(
    conn: &Connection,
    now: UnixTime,
) -> Result<Vec<(CharacterId, UnixTime)>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT character_id, until FROM guild_rejoin_cooldowns WHERE until > ?1 ORDER BY character_id",
    )?;
    let mut rows = stmt.query([now.0])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut c = Cols::new(row, &["character_id", "until"], 0);
        out.push((CharacterId(c.int()?), UnixTime(c.int()?)));
    }
    Ok(out)
}

/// The raw end of a character's rejoin cooldown (`CharacterLoad.guild_rejoin_until`).
pub fn rejoin_until(
    conn: &Connection,
    character: CharacterId,
) -> Result<Option<UnixTime>, DbError> {
    Ok(conn
        .prepare_cached("SELECT until FROM guild_rejoin_cooldowns WHERE character_id = ?1")?
        .query_row([character.0], |r| r.get::<_, i64>(0))
        .optional()?
        .map(UnixTime))
}

/// The guild of a character, if any (tests, admin tools).
pub fn guild_of(conn: &Connection, character: CharacterId) -> Result<Option<GuildId>, DbError> {
    conn.prepare_cached("SELECT guild_id FROM guild_members WHERE character_id = ?1")?
        .query_row([character.0], |r| r.get::<_, i64>(0))
        .optional()?
        .map(|id| u32_of(id, "guild_members.guild_id").map(GuildId))
        .transpose()
}

/// A page of the bank log of one guild, newest first, optionally of one kind and only rows with `id < before_id`:
/// the log viewer of the admin drawer (Plan/25 §6.1; the in-game `GuildLogViewReq` is served from the in-memory tail).
/// `limit` 0 = 100, at most 500.
pub fn bank_log_page(
    conn: &Connection,
    guild: GuildId,
    kind: Option<GuildBankLogKind>,
    before_id: Option<i64>,
    limit: u32,
) -> Result<Vec<GuildBankLogRecord>, DbError> {
    let limit = page_limit(limit)?;
    let mut stmt = conn.prepare_cached(
        "SELECT id, guild_id, character_id, kind, item_id, upgrade_level, count, at FROM guild_bank_log
         WHERE guild_id = ?1 AND (?2 IS NULL OR kind = ?2) AND id < ?3
         ORDER BY id DESC LIMIT ?4",
    )?;
    let mut rows = stmt.query(params![
        guild.0,
        kind.map(GuildBankLogKind::as_str),
        before_id.unwrap_or(i64::MAX),
        limit
    ])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_log(row)?);
    }
    Ok(out)
}
