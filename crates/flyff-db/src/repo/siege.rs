//! SQL of the siege and GC 1:1 group (migration `0005`, Plan/13 §10, §16.3, §16.9; rules in Plan/21 §7-§8).
//!
//! **Bids and refunds are money.** A cancel mails 80 % of the bid, a claim pays the purse, a re-tender takes the
//! difference: each of them is one atomic batch that carries a snapshot or a mail next to the row. If the row write
//! were allowed twice, the second batch would pay twice. The upserts therefore enforce the transition tables below;
//! an illegal edge is `Conflict`, which rolls back the **whole batch including its snapshots and mails** (so a second
//! cancel can never mail a second refund and a second claim can never pay twice).
//!
//! | row | legal edges (`old -> new`; an unchanged status is a rewrite of the other fields) |
//! |---|---|
//! | siege | `10 -> 10/20/30`, `20 -> 20/30`, `30 -> 30`; a new row may start in any status |
//! | bid | new row only as `10`; `10/12/20 -> 10/12` only with a **higher** `bid`; `10/12/20 -> 11` (cancel); `11 -> 12` (apply again); `10/12 -> 20`; `20 -> 20`; `10/12/20 -> 30/31`; `30 -> 30`; `30 -> 31` (claim) or `32`; `11`, `31` and `32` are terminal (no rewrite, not even an identical one) |
//! | player | a new row in any of `30/31/32`; `30 -> 30/31/32`; `31` and `32` are terminal |
//! | GC 1:1 tender | new row only as `T`; `T -> T` only with a **higher** `penya`; `T -> F/C/E`; `F -> N/G`; `C -> T` (a cancelled guild may tender again, one row per round and guild); everything else |
//!
//! Failure mapping: an unknown siege is `NotFound` (the `FOREIGN KEY` of a bid or player), an illegal edge `Conflict`,
//! a character twice in a line-up `Invalid`.

use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{
    BidStatus, Gc1to1Load, Gc1to1TenderRecord, GuildSiegeBidRecord, GuildSiegePlayerRecord,
    GuildSiegeRecord, PlayerRewardStatus, SiegeLoad, SiegeRankRow, SiegeStatus, TenderState,
};
use crate::repo::characters::Cols;
use flyff_common::{CharacterId, GuildId, UnixTime};
use rusqlite::{Connection, params};
use std::collections::BTreeSet;

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
            format!("{what}: the siege does not exist"),
        )
    } else if is_constraint(&e, "UNIQUE", "") {
        conflict(format!("{what}: a row with that key exists"))
    } else {
        e.into()
    }
}

// ---- the transition guards (pure, shared by the ops and the tests) -----------------------------------------------

/// The guard of a bid row: may a row currently in `old` (status and bid) be rewritten as `new`? `Err` carries the
/// reason (it becomes the `Conflict`). See the table in the module docs.
pub fn check_bid_transition(
    old: Option<(BidStatus, i64)>,
    new: &GuildSiegeBidRecord,
) -> Result<(), String> {
    let Some((from, old_bid)) = old else {
        return if new.status == BidStatus::Applied {
            Ok(())
        } else {
            Err(format!(
                "a new bid row starts as status 10 (applied), not {}",
                new.status.code()
            ))
        };
    };
    let to = new.status;
    let ok = match (from, to) {
        (
            BidStatus::Applied | BidStatus::Reapplied | BidStatus::Fighting,
            BidStatus::Applied | BidStatus::Reapplied,
        ) => {
            return if new.bid > old_bid {
                Ok(())
            } else {
                Err(format!(
                    "a raised bid must be higher than {old_bid}, not {}",
                    new.bid
                ))
            };
        }
        (BidStatus::Applied | BidStatus::Reapplied | BidStatus::Fighting, BidStatus::Cancelled) => {
            true
        }
        (BidStatus::Cancelled, BidStatus::Reapplied) => true,
        (BidStatus::Applied | BidStatus::Reapplied, BidStatus::Fighting) => true,
        (BidStatus::Fighting, BidStatus::Fighting) => true,
        (
            BidStatus::Applied | BidStatus::Reapplied | BidStatus::Fighting,
            BidStatus::Graded | BidStatus::Paid,
        ) => true,
        (BidStatus::Graded, BidStatus::Graded | BidStatus::Paid | BidStatus::Expired) => true,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "a bid in status {} cannot become {}{}",
            from.code(),
            to.code(),
            if from.is_terminal() || from == BidStatus::Cancelled {
                " (it is final: a second cancel or claim would pay twice)"
            } else {
                ""
            }
        ))
    }
}

