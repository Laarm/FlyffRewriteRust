//! SQL of the lord, tax and secret-room group (migration `0006`, Plan/13 §11, §16.3, §16.9; rules in Plan/22
//! §9-§11).
//!
//! **Deposits, tenders and refunds are money.** A refund is a mail in the same batch as the row that makes it due
//! (`LordElectionEnd`, `lord_begin_vote`, `SecretRoomCancel`, `SecretRoomResult`); if the row write were allowed twice,
//! the second batch would refund twice. The upserts therefore enforce the transition tables below; an illegal edge is
//! `Conflict`, which rolls back the **whole batch including its snapshots and mails**.
//!
//! | row | legal edges (`old -> new`; an unchanged state is a rewrite of the other fields) |
//! |---|---|
//! | election | a new row in any state; `0 -> 0/1/2/3`, `1 -> 1/2/3`, `2 -> 2/3`; `3` is terminal (not even an identical rewrite: a second end, a second refund run, is impossible) |
//! | candidate | a new row; then `deposit` and `votes` never decrease, `created_at` never changes, `left_out` goes `false -> true` once (a left-out row is final: no second leave-out refund), and no row of an expired election changes (no raise after the end-of-vote refunds); pledge-only and vote-count rewrites are fine |
//! | vote | one row per (election, elector); a second vote and a vote into an expired election are `Conflict` |
//! | lord event | one row per creator; `exp_factor`, `item_factor`, `created_at` never change; a rewrite must LOWER `ticks_left` (`0 <= new < stored`): a second create of the same creator, even in the same second with the same factors, is `Conflict` and rolls its cost back |
//! | secret-room tender | a new row only as `T`; `T -> T` only with a **higher** `penya`; `T -> C/F/W/L`; `C -> T` and `F -> T` (a guild that cancelled, or was refunded at an open, may tender again in the same round); `W -> W`, `L -> L` (a result rewrite of war state, kills, world); `C -> C` and `F -> F` are refused (no second cancel refund, no second failed-ranking mail); everything else `Conflict` |
//!
//! Failure mapping: an unknown election is `NotFound`, an illegal edge or a second vote `Conflict`, a limit
//! (`Invalid`) is the second line of defence: the game validates first.

