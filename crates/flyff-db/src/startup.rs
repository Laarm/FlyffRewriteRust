//! Loading the [`StartupState`] (Plan/13 §16.9): one read transaction before the game thread starts.

use crate::error::{DbError, DbErrorKind};
use crate::model::{
    CharacterState, DirectoryEntry, MetaCounters, PartyNameRecord, RuntimeOverrideRecord,
    StartupState, character_state_from_db,
};
use crate::repo::{guilds, houses, realm, relations, siege};
use flyff_common::{AccountId, CharacterId, UnixTime};
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};

fn counters(conn: &Connection) -> Result<MetaCounters, DbError> {
    let mut stmt = conn.prepare("SELECT key, value FROM meta")?;
    let map: BTreeMap<String, i64> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<_, _>>()?;
    let defaults = MetaCounters::default();
    let u32_of = |key: &str, default: u32| {
        map.get(key)
            .map_or(default, |v| u32::try_from(*v).unwrap_or(u32::MAX))
    };
    Ok(MetaCounters {
        next_character_id: u32_of("next_character_id", defaults.next_character_id),
        next_guild_id: u32_of("next_guild_id", defaults.next_guild_id),
        next_mail_id: u32_of("next_mail_id", defaults.next_mail_id),
        next_guild_war_id: u32_of("next_guild_war_id", defaults.next_guild_war_id),
        next_item_serial: map
            .get("next_item_serial")
            .map_or(defaults.next_item_serial, |v| {
                u64::try_from(*v).unwrap_or(1)
            }),
        next_campus_id: u32_of("next_campus_id", defaults.next_campus_id),
        next_couple_id: u32_of("next_couple_id", defaults.next_couple_id),
        next_siege_id: u32_of("next_siege_id", defaults.next_siege_id),
        next_lord_election_id: u32_of("next_lord_election_id", defaults.next_lord_election_id),
        next_guild_furniture_seq: u32_of(
            "next_guild_furniture_seq",
            defaults.next_guild_furniture_seq,
        ),
        next_quiz_event_id: u32_of("next_quiz_event_id", defaults.next_quiz_event_id),
        tax_round: u32_of("tax_round", defaults.tax_round),
        secret_room_round: u32_of("secret_room_round", defaults.secret_room_round),
        rainbow_race_round: u32_of("rainbow_race_round", defaults.rainbow_race_round),
        gc1to1_round: u32_of("gc1to1_round", defaults.gc1to1_round),
    })
}

fn server_kv(conn: &Connection) -> Result<BTreeMap<String, serde_json::Value>, DbError> {
    let mut stmt = conn.prepare("SELECT key, value FROM server_kv ORDER BY key")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(key, text)| {
            serde_json::from_str(&text)
                .map(|value| (key.clone(), value))
                .map_err(|e| DbError::new(DbErrorKind::Bug, format!("server_kv '{key}': {e}")))
        })
        .collect()
}

fn runtime_overrides(conn: &Connection) -> Result<Vec<RuntimeOverrideRecord>, DbError> {
    let mut stmt =
        conn.prepare("SELECT key, value, set_by, set_at FROM runtime_override ORDER BY key")?;
    let rows: Vec<(String, String, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(key, text, set_by, set_at)| {
            let value = serde_json::from_str(&text).map_err(|e| {
                DbError::new(DbErrorKind::Bug, format!("runtime_override '{key}': {e}"))
            })?;
            Ok(RuntimeOverrideRecord {
                key,
                value,
                set_by,
                set_at: UnixTime(set_at),
            })
        })
        .collect()
}

/// `(id, account_id, name, slot, sex, job, level, state, deleted_at)` of one `characters` row.
type DirectoryRow = (i64, i64, String, i64, i64, i64, i64, i64, Option<i64>);

fn directory(conn: &Connection) -> Result<Vec<DirectoryEntry>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, name, slot, sex, job, level, state, deleted_at FROM characters ORDER BY id",
    )?;
    let rows: Vec<DirectoryRow> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    let bad = |what: &str, value: i64| {
        DbError::new(
            DbErrorKind::Bug,
            format!("characters.{what} holds {value}, out of range"),
        )
    };
    rows.into_iter()
        .map(
            |(id, account, name, slot, sex, job, level, state, deleted_at)| {
                Ok(DirectoryEntry {
                    id: CharacterId(u32::try_from(id).map_err(|_| bad("id", id))?),
                    account_id: AccountId(
                        u32::try_from(account).map_err(|_| bad("account_id", account))?,
                    ),
                    name,
                    slot: u8::try_from(slot).map_err(|_| bad("slot", slot))?,
                    sex: u8::try_from(sex).map_err(|_| bad("sex", sex))?,
                    job: u8::try_from(job).map_err(|_| bad("job", job))?,
                    level: u16::try_from(level).map_err(|_| bad("level", level))?,
                    state: character_state_from_db(state)?,
                    deleted_at: deleted_at.map(UnixTime),
                })
            },
        )
        .collect()
}