/// The guard of a siege-player row (reward status 30 -> 30/31/32, 31 and 32 are terminal).
pub fn check_player_transition(
    old: Option<PlayerRewardStatus>,
    new: PlayerRewardStatus,
) -> Result<(), String> {
    match old {
        None | Some(PlayerRewardStatus::Pending) => Ok(()),
        Some(from) => Err(format!(
            "a player reward in status {} is final and cannot become {}",
            from.code(),
            new.code()
        )),
    }
}

/// The guard of a siege row: the status only moves forward (10 -> 20 -> 30, 10 -> 30).
pub fn check_siege_transition(old: Option<SiegeStatus>, new: SiegeStatus) -> Result<(), String> {
    match old {
        Some(from) if !from.can_become(new) => Err(format!(
            "a siege in status {} cannot go back to {}",
            from.code(),
            new.code()
        )),
        _ => Ok(()),
    }
}

/// The guard of a GC 1:1 tender row (21 §8.3 plus `C -> T`).
pub fn check_tender_transition(
    old: Option<(TenderState, i64)>,
    new: &Gc1to1TenderRecord,
) -> Result<(), String> {
    let Some((from, old_penya)) = old else {
        return if new.state == TenderState::Tendered {
            Ok(())
        } else {
            Err(format!(
                "a new tender row starts as T, not {}",
                new.state.code()
            ))
        };
    };
    let to = new.state;
    match (from, to) {
        (TenderState::Tendered, TenderState::Tendered) => {
            if new.penya > old_penya {
                Ok(())
            } else {
                Err(format!(
                    "a re-tender must be higher than {old_penya}, not {}",
                    new.penya
                ))
            }
        }
        (
            TenderState::Tendered,
            TenderState::Failed | TenderState::Cancelled | TenderState::Ended,
        )
        | (TenderState::Failed, TenderState::NotClaimed | TenderState::Given)
        | (TenderState::Cancelled, TenderState::Tendered) => Ok(()),
        _ => Err(format!(
            "a tender in state {} cannot become {}",
            from.code(),
            to.code()
        )),
    }
}

// ---- sieges -----------------------------------------------------------------------------------------------------

const SIEGE_COLUMNS: &[&str] = &[
    "id",
    "status",
    "opened_at",
    "started_at",
    "ended_at",
    "winner_guild_id",
    "winner_streak",
    "best_player_id",
];

const SIEGE_SELECT: &str = "SELECT id, status, opened_at, started_at, ended_at, winner_guild_id, \
     winner_streak, best_player_id FROM guild_sieges";

fn read_siege(row: &rusqlite::Row<'_>) -> Result<GuildSiegeRecord, DbError> {
    let mut c = Cols::new(row, SIEGE_COLUMNS, 0);
    let id: u32 = c.int()?;
    let code: u8 = c.int()?;
    Ok(GuildSiegeRecord {
        id,
        status: SiegeStatus::from_code(code)
            .ok_or_else(|| bug(format!("guild_sieges.status {code} of siege {id}")))?,
        opened_at: UnixTime(c.int()?),
        started_at: c.opt_int::<i64>()?.map(UnixTime),
        ended_at: c.opt_int::<i64>()?.map(UnixTime),
        winner_guild_id: c.opt_int()?.map(GuildId),
        winner_streak: c.int()?,
        best_player_id: c.opt_int()?.map(CharacterId),
    })
}

/// One siege by id.
pub fn siege_by_id(conn: &Connection, id: u32) -> Result<Option<GuildSiegeRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{SIEGE_SELECT} WHERE id = ?1"))?;
    let mut rows = stmt.query([id])?;
    rows.next()?.map(read_siege).transpose()
}