use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{
    ElectionState, LORD_PLEDGE_MAX_BYTES, LordCandidateRecord, LordElectionRecord, LordEventRecord,
    LordLoad, LordVoteRecord, SecretRoomLineup, SecretRoomLoad, SecretRoomTenderRecord,
    SecretRoomTenderState, TaxContinent, TaxDetailRecord, TaxKind, TaxLoad, TaxOwnerRecord,
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

/// Maps the constraint failures of a write to the *Expected* kinds: a primary key or unique clash is `Conflict`, a
/// missing parent row `NotFound`.
fn map_write_error(e: rusqlite::Error, what: &str) -> DbError {
    if is_constraint(&e, "FOREIGN KEY", "") {
        DbError::new(
            DbErrorKind::NotFound,
            format!("{what}: the referenced row does not exist"),
        )
    } else if is_constraint(&e, "UNIQUE", "") {
        conflict(format!("{what}: a row with that key exists"))
    } else {
        e.into()
    }
}

// ---- the transition guards (pure, shared by the ops and the tests) -----------------------------------------------

/// The guard of an election row: may a row currently in `old` be rewritten as `new`? See the table in the module
/// docs. `Err` carries the reason (it becomes the `Conflict`).
pub fn check_election_transition(
    old: Option<ElectionState>,
    new: ElectionState,
) -> Result<(), String> {
    let Some(from) = old else {
        return Ok(());
    };
    match from {
        ElectionState::Expired => Err(format!(
            "an election in state 3 is final and cannot be rewritten as {} (a second end would refund twice)",
            new.code()
        )),
        _ if new < from => Err(format!(
            "an election in state {} cannot go back to {}",
            from.code(),
            new.code()
        )),
        _ => Ok(()),
    }
}

/// The guard of a candidate row: `election` is the state of its election, `old` the stored row, `new` the rewrite.
pub fn check_candidate_transition(
    election: ElectionState,
    old: Option<&LordCandidateRecord>,
    new: &LordCandidateRecord,
) -> Result<(), String> {
    if election == ElectionState::Expired {
        return Err(
            "the election has ended: its candidate rows are frozen (no raise after the refunds)"
                .to_owned(),
        );
    }
    let Some(old) = old else {
        return Ok(());
    };
    if old.left_out {
        return Err(
            "the candidate is left out: the row is final (a second leave-out would refund twice)"
                .to_owned(),
        );
    }
    if new.deposit < old.deposit {
        return Err(format!(
            "the deposit never decreases ({} -> {})",
            old.deposit, new.deposit
        ));
    }
    if new.votes < old.votes {
        return Err(format!(
            "the votes never decrease ({} -> {})",
            old.votes, new.votes
        ));
    }
    if new.created_at != old.created_at {
        return Err("created_at (the tie-break of equal votes) never changes".to_owned());
    }
    Ok(())
}

/// The guard of a lord event row: one running component per creator; only the remaining ticks change, and they
/// must go DOWN.
pub fn check_event_transition(
    old: Option<&LordEventRecord>,
    new: &LordEventRecord,
) -> Result<(), String> {
    let Some(old) = old else {
        return Ok(());
    };
    if old.exp_factor != new.exp_factor
        || old.item_factor != new.item_factor
        || old.created_at != new.created_at
    {
        return Err(
            "the creator already runs a component (factors and created_at never change)".to_owned(),
        );
    }
    if new.ticks_left >= old.ticks_left {
        return Err(format!(
            "the remaining ticks must go down ({} -> {})",
            old.ticks_left, new.ticks_left
        ));
    }
    Ok(())
}

/// The guard of a secret-room tender row (22 §11.2 plus `C -> T` and `F -> T`).
pub fn check_secret_room_tender_transition(
    old: Option<(SecretRoomTenderState, i64)>,
    new: &SecretRoomTenderRecord,
) -> Result<(), String> {
    use SecretRoomTenderState::{Cancelled, Failed, Lost, Tendered, Won};
    let Some((from, old_penya)) = old else {
        return if new.state == Tendered {
            Ok(())
        } else {
            Err(format!(
                "a new tender row starts as T, not {}",
                new.state.code()
            ))
        };
    };
    match (from, new.state) {
        (Tendered, Tendered) => {
            if new.penya > old_penya {
                Ok(())
            } else {
                Err(format!(
                    "a raised tender must be higher than {old_penya}, not {}",
                    new.penya
                ))
            }
        }
        (Tendered, Cancelled | Failed | Won | Lost)
        | (Cancelled | Failed, Tendered)
        | (Won, Won)
        | (Lost, Lost) => Ok(()),
        (from, to) => Err(format!(
            "a tender in state {} cannot become {}{}",
            from.code(),
            to.code(),
            if from == to && matches!(from, Cancelled | Failed) {
                " (it was refunded: a second refund would pay twice)"
            } else {
                ""
            }
        )),
    }
}

// ---- elections --------------------------------------------------------------------------------------------------

const ELECTION_COLUMNS: &[&str] = &[
    "id",
    "state",
    "begin_at",
    "requirement",
    "lord_id",
    "ended_at",
];

const ELECTION_SELECT: &str =
    "SELECT id, state, begin_at, requirement, lord_id, ended_at FROM lord_elections";

fn read_election(row: &rusqlite::Row<'_>) -> Result<LordElectionRecord, DbError> {
    let mut c = Cols::new(row, ELECTION_COLUMNS, 0);
    let id: u32 = c.int()?;
    let code: u8 = c.int()?;
    Ok(LordElectionRecord {
        id,
        state: ElectionState::from_code(code)
            .ok_or_else(|| bug(format!("lord_elections.state {code} of election {id}")))?,
        begin_at: UnixTime(c.int()?),
        requirement: c.int()?,
        lord_id: c.opt_int()?.map(CharacterId),
        ended_at: c.opt_int::<i64>()?.map(UnixTime),
    })
}

/// One election by id.
pub fn election_by_id(conn: &Connection, id: u32) -> Result<Option<LordElectionRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{ELECTION_SELECT} WHERE id = ?1"))?;
    let mut rows = stmt.query([id])?;
    rows.next()?.map(read_election).transpose()
}