fn party_names(conn: &Connection) -> Result<Vec<PartyNameRecord>, DbError> {
    let mut stmt =
        conn.prepare("SELECT character_id, name FROM party_names ORDER BY character_id")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(id, name)| {
            Ok(PartyNameRecord {
                character_id: CharacterId(u32::try_from(id).map_err(|_| {
                    DbError::new(
                        DbErrorKind::Bug,
                        format!("party_names.character_id holds {id}, out of range"),
                    )
                })?),
                name,
            })
        })
        .collect()
}

/// The undelivered notes per receiver (`notes_undelivered` index), ordered by receiver.
fn note_counts(conn: &Connection) -> Result<Vec<(CharacterId, u32)>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT to_id, COUNT(*) FROM notes WHERE delivered_at IS NULL GROUP BY to_id ORDER BY to_id",
    )?;
    let rows: Vec<(i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(id, n)| {
            let out_of_range = |what: &str, v: i64| {
                DbError::new(
                    DbErrorKind::Bug,
                    format!("notes {what} holds {v}, out of range"),
                )
            };
            Ok((
                CharacterId(u32::try_from(id).map_err(|_| out_of_range("to_id", id))?),
                u32::try_from(n).map_err(|_| out_of_range("count", n))?,
            ))
        })
        .collect()
}

/// Reads the startup state in one read transaction with the host clock ([`load_at`]).
pub fn load(conn: &Connection) -> Result<StartupState, DbError> {
    load_at(conn, crate::clock::now_unix())
}

/// Reads the startup state in one read transaction. `now` only decides which rejoin cooldowns are still running
/// (`until > now`); the groups are loaded set-based (one query per table, grouped in memory).
pub fn load_at(conn: &Connection, now: UnixTime) -> Result<StartupState, DbError> {
    let tx = conn.unchecked_transaction()?;
    let counters = counters(&tx)?;
    let server_kv = server_kv(&tx)?;
    let runtime_overrides = runtime_overrides(&tx)?;
    let directory = directory(&tx)?;
    let party_names = party_names(&tx)?;
    let mails = crate::repo::messaging::open_mails(&tx)?;
    // the social groups (0003-0005)
    let guilds = guilds::load_guilds(&tx)?;
    let known: BTreeSet<u32> = guilds.iter().map(|g| g.guild.id.0).collect();
    // the content groups (0006, 0007): the rounds are the `meta` counters of the same transaction
    let lord = realm::load_lord(&tx)?;
    let tax = realm::load_tax(&tx, counters.tax_round)?;
    let secret_room = realm::load_secret_room(&tx, counters.secret_room_round)?;
    let rainbow_race = houses::load_rainbow_race(&tx, counters.rainbow_race_round)?;
    let state = StartupState {
        counters,
        server_kv,
        runtime_overrides,
        directory,
        party_names,
        mails,
        running_wars: guilds::load_running_wars(&tx, &known)?,
        rejoin_cooldowns: guilds::load_rejoin_cooldowns(&tx, now)?,
        guilds,
        couples: relations::load_couples(&tx)?,
        couple_proposals: relations::load_couple_proposals(&tx)?,
        campuses: relations::load_campuses(&tx)?,
        campus_points: relations::load_campus_points(&tx)?,
        sieges: siege::load_sieges(&tx)?,
        gc1to1: siege::load_gc1to1(&tx)?,
        lord,
        tax,
        secret_room,
        guild_houses: houses::load_guild_houses(&tx)?,
        rainbow_race,
        note_counts: note_counts(&tx)?,
    };
    tx.commit()?;
    Ok(state)
}

/// Directory entries of characters that are soft-deleted before `cutoff` (the purge selection of
/// Plan/13 §14.2, run by the game thread over its `PlayerDirectory`).
pub fn purge_candidates(directory: &[DirectoryEntry], cutoff: UnixTime) -> Vec<CharacterId> {
    directory
        .iter()
        .filter(|e| e.state == CharacterState::Deleted && e.deleted_at.is_some_and(|t| t < cutoff))
        .map(|e| e.id)
        .collect()
}