/// `UpsertGuildSiege`: inserts the siege row or rewrites status, start, end, winner, streak and best player
/// (`opened_at` is kept). The status only moves forward; a step back is `Conflict`.
pub fn upsert_siege(conn: &Connection, s: &GuildSiegeRecord) -> Result<(), DbError> {
    let old = siege_by_id(conn, s.id)?.map(|r| r.status);
    check_siege_transition(old, s.status)
        .map_err(|why| conflict(format!("UpsertGuildSiege {}: {why}", s.id)))?;
    conn.prepare_cached(
        "INSERT INTO guild_sieges (id, status, opened_at, started_at, ended_at, winner_guild_id,
                                   winner_streak, best_player_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
           status = excluded.status, started_at = excluded.started_at, ended_at = excluded.ended_at,
           winner_guild_id = excluded.winner_guild_id, winner_streak = excluded.winner_streak,
           best_player_id = excluded.best_player_id",
    )?
    .execute(params![
        s.id,
        s.status.code(),
        s.opened_at.0,
        s.started_at.map(|t| t.0),
        s.ended_at.map(|t| t.0),
        s.winner_guild_id.map(|g| g.0),
        s.winner_streak,
        s.best_player_id.map(|c| c.0),
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("UpsertGuildSiege {}", s.id)))
}

// ---- bids -------------------------------------------------------------------------------------------------------

const BID_COLUMNS: &[&str] = &[
    "siege_id",
    "guild_id",
    "status",
    "bid",
    "refund",
    "reward",
    "points",
    "straight_wins",
    "applied_at",
    "cancelled_at",
    "rewarded_at",
];

const BID_SELECT: &str = "SELECT siege_id, guild_id, status, bid, refund, reward, points, straight_wins, \
     applied_at, cancelled_at, rewarded_at FROM guild_siege_bids";

fn read_bid(row: &rusqlite::Row<'_>) -> Result<GuildSiegeBidRecord, DbError> {
    let mut c = Cols::new(row, BID_COLUMNS, 0);
    let siege_id: u32 = c.int()?;
    let guild_id = GuildId(c.int()?);
    let code: u8 = c.int()?;
    Ok(GuildSiegeBidRecord {
        siege_id,
        guild_id,
        status: BidStatus::from_code(code).ok_or_else(|| {
            bug(format!(
                "guild_siege_bids.status {code} of siege {siege_id} guild {guild_id}"
            ))
        })?,
        bid: c.int()?,
        refund: c.int()?,
        reward: c.int()?,
        points: c.int()?,
        straight_wins: c.int()?,
        applied_at: UnixTime(c.int()?),
        cancelled_at: c.opt_int::<i64>()?.map(UnixTime),
        rewarded_at: c.opt_int::<i64>()?.map(UnixTime),
    })
}

/// One bid by siege and guild.
pub fn bid_by_key(
    conn: &Connection,
    siege_id: u32,
    guild: GuildId,
) -> Result<Option<GuildSiegeBidRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{BID_SELECT} WHERE siege_id = ?1 AND guild_id = ?2"
    ))?;
    let mut rows = stmt.query(params![siege_id, guild.0])?;
    rows.next()?.map(read_bid).transpose()
}

/// `UpsertGuildSiegeBid`: inserts the bid or rewrites the row, subject to [`check_bid_transition`] (`Conflict`
/// otherwise). `NotFound` for an unknown siege.
pub fn upsert_bid(conn: &Connection, b: &GuildSiegeBidRecord) -> Result<(), DbError> {
    let old = bid_by_key(conn, b.siege_id, b.guild_id)?.map(|r| (r.status, r.bid));
    check_bid_transition(old, b).map_err(|why| {
        conflict(format!(
            "UpsertGuildSiegeBid siege {} guild {}: {why}",
            b.siege_id, b.guild_id
        ))
    })?;
    conn.prepare_cached(
        "INSERT INTO guild_siege_bids (siege_id, guild_id, status, bid, refund, reward, points,
                                       straight_wins, applied_at, cancelled_at, rewarded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(siege_id, guild_id) DO UPDATE SET
           status = excluded.status, bid = excluded.bid, refund = excluded.refund,
           reward = excluded.reward, points = excluded.points, straight_wins = excluded.straight_wins,
           applied_at = excluded.applied_at, cancelled_at = excluded.cancelled_at,
           rewarded_at = excluded.rewarded_at",
    )?
    .execute(params![
        b.siege_id,
        b.guild_id.0,
        b.status.code(),
        b.bid,
        b.refund,
        b.reward,
        b.points,
        b.straight_wins,
        b.applied_at.0,
        b.cancelled_at.map(|t| t.0),
        b.rewarded_at.map(|t| t.0),
    ])
    .map(|_| ())
    .map_err(|e| {
        map_write_error(
            e,
            &format!("UpsertGuildSiegeBid siege {} guild {}", b.siege_id, b.guild_id),
        )
    })
}