fn validate_election(e: &LordElectionRecord) -> Result<(), DbError> {
    if e.state != ElectionState::Expired && (e.lord_id.is_some() || e.ended_at.is_some()) {
        return Err(invalid(format!(
            "UpsertLordElection {}: lord_id and ended_at belong to state 3, not {}",
            e.id,
            e.state.code()
        )));
    }
    if e.begin_at.0 < 0 {
        return Err(invalid(format!(
            "UpsertLordElection {}: negative begin_at {}",
            e.id, e.begin_at.0
        )));
    }
    if e.requirement < 0 {
        return Err(invalid(format!(
            "UpsertLordElection {}: negative requirement {}",
            e.id, e.requirement
        )));
    }
    Ok(())
}

/// `UpsertLordElection`: inserts the election or rewrites state, begin, requirement, winner and end, subject to
/// [`check_election_transition`] (`Conflict` otherwise). `Invalid` for a winner or an end time outside state 3 and
/// negative begin or requirement.
pub fn upsert_election(conn: &Connection, e: &LordElectionRecord) -> Result<(), DbError> {
    validate_election(e)?;
    let old = election_by_id(conn, e.id)?.map(|r| r.state);
    check_election_transition(old, e.state)
        .map_err(|why| conflict(format!("UpsertLordElection {}: {why}", e.id)))?;
    conn.prepare_cached(
        "INSERT INTO lord_elections (id, state, begin_at, requirement, lord_id, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           state = excluded.state, begin_at = excluded.begin_at, requirement = excluded.requirement,
           lord_id = excluded.lord_id, ended_at = excluded.ended_at",
    )?
    .execute(params![
        e.id,
        e.state.code(),
        e.begin_at.0,
        e.requirement,
        e.lord_id.map(|c| c.0),
        e.ended_at.map(|t| t.0),
    ])
    .map(|_| ())
    .map_err(|err| map_write_error(err, &format!("UpsertLordElection {}", e.id)))
}

// ---- candidates and votes ---------------------------------------------------------------------------------------

const CANDIDATE_COLUMNS: &[&str] = &[
    "election_id",
    "character_id",
    "deposit",
    "votes",
    "pledge",
    "left_out",
    "created_at",
];

const CANDIDATE_SELECT: &str = "SELECT election_id, character_id, deposit, votes, pledge, left_out, created_at \
     FROM lord_candidates";

fn read_candidate(row: &rusqlite::Row<'_>) -> Result<LordCandidateRecord, DbError> {
    let mut c = Cols::new(row, CANDIDATE_COLUMNS, 0);
    Ok(LordCandidateRecord {
        election_id: c.int()?,
        character_id: CharacterId(c.int()?),
        deposit: c.int()?,
        votes: c.int()?,
        pledge: c.text()?,
        left_out: c.int::<i64>()? != 0,
        created_at: UnixTime(c.int()?),
    })
}

/// One candidate by election and character.
pub fn candidate_by_key(
    conn: &Connection,
    election: u32,
    character: CharacterId,
) -> Result<Option<LordCandidateRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{CANDIDATE_SELECT} WHERE election_id = ?1 AND character_id = ?2"
    ))?;
    let mut rows = stmt.query(params![election, character.0])?;
    rows.next()?.map(read_candidate).transpose()
}

fn validate_candidate(c: &LordCandidateRecord) -> Result<(), DbError> {
    if c.pledge.len() > LORD_PLEDGE_MAX_BYTES {
        return Err(invalid(format!(
            "UpsertLordCandidate {}: the pledge is longer than {LORD_PLEDGE_MAX_BYTES} bytes",
            c.character_id
        )));
    }
    if c.deposit < 0 || c.votes < 0 {
        return Err(invalid(format!(
            "UpsertLordCandidate {}: negative deposit {} or votes {}",
            c.character_id, c.deposit, c.votes
        )));
    }
    Ok(())
}