// ---- players ----------------------------------------------------------------------------------------------------

const PLAYER_COLUMNS: &[&str] = &[
    "siege_id",
    "guild_id",
    "character_id",
    "status",
    "points",
    "reward",
    "rewarded_at",
];

const PLAYER_SELECT: &str = "SELECT siege_id, guild_id, character_id, status, points, reward, rewarded_at \
     FROM guild_siege_players";

fn read_player(row: &rusqlite::Row<'_>) -> Result<GuildSiegePlayerRecord, DbError> {
    let mut c = Cols::new(row, PLAYER_COLUMNS, 0);
    let siege_id: u32 = c.int()?;
    let guild_id = GuildId(c.int()?);
    let character_id = CharacterId(c.int()?);
    let code: u8 = c.int()?;
    Ok(GuildSiegePlayerRecord {
        siege_id,
        guild_id,
        character_id,
        status: PlayerRewardStatus::from_code(code).ok_or_else(|| {
            bug(format!(
                "guild_siege_players.status {code} of siege {siege_id} player {character_id}"
            ))
        })?,
        points: c.int()?,
        reward: c.int()?,
        rewarded_at: c.opt_int::<i64>()?.map(UnixTime),
    })
}

/// One line-up row by siege, guild and character.
pub fn player_by_key(
    conn: &Connection,
    siege_id: u32,
    guild: GuildId,
    character: CharacterId,
) -> Result<Option<GuildSiegePlayerRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{PLAYER_SELECT} WHERE siege_id = ?1 AND guild_id = ?2 AND character_id = ?3"
    ))?;
    let mut rows = stmt.query(params![siege_id, guild.0, character.0])?;
    rows.next()?.map(read_player).transpose()
}

/// `UpsertGuildSiegePlayer`: inserts the line-up row or rewrites it, subject to [`check_player_transition`].
pub fn upsert_player(conn: &Connection, p: &GuildSiegePlayerRecord) -> Result<(), DbError> {
    let old = player_by_key(conn, p.siege_id, p.guild_id, p.character_id)?.map(|r| r.status);
    check_player_transition(old, p.status).map_err(|why| {
        conflict(format!(
            "UpsertGuildSiegePlayer siege {} player {}: {why}",
            p.siege_id, p.character_id
        ))
    })?;
    conn.prepare_cached(
        "INSERT INTO guild_siege_players (siege_id, guild_id, character_id, status, points, reward, rewarded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(siege_id, guild_id, character_id) DO UPDATE SET
           status = excluded.status, points = excluded.points, reward = excluded.reward,
           rewarded_at = excluded.rewarded_at",
    )?
    .execute(params![
        p.siege_id,
        p.guild_id.0,
        p.character_id.0,
        p.status.code(),
        p.points,
        p.reward,
        p.rewarded_at.map(|t| t.0),
    ])
    .map(|_| ())
    .map_err(|e| {
        map_write_error(
            e,
            &format!(
                "UpsertGuildSiegePlayer siege {} player {}",
                p.siege_id, p.character_id
            ),
        )
    })
}

/// `ExpireSiegeRewards` (`uspExpireCombatReward`): every bid and player row in status 30 of a siege older than
/// `before_siege_id` becomes 32 (expired, `rewarded_at = at`). Idempotent; returns the number of rows changed.
pub fn expire_rewards(
    conn: &Connection,
    before_siege_id: u32,
    at: UnixTime,
) -> Result<u32, DbError> {
    let bids = conn
        .prepare_cached(
            "UPDATE guild_siege_bids SET status = 32, rewarded_at = ?2 WHERE status = 30 AND siege_id < ?1",
        )?
        .execute(params![before_siege_id, at.0])?;
    let players = conn
        .prepare_cached(
            "UPDATE guild_siege_players SET status = 32, rewarded_at = ?2 WHERE status = 30 AND siege_id < ?1",
        )?
        .execute(params![before_siege_id, at.0])?;
    Ok(u32::try_from(bids + players).unwrap_or(u32::MAX))
}