/// `UpsertLordCandidate`: inserts the candidate or rewrites deposit, votes, pledge and `left_out` (`created_at` is
/// kept), subject to [`check_candidate_transition`] (`Conflict`). `NotFound` for an unknown election, `Invalid` for a
/// pledge over 255 bytes and a negative deposit or vote count.
pub fn upsert_candidate(conn: &Connection, c: &LordCandidateRecord) -> Result<(), DbError> {
    validate_candidate(c)?;
    let Some(election) = election_by_id(conn, c.election_id)? else {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!(
                "UpsertLordCandidate {}: election {} does not exist",
                c.character_id, c.election_id
            ),
        ));
    };
    let old = candidate_by_key(conn, c.election_id, c.character_id)?;
    check_candidate_transition(election.state, old.as_ref(), c).map_err(|why| {
        conflict(format!(
            "UpsertLordCandidate election {} candidate {}: {why}",
            c.election_id, c.character_id
        ))
    })?;
    conn.prepare_cached(
        "INSERT INTO lord_candidates (election_id, character_id, deposit, votes, pledge, left_out, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(election_id, character_id) DO UPDATE SET
           deposit = excluded.deposit, votes = excluded.votes, pledge = excluded.pledge,
           left_out = excluded.left_out",
    )?
    .execute(params![
        c.election_id,
        c.character_id.0,
        c.deposit,
        c.votes,
        c.pledge,
        i64::from(c.left_out),
        c.created_at.0,
    ])
    .map(|_| ())
    .map_err(|e| {
        map_write_error(
            e,
            &format!(
                "UpsertLordCandidate election {} candidate {}",
                c.election_id, c.character_id
            ),
        )
    })
}

/// `InsertLordVote`: one vote per (election, elector). A second vote is `Conflict` (legacy `uspElectionIncVote`
/// answered `bResult 0`), a vote into an expired election `Conflict`, an unknown election `NotFound`.
pub fn insert_vote(conn: &Connection, v: &LordVoteRecord) -> Result<(), DbError> {
    let Some(election) = election_by_id(conn, v.election_id)? else {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!(
                "InsertLordVote {}: election {} does not exist",
                v.elector_id, v.election_id
            ),
        ));
    };
    if election.state == ElectionState::Expired {
        return Err(conflict(format!(
            "InsertLordVote {}: election {} has ended",
            v.elector_id, v.election_id
        )));
    }
    conn.prepare_cached(
        "INSERT INTO lord_votes (election_id, elector_id, voted_at) VALUES (?1, ?2, ?3)",
    )?
    .execute(params![v.election_id, v.elector_id.0, v.voted_at.0])
    .map(|_| ())
    .map_err(|e| {
        if is_constraint(&e, "UNIQUE", "") {
            conflict(format!(
                "InsertLordVote: elector {} already voted in election {}",
                v.elector_id, v.election_id
            ))
        } else {
            map_write_error(
                e,
                &format!(
                    "InsertLordVote election {} elector {}",
                    v.election_id, v.elector_id
                ),
            )
        }
    })
}

// ---- lord events and skills -------------------------------------------------------------------------------------

const EVENT_COLUMNS: &[&str] = &[
    "character_id",
    "ticks_left",
    "exp_factor",
    "item_factor",
    "created_at",
];

const EVENT_SELECT: &str =
    "SELECT character_id, ticks_left, exp_factor, item_factor, created_at FROM lord_events";

fn read_event(row: &rusqlite::Row<'_>) -> Result<LordEventRecord, DbError> {
    let mut c = Cols::new(row, EVENT_COLUMNS, 0);
    Ok(LordEventRecord {
        character_id: CharacterId(c.int()?),
        ticks_left: c.int()?,
        exp_factor: c.f32()?,
        item_factor: c.f32()?,
        created_at: UnixTime(c.int()?),
    })
}

/// The running lord event of a creator.
pub fn event_by_creator(
    conn: &Connection,
    creator: CharacterId,
) -> Result<Option<LordEventRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{EVENT_SELECT} WHERE character_id = ?1"))?;
    let mut rows = stmt.query([creator.0])?;
    rows.next()?.map(read_event).transpose()
}

fn validate_event(e: &LordEventRecord) -> Result<(), DbError> {
    if e.ticks_left < 0 {
        return Err(invalid(format!(
            "UpsertLordEvent {}: negative ticks {}",
            e.character_id, e.ticks_left
        )));
    }
    for (what, factor) in [("exp_factor", e.exp_factor), ("item_factor", e.item_factor)] {
        if !factor.is_finite() || factor < 0.0 {
            return Err(invalid(format!(
                "UpsertLordEvent {}: {what} {factor} is negative or not finite",
                e.character_id
            )));
        }
    }
    Ok(())
}

/// `UpsertLordEvent`: a new component of a creator, or the minute tick of the running one (`ticks_left` only, and it
/// must go down), subject to [`check_event_transition`] (`Conflict`). `Invalid` for negative ticks and a negative or
/// non-finite factor.
pub fn upsert_event(conn: &Connection, e: &LordEventRecord) -> Result<(), DbError> {
    validate_event(e)?;
    let old = event_by_creator(conn, e.character_id)?;
    check_event_transition(old.as_ref(), e)
        .map_err(|why| conflict(format!("UpsertLordEvent {}: {why}", e.character_id)))?;
    conn.prepare_cached(
        "INSERT INTO lord_events (character_id, ticks_left, exp_factor, item_factor, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(character_id) DO UPDATE SET ticks_left = excluded.ticks_left",
    )?
    .execute(params![
        e.character_id.0,
        e.ticks_left,
        f64::from(e.exp_factor),
        f64::from(e.item_factor),
        e.created_at.0,
    ])
    .map(|_| ())
    .map_err(|err| map_write_error(err, &format!("UpsertLordEvent {}", e.character_id)))
}

/// `DeleteLordEvent`: removes the component of a creator (a row reaching 0 ticks); idempotent. Returns whether a row
/// was deleted.
pub fn delete_event(conn: &Connection, creator: CharacterId) -> Result<bool, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM lord_events WHERE character_id = ?1")?
        .execute([creator.0])?
        > 0)
}