// ---- GC 1:1 -----------------------------------------------------------------------------------------------------

const TENDER_COLUMNS: &[&str] = &["round", "guild_id", "penya", "state", "tendered_at"];

const TENDER_SELECT: &str = "SELECT round, guild_id, penya, state, tendered_at FROM gc1to1_tenders";

fn read_tender(row: &rusqlite::Row<'_>) -> Result<Gc1to1TenderRecord, DbError> {
    let mut c = Cols::new(row, TENDER_COLUMNS, 0);
    let round: u32 = c.int()?;
    let guild_id = GuildId(c.int()?);
    let penya = c.int()?;
    let state_text = c.text()?;
    Ok(Gc1to1TenderRecord {
        round,
        guild_id,
        penya,
        state: TenderState::parse(&state_text).ok_or_else(|| {
            bug(format!(
                "gc1to1_tenders.state '{state_text}' of round {round} guild {guild_id}"
            ))
        })?,
        tendered_at: UnixTime(c.int()?),
    })
}

/// One tender by round and guild.
pub fn tender_by_key(
    conn: &Connection,
    round: u32,
    guild: GuildId,
) -> Result<Option<Gc1to1TenderRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{TENDER_SELECT} WHERE round = ?1 AND guild_id = ?2"
    ))?;
    let mut rows = stmt.query(params![round, guild.0])?;
    rows.next()?.map(read_tender).transpose()
}

/// `UpsertGc1to1Tender`: inserts the tender or rewrites it, subject to [`check_tender_transition`].
pub fn upsert_tender(conn: &Connection, t: &Gc1to1TenderRecord) -> Result<(), DbError> {
    let old = tender_by_key(conn, t.round, t.guild_id)?.map(|r| (r.state, r.penya));
    check_tender_transition(old, t).map_err(|why| {
        conflict(format!(
            "UpsertGc1to1Tender round {} guild {}: {why}",
            t.round, t.guild_id
        ))
    })?;
    conn.prepare_cached(
        "INSERT INTO gc1to1_tenders (round, guild_id, penya, state, tendered_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(round, guild_id) DO UPDATE SET
           penya = excluded.penya, state = excluded.state, tendered_at = excluded.tendered_at",
    )?
    .execute(params![
        t.round,
        t.guild_id.0,
        t.penya,
        t.state.as_str(),
        t.tendered_at.0
    ])
    .map(|_| ())
    .map_err(|e| {
        map_write_error(
            e,
            &format!("UpsertGc1to1Tender round {} guild {}", t.round, t.guild_id),
        )
    })
}

/// `ReplaceGc1to1Lineup`: deletes the line-up of `(round, guild)` and stores `members` as `seq` 0..n (the fight
/// order). A character twice is `Invalid` (nothing is deleted then).
pub fn replace_lineup(
    conn: &Connection,
    round: u32,
    guild: GuildId,
    members: &[CharacterId],
) -> Result<(), DbError> {
    let mut seen = BTreeSet::new();
    if let Some(twice) = members.iter().find(|m| !seen.insert(m.0)) {
        return Err(DbError::new(
            DbErrorKind::Invalid,
            format!("ReplaceGc1to1Lineup round {round} guild {guild}: character {twice} twice"),
        ));
    }
    conn.prepare_cached("DELETE FROM gc1to1_lineups WHERE round = ?1 AND guild_id = ?2")?
        .execute(params![round, guild.0])?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO gc1to1_lineups (round, guild_id, seq, character_id) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (seq, member) in members.iter().enumerate() {
        insert.execute(params![round, guild.0, seq as i64, member.0])?;
    }
    Ok(())
}

// ---- reads ------------------------------------------------------------------------------------------------------

/// The cross-siege player ranking (`uspRankGuildCombatPlayer`): `SUM(points)` per character joined with
/// `characters.job`, highest first, then by character id. Rows of purged characters drop out like the legacy join.
/// Re-run after every result.
pub fn player_ranking(conn: &Connection) -> Result<Vec<SiegeRankRow>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.character_id, c.job, SUM(p.points) AS total
         FROM guild_siege_players p JOIN characters c ON c.id = p.character_id
         GROUP BY p.character_id
         ORDER BY total DESC, p.character_id",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut c = Cols::new(row, &["character_id", "job", "points"], 0);
        out.push(SiegeRankRow {
            character_id: CharacterId(c.int()?),
            job: c.int()?,
            points: c.int()?,
        });
    }
    Ok(out)
}