/// `ClearLordEvents` (`/LEventInitialize`): removes every component; returns how many rows were deleted.
pub fn clear_events(conn: &Connection) -> Result<u32, DbError> {
    let n = conn
        .prepare_cached("DELETE FROM lord_events")?
        .execute([])?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `SetLordSkillCooldown`: upserts the cooldown of a lord skill in minutes (`0` = ready). `Invalid` below 0.
pub fn set_skill_cooldown(conn: &Connection, skill_id: u32, minutes: i32) -> Result<(), DbError> {
    if minutes < 0 {
        return Err(invalid(format!(
            "SetLordSkillCooldown {skill_id}: negative cooldown {minutes}"
        )));
    }
    conn.prepare_cached(
        "INSERT INTO lord_skills (skill_id, cooldown_min) VALUES (?1, ?2)
         ON CONFLICT(skill_id) DO UPDATE SET cooldown_min = excluded.cooldown_min",
    )?
    .execute(params![skill_id, minutes])?;
    Ok(())
}

// ---- tax --------------------------------------------------------------------------------------------------------

/// `UpsertTaxOwner`: a whole-row upsert by (round, continent). No guard: the payout guard is the
/// `tax.last_paid_on` date inside the payout batch.
pub fn upsert_tax_owner(conn: &Connection, o: &TaxOwnerRecord) -> Result<(), DbError> {
    conn.prepare_cached(
        "INSERT INTO tax_owners (round, continent, owner_id, next_owner_id, rate_set, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(round, continent) DO UPDATE SET
           owner_id = excluded.owner_id, next_owner_id = excluded.next_owner_id,
           rate_set = excluded.rate_set, changed_at = excluded.changed_at",
    )?
    .execute(params![
        o.round,
        o.continent.code(),
        o.owner_id,
        o.next_owner_id,
        i64::from(o.rate_set),
        o.changed_at.0,
    ])
    .map(|_| ())
    .map_err(|e| {
        map_write_error(
            e,
            &format!(
                "UpsertTaxOwner round {} continent {}",
                o.round,
                o.continent.code()
            ),
        )
    })
}

fn validate_tax_detail(d: &TaxDetailRecord) -> Result<(), DbError> {
    let label = || {
        format!(
            "UpsertTaxDetail round {} continent {} kind {}",
            d.round,
            d.continent.code(),
            d.kind.code()
        )
    };
    if d.continent == TaxContinent::West && d.kind == TaxKind::Admission {
        return Err(invalid(format!(
            "{}: the West has no admission tax",
            label()
        )));
    }
    for (what, rate) in [("rate", d.rate), ("next_rate", d.next_rate)] {
        if !(0..=100).contains(&rate) {
            return Err(invalid(format!(
                "{}: {what} {rate} is outside 0..=100",
                label()
            )));
        }
    }
    for (what, counter) in [
        ("tx_count", d.tx_count),
        ("gold", d.gold),
        ("perin", d.perin),
    ] {
        if counter < 0 {
            return Err(invalid(format!("{}: negative {what} {counter}", label())));
        }
    }
    Ok(())
}

/// `UpsertTaxDetail`: a whole-row upsert by (round, continent, kind). `Invalid` for West + Admission, a rate or next
/// rate outside 0..=100 and a negative count, gold or Perin.
pub fn upsert_tax_detail(conn: &Connection, d: &TaxDetailRecord) -> Result<(), DbError> {
    validate_tax_detail(d)?;
    conn.prepare_cached(
        "INSERT INTO tax_details (round, continent, kind, rate, next_rate, tx_count, gold, perin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(round, continent, kind) DO UPDATE SET
           rate = excluded.rate, next_rate = excluded.next_rate, tx_count = excluded.tx_count,
           gold = excluded.gold, perin = excluded.perin",
    )?
    .execute(params![
        d.round,
        d.continent.code(),
        d.kind.code(),
        d.rate,
        d.next_rate,
        d.tx_count,
        d.gold,
        d.perin,
    ])
    .map(|_| ())
    .map_err(|e| {
        map_write_error(
            e,
            &format!(
                "UpsertTaxDetail round {} continent {} kind {}",
                d.round,
                d.continent.code(),
                d.kind.code()
            ),
        )
    })
}

// ---- secret room ------------------------------------------------------------------------------------------------

const TENDER_COLUMNS: &[&str] = &[
    "round",
    "continent",
    "guild_id",
    "penya",
    "state",
    "world_id",
    "war_state",
    "kills",
    "tendered_at",
];

const TENDER_SELECT: &str = "SELECT round, continent, guild_id, penya, state, world_id, war_state, kills, \
     tendered_at FROM secret_room_tenders";

fn side_of(code: u8, what: &str) -> Result<TaxContinent, DbError> {
    TaxContinent::from_code(code)
        .filter(|c| c.is_side())
        .ok_or_else(|| bug(format!("{what}: continent {code} is neither East nor West")))
}

fn read_tender(row: &rusqlite::Row<'_>) -> Result<SecretRoomTenderRecord, DbError> {
    let mut c = Cols::new(row, TENDER_COLUMNS, 0);
    let round: u32 = c.int()?;
    let continent = side_of(c.int()?, "secret_room_tenders")?;
    let guild_id = GuildId(c.int()?);
    let penya = c.int()?;
    let state_text = c.text()?;
    Ok(SecretRoomTenderRecord {
        round,
        continent,
        guild_id,
        penya,
        state: SecretRoomTenderState::parse(&state_text).ok_or_else(|| {
            bug(format!(
                "secret_room_tenders.state '{state_text}' of round {round} guild {guild_id}"
            ))
        })?,
        world_id: WorldId(c.int()?),
        war_state: c.int()?,
        kills: c.int()?,
        tendered_at: UnixTime(c.int()?),
    })
}

/// One tender by round, side and guild.
pub fn secret_room_tender_by_key(
    conn: &Connection,
    round: u32,
    continent: TaxContinent,
    guild: GuildId,
) -> Result<Option<SecretRoomTenderRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{TENDER_SELECT} WHERE round = ?1 AND continent = ?2 AND guild_id = ?3"
    ))?;
    let mut rows = stmt.query(params![round, continent.code(), guild.0])?;
    rows.next()?.map(read_tender).transpose()
}

/// `UpsertSecretRoomTender`: inserts the tender or rewrites penya, state, world, war state, kills and `tendered_at`,
/// subject to [`check_secret_room_tender_transition`] (`Conflict`). `Invalid` for the continent `All` and a penya of
/// 0 or less.
pub fn upsert_secret_room_tender(
    conn: &Connection,
    t: &SecretRoomTenderRecord,
) -> Result<(), DbError> {
    let label = || {
        format!(
            "UpsertSecretRoomTender round {} continent {} guild {}",
            t.round,
            t.continent.code(),
            t.guild_id
        )
    };
    if !t.continent.is_side() {
        return Err(invalid(format!(
            "{}: a tender is for the East or the West, not 255",
            label()
        )));
    }
    if t.penya <= 0 {
        return Err(invalid(format!(
            "{}: penya {} is not positive",
            label(),
            t.penya
        )));
    }
    let old = secret_room_tender_by_key(conn, t.round, t.continent, t.guild_id)?
        .map(|r| (r.state, r.penya));
    check_secret_room_tender_transition(old, t)
        .map_err(|why| conflict(format!("{}: {why}", label())))?;
    conn.prepare_cached(
        "INSERT INTO secret_room_tenders (round, continent, guild_id, penya, state, world_id, war_state,
                                          kills, tendered_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(round, continent, guild_id) DO UPDATE SET
           penya = excluded.penya, state = excluded.state, world_id = excluded.world_id,
           war_state = excluded.war_state, kills = excluded.kills, tendered_at = excluded.tendered_at",
    )?
    .execute(params![
        t.round,
        t.continent.code(),
        t.guild_id.0,
        t.penya,
        t.state.as_str(),
        t.world_id.0,
        t.war_state,
        t.kills,
        t.tendered_at.0,
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &label()))
}

/// `ReplaceSecretRoomLineup`: deletes the line-up of (round, side, guild) and stores `members`. A character twice or
/// a continent other than 15 and 240 is `Invalid` (nothing is deleted then); an empty list deletes the line-up.
pub fn replace_secret_room_lineup(
    conn: &Connection,
    round: u32,
    continent: u8,
    guild: GuildId,
    members: &[CharacterId],
) -> Result<(), DbError> {
    if !TaxContinent::from_code(continent).is_some_and(TaxContinent::is_side) {
        return Err(invalid(format!(
            "ReplaceSecretRoomLineup round {round} guild {guild}: continent {continent} is neither 15 nor 240"
        )));
    }
    let mut seen = BTreeSet::new();
    if let Some(twice) = members.iter().find(|m| !seen.insert(m.0)) {
        return Err(invalid(format!(
            "ReplaceSecretRoomLineup round {round} guild {guild}: character {twice} twice"
        )));
    }
    conn.prepare_cached(
        "DELETE FROM secret_room_lineups WHERE round = ?1 AND continent = ?2 AND guild_id = ?3",
    )?
    .execute(params![round, continent, guild.0])?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO secret_room_lineups (round, continent, guild_id, character_id)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for member in members {
        insert.execute(params![round, continent, guild.0, member.0])?;
    }
    Ok(())
}

// ---- reads ------------------------------------------------------------------------------------------------------

/// The lord state at boot (`StartupState.lord`, 22 §9): the newest election row of any state with its candidates
/// (left-out included) and electors, the current lord (`lord_id` of the newest expired election, `None` when there is
/// none or it had no winner), the running lord events and the skill cooldowns.
pub fn load_lord(conn: &Connection) -> Result<LordLoad, DbError> {
    let election = {
        let mut stmt =
            conn.prepare_cached(&format!("{ELECTION_SELECT} ORDER BY id DESC LIMIT 1"))?;
        let mut rows = stmt.query([])?;
        rows.next()?.map(read_election).transpose()?
    };
    let lord = conn
        .prepare_cached(
            "SELECT lord_id FROM lord_elections WHERE state = 3 ORDER BY id DESC LIMIT 1",
        )?
        .query_row([], |r| r.get::<_, Option<u32>>(0))
        .optional()?
        .flatten()
        .map(CharacterId);
    let mut candidates = Vec::new();
    let mut voters = Vec::new();
    if let Some(election) = &election {
        let mut stmt = conn.prepare_cached(&format!(
            "{CANDIDATE_SELECT} WHERE election_id = ?1 ORDER BY created_at, character_id"
        ))?;
        let mut rows = stmt.query([election.id])?;
        while let Some(row) = rows.next()? {
            candidates.push(read_candidate(row)?);
        }
        let mut stmt = conn.prepare_cached(
            "SELECT elector_id FROM lord_votes WHERE election_id = ?1 ORDER BY voted_at, elector_id",
        )?;
        let mut rows = stmt.query([election.id])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["elector_id"], 0);
            voters.push(CharacterId(c.int()?));
        }
    }
    let mut events = Vec::new();
    {
        let mut stmt =
            conn.prepare_cached(&format!("{EVENT_SELECT} ORDER BY created_at, character_id"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            events.push(read_event(row)?);
        }
    }
    let mut skill_cooldowns = Vec::new();
    {
        let mut stmt = conn
            .prepare_cached("SELECT skill_id, cooldown_min FROM lord_skills ORDER BY skill_id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["skill_id", "cooldown_min"], 0);
            skill_cooldowns.push((c.int()?, c.int()?));
        }
    }
    Ok(LordLoad {
        election,
        candidates,
        voters,
        lord,
        events,
        skill_cooldowns,
    })
}