fn bids_where(
    conn: &Connection,
    clause: &str,
    bind: &[u32],
) -> Result<Vec<GuildSiegeBidRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{BID_SELECT} WHERE {clause} ORDER BY siege_id, applied_at, guild_id"
    ))?;
    let mut rows = stmt.query(rusqlite::params_from_iter(bind))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_bid(row)?);
    }
    Ok(out)
}

fn players_where(
    conn: &Connection,
    clause: &str,
    bind: &[u32],
) -> Result<Vec<GuildSiegePlayerRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{PLAYER_SELECT} WHERE {clause} ORDER BY siege_id, guild_id, character_id"
    ))?;
    let mut rows = stmt.query(rusqlite::params_from_iter(bind))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_player(row)?);
    }
    Ok(out)
}

/// The siege state at boot (`StartupState.sieges`, 21 §7.2): the newest siege row of any status with its bids
/// (`applied_at` order) and line-up rows, the unpaid rows (status 30) of older sieges, the newest ended siege and the
/// player ranking.
pub fn load_sieges(conn: &Connection) -> Result<SiegeLoad, DbError> {
    let current = {
        let mut stmt = conn.prepare_cached(&format!("{SIEGE_SELECT} ORDER BY id DESC LIMIT 1"))?;
        let mut rows = stmt.query([])?;
        rows.next()?.map(read_siege).transpose()?
    };
    let last_ended = {
        let mut stmt = conn.prepare_cached(&format!(
            "{SIEGE_SELECT} WHERE status = 30 ORDER BY id DESC LIMIT 1"
        ))?;
        let mut rows = stmt.query([])?;
        rows.next()?.map(read_siege).transpose()?
    };
    let (bids, players, unpaid_bids, unpaid_players) = match &current {
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        Some(cur) => {
            // rows in status 30 belong to ended sieges: "older" than the current one, or the current one itself
            // when it just ended and the next row has not been created yet
            let older = if cur.status == SiegeStatus::Ended {
                u32::MAX
            } else {
                cur.id
            };
            (
                bids_where(conn, "siege_id = ?1", &[cur.id])?,
                players_where(conn, "siege_id = ?1", &[cur.id])?,
                bids_where(conn, "status = 30 AND siege_id < ?1", &[older])?,
                players_where(
                    conn,
                    "status = 30 AND reward > 0 AND siege_id < ?1",
                    &[older],
                )?,
            )
        }
    };
    Ok(SiegeLoad {
        current,
        bids,
        players,
        unpaid_bids,
        unpaid_players,
        last_ended,
        ranking: player_ranking(conn)?,
    })
}

/// The GC 1:1 tenders in state `T` or `F` (by round, `penya` descending, `tendered_at`) with their line-ups
/// (`StartupState.gc1to1`).
pub fn load_gc1to1(conn: &Connection) -> Result<Gc1to1Load, DbError> {
    let mut tenders = Vec::new();
    {
        let mut stmt = conn.prepare_cached(&format!(
            "{TENDER_SELECT} WHERE state IN ('T', 'F') ORDER BY round, penya DESC, tendered_at, guild_id"
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            tenders.push(read_tender(row)?);
        }
    }
    let wanted: BTreeSet<(u32, u32)> = tenders.iter().map(|t| (t.round, t.guild_id.0)).collect();
    let mut lineups: Vec<(u32, GuildId, Vec<CharacterId>)> = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT round, guild_id, character_id FROM gc1to1_lineups ORDER BY round, guild_id, seq",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["round", "guild_id", "character_id"], 0);
            let round: u32 = c.int()?;
            let guild: u32 = c.int()?;
            let character = CharacterId(c.int()?);
            if !wanted.contains(&(round, guild)) {
                continue;
            }
            match lineups.last_mut() {
                Some((r, g, members)) if *r == round && g.0 == guild => members.push(character),
                _ => lineups.push((round, GuildId(guild), vec![character])),
            }
        }
    }
    Ok(Gc1to1Load { tenders, lineups })
}