/// The tax state at boot (`StartupState.tax`): the owner and detail rows of `round` (= `meta.tax_round`) only.
pub fn load_tax(conn: &Connection, round: u32) -> Result<TaxLoad, DbError> {
    let mut owners = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT round, continent, owner_id, next_owner_id, rate_set, changed_at
             FROM tax_owners WHERE round = ?1 ORDER BY continent",
        )?;
        let mut rows = stmt.query([round])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(
                row,
                &[
                    "round",
                    "continent",
                    "owner_id",
                    "next_owner_id",
                    "rate_set",
                    "changed_at",
                ],
                0,
            );
            let round: u32 = c.int()?;
            let code: u8 = c.int()?;
            owners.push(TaxOwnerRecord {
                round,
                continent: TaxContinent::from_code(code)
                    .ok_or_else(|| bug(format!("tax_owners.continent {code} of round {round}")))?,
                owner_id: c.opt_int()?,
                next_owner_id: c.opt_int()?,
                rate_set: c.int::<i64>()? != 0,
                changed_at: UnixTime(c.int()?),
            });
        }
    }
    let mut details = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT round, continent, kind, rate, next_rate, tx_count, gold, perin
             FROM tax_details WHERE round = ?1 ORDER BY continent, kind",
        )?;
        let mut rows = stmt.query([round])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(
                row,
                &[
                    "round",
                    "continent",
                    "kind",
                    "rate",
                    "next_rate",
                    "tx_count",
                    "gold",
                    "perin",
                ],
                0,
            );
            let round: u32 = c.int()?;
            let continent: u8 = c.int()?;
            let kind: u8 = c.int()?;
            details.push(TaxDetailRecord {
                round,
                continent: TaxContinent::from_code(continent).ok_or_else(|| {
                    bug(format!(
                        "tax_details.continent {continent} of round {round}"
                    ))
                })?,
                kind: TaxKind::from_code(kind)
                    .ok_or_else(|| bug(format!("tax_details.kind {kind} of round {round}")))?,
                rate: c.int()?,
                next_rate: c.int()?,
                tx_count: c.int()?,
                gold: c.int()?,
                perin: c.int()?,
            });
        }
    }
    Ok(TaxLoad {
        round,
        owners,
        details,
    })
}

/// The secret-room state at boot (`StartupState.secret_room`): the `T` tenders of `round` (= `meta.secret_room_round`)
/// and the stored line-ups of those tenders. Cancelled, failed, won and lost rows and older rounds are history.
pub fn load_secret_room(conn: &Connection, round: u32) -> Result<SecretRoomLoad, DbError> {
    let mut tenders = Vec::new();
    {
        let mut stmt = conn.prepare_cached(&format!(
            "{TENDER_SELECT} WHERE round = ?1 AND state = 'T'
             ORDER BY continent, penya DESC, tendered_at, guild_id"
        ))?;
        let mut rows = stmt.query([round])?;
        while let Some(row) = rows.next()? {
            tenders.push(read_tender(row)?);
        }
    }
    let wanted: BTreeSet<(u8, u32)> = tenders
        .iter()
        .map(|t| (t.continent.code(), t.guild_id.0))
        .collect();
    let mut lineups: Vec<SecretRoomLineup> = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT continent, guild_id, character_id FROM secret_room_lineups
             WHERE round = ?1 ORDER BY continent, guild_id, character_id",
        )?;
        let mut rows = stmt.query([round])?;
        while let Some(row) = rows.next()? {
            let mut c = Cols::new(row, &["continent", "guild_id", "character_id"], 0);
            let code: u8 = c.int()?;
            let guild: u32 = c.int()?;
            let character = CharacterId(c.int()?);
            if !wanted.contains(&(code, guild)) {
                continue;
            }
            let continent = side_of(code, "secret_room_lineups")?;
            match lineups.last_mut() {
                Some(last) if last.continent == continent && last.guild_id.0 == guild => {
                    last.members.push(character);
                }
                _ => lineups.push(SecretRoomLineup {
                    continent,
                    guild_id: GuildId(guild),
                    members: vec![character],
                }),
            }
        }
    }
    Ok(SecretRoomLoad {
        round,
        tenders,
        lineups,
    })
}
