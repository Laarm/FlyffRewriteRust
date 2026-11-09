//! The reads of the players area of the console (Plan/25 §4.2, §4.4, §4.7, §4.8, §11.6; WP-ADMIN-16 / WP-ADMIN-17):
//! `SearchCharacters`, `CharacterDocs`, `AccountBanks`, `AccountDetail`, `Alts` (plus the alt clusters and the active
//! bans of the Moderation tab).
//!
//! They are built on the chunked scan framework of [`super::scan`]: every function returns its rows through
//! [`PersistHandle::admin_collect`], so a long list never delays a login (the chunks queue behind lobby work, see the
//! framework's module documentation). All of them are point reads or pages bounded by construction (a page is at
//! most [`super::scan::CHUNK_ROWS_MAX`] rows), none is "heavy".
//!
//! ```text
//! let hits = persist.admin_search_characters(CharacterSearch::list("lead", now)).await?;      // Page<CharacterHit>
//! let docs = persist.admin_character_docs(id).await?;                                         // any state, no account check
//! let banks = persist.admin_account_banks(account).await?;                                    // Vec<AccountBankRow>
//! ```

use super::model::invalid;
use super::reads::{DEFAULT_PAGE, Page};
use super::scan::{KeysetScan, Once, page_limit};
use crate::error::DbError;
use crate::handle::PersistHandle;
use crate::model::{
    AccountBanRecord, BankDoc, CharacterDocs, CharacterIdentity, CharacterRow, CharacterState,
    JsonDoc, character_state_from_db,
};
use crate::repo::characters::{Cols, read_row};
use flyff_common::{AccountId, CharacterId, GuildId, UnixTime};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

/// A login block "until" that never ends (year 2286): `characters.login_blocked_until` has no separate "permanent"
/// marker, and the enter-world check only compares it with the clock.
pub const LOGIN_BLOCK_FOREVER: UnixTime = UnixTime(9_999_999_999);

/// How many alt accounts / cluster members are listed at most.
pub const ALTS_MAX: usize = 200;
/// How many clusters the Moderation -> Alts tab shows at most.
pub const ALT_CLUSTERS_MAX: usize = 100;

/// Which characters a search lists (Plan/25 §4.2 status chips).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterStatus {
    /// Any state.
    #[default]
    Any,
    /// Playable characters (`state = 0`).
    Active,
    /// Soft-deleted characters (`state = 1`), still within the retention time.
    Deleted,
    /// Sealed characters (`state = 2`).
    Sealed,
    /// Characters whose login is blocked right now (`login_blocked_until > now`).
    LoginBlocked,
}

impl CharacterStatus {
    /// Parses the `?status=` text of the route (`any`, `active`, `deleted`, `sealed`, `blocked`).
    pub fn parse(text: &str) -> Option<CharacterStatus> {
        match text {
            "" | "any" | "all" => Some(CharacterStatus::Any),
            "active" | "offline" | "online" => Some(CharacterStatus::Active),
            "deleted" => Some(CharacterStatus::Deleted),
            "sealed" => Some(CharacterStatus::Sealed),
            "blocked" | "login_blocked" => Some(CharacterStatus::LoginBlocked),
            _ => None,
        }
    }
}

/// A `SearchCharacters` request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterSearch {
    /// Name prefix, account-name prefix or a character id; empty = every character.
    pub text: String,
    /// Which states.
    pub status: CharacterStatus,
    /// The clock (`login_blocked_until > now`).
    pub now: UnixTime,
    /// Continue after this id (the `next_cursor` of the previous page); list mode only.
    pub cursor: Option<i64>,
    /// Page size `1..=500` (0 = [`DEFAULT_PAGE`]).
    pub limit: u32,
    /// Palette mode: best match first (exact name, active before deleted, short names), no cursor.
    pub ranked: bool,
}

impl CharacterSearch {
    /// A newest-first list of every character matching `text`, first page.
    pub fn list(text: impl Into<String>, now: UnixTime) -> CharacterSearch {
        CharacterSearch {
            text: text.into(),
            status: CharacterStatus::Any,
            now,
            cursor: None,
            limit: DEFAULT_PAGE,
            ranked: false,
        }
    }

    /// The palette form: at most `limit` best matches.
    pub fn ranked(text: impl Into<String>, now: UnixTime, limit: u32) -> CharacterSearch {
        CharacterSearch {
            ranked: true,
            limit,
            ..CharacterSearch::list(text, now)
        }
    }
}

/// One character of a search result or an account's character list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterHit {
    /// `characters.id`.
    pub id: CharacterId,
    /// The owning account.
    pub account_id: AccountId,
    /// `accounts.name`.
    pub account_name: String,
    /// `characters.name`.
    pub name: String,
    /// The slot `0..=2`.
    pub slot: u8,
    /// Active, deleted or sealed.
    pub state: CharacterState,
    /// Level at the last save.
    pub level: u16,
    /// Job at the last save.
    pub job: u8,
    /// World at the last save.
    pub world_id: u32,
    /// `characters.authority` (an ASCII letter).
    pub authority: u8,
    /// `characters.created_at`.
    pub created_at: UnixTime,
    /// `characters.deleted_at` of a soft-deleted character.
    pub deleted_at: Option<UnixTime>,
    /// `characters.login_blocked_until`, `None` = never blocked (a past time = the block ran out).
    pub login_blocked_until: Option<UnixTime>,
    /// `last_logout_at`, or the time of the last save when the character never logged out; `0` = never.
    pub last_seen: UnixTime,
}

/// What a character page selects (the columns [`decode_hit`] reads), joined with the account.
const HIT_COLUMNS: &str = "ch.id, ch.account_id, a.name, ch.name, ch.slot, ch.state, ch.level, ch.job, ch.world_id, \
     ch.authority, ch.created_at, ch.deleted_at, ch.login_blocked_until, ch.last_logout_at, ch.saved_at";

fn decode_hit(row: &Row<'_>) -> Result<Option<CharacterHit>, DbError> {
    const NAMES: &[&str] = &[
        "id",
        "account_id",
        "account_name",
        "name",
        "slot",
        "state",
        "level",
        "job",
        "world_id",
        "authority",
        "created_at",
        "deleted_at",
        "login_blocked_until",
        "last_logout_at",
        "saved_at",
    ];
    let mut c = Cols::new(row, NAMES, 0);
    let id = CharacterId(c.int()?);
    let account_id = AccountId(c.int()?);
    let account_name = c.text()?;
    let name = c.text()?;
    let slot = c.int()?;
    let state = character_state_from_db(c.int::<i64>()?)?;
    let level = c.int()?;
    let job = c.int()?;
    let world_id = c.int()?;
    let authority = c.text()?.bytes().next().unwrap_or(0);
    let created_at = UnixTime(c.int()?);
    let deleted_at = c.opt_int::<i64>()?.map(UnixTime);
    let login_blocked_until = c.opt_int::<i64>()?.map(UnixTime);
    let last_logout = c.int::<i64>()?;
    let saved_at = c.opt_int::<i64>()?;
    Ok(Some(CharacterHit {
        id,
        account_id,
        account_name,
        name,
        slot,
        state,
        level,
        job,
        world_id,
        authority,
        created_at,
        deleted_at,
        login_blocked_until,
        last_seen: UnixTime(if last_logout > 0 {
            last_logout
        } else {
            saved_at.unwrap_or(0)
        }),
    }))
}

/// Escapes `%`, `_` and `\` for a `LIKE ... ESCAPE '\'` pattern.
fn like_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The `WHERE` extension of a search and its parameters (`?3...`): text, status.
fn search_conditions(s: &CharacterSearch) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut params: Vec<Value> = Vec::new();
    let text = s.text.trim();
    if !text.is_empty() {
        let pattern = format!("{}%", like_escape(text));
        sql.push_str(
            " AND (ch.name LIKE ?3 ESCAPE '\\' OR a.name LIKE ?3 ESCAPE '\\' OR ch.id = ?4)",
        );
        params.push(Value::Text(pattern));
        params.push(
            text.parse::<u32>()
                .map_or(Value::Null, |n| Value::Integer(i64::from(n))),
        );
    } else {
        // keep the numbering of the parameters stable
        sql.push_str(" AND (?3 IS NULL AND ?4 IS NULL)");
        params.push(Value::Null);
        params.push(Value::Null);
    }
    match s.status {
        CharacterStatus::Any => sql.push_str(" AND ?5 IS NOT NULL"),
        CharacterStatus::Active => sql.push_str(" AND ch.state = 0 AND ?5 IS NOT NULL"),
        CharacterStatus::Deleted => sql.push_str(" AND ch.state = 1 AND ?5 IS NOT NULL"),
        CharacterStatus::Sealed => sql.push_str(" AND ch.state = 2 AND ?5 IS NOT NULL"),
        CharacterStatus::LoginBlocked => {
            sql.push_str(" AND ch.state <> 1 AND ch.login_blocked_until > ?5");
        }
    }
    params.push(Value::Integer(s.now.0));
    (sql, params)
}

fn page_of<T>(mut items: Vec<T>, limit: usize, id_of: fn(&T) -> i64) -> Page<T> {
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(id_of)
    } else {
        None
    };
    Page { items, next_cursor }
}

fn search_page(rows: Vec<CharacterHit>, size: usize) -> Page<CharacterHit> {
    page_of(rows, size, |h| i64::from(h.id.0))
}

// ---- CharacterDocs -------------------------------------------------------------------------------------------------

/// Everything the console shows of one character in any state (`CharacterDocs`, Plan/25 §11.6): unlike the
/// enter-world load there is **no account check** and no state check, and the documents are not repaired (the
/// console shows what is stored).
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterDocsView {
    /// The identity columns.
    pub identity: CharacterIdentity,
    /// `accounts.name` of the owner.
    pub account_name: String,
    /// The snapshot columns.
    pub row: CharacterRow,
    /// The nine documents.
    pub docs: CharacterDocs,
    /// The bank row of this character (`banks`); banks are account-scoped, see [`AccountBankRow`].
    pub bank: Option<AccountBankRow>,
    /// `characters.saved_at`: the commit time of the last snapshot.
    pub saved_at: Option<UnixTime>,
    /// `characters.deleted_at`.
    pub deleted_at: Option<UnixTime>,
    /// The guild of the character (`guild_members`).
    pub guild_id: Option<GuildId>,
}

/// The identity, the account name, `saved_at`, `deleted_at`, the guild and the snapshot columns of one row.
type Head = (
    CharacterIdentity,
    String,
    Option<i64>,
    Option<i64>,
    Option<u32>,
    CharacterRow,
);

fn character_docs(
    conn: &Connection,
    id: CharacterId,
) -> Result<Option<CharacterDocsView>, DbError> {
    let tx = conn.unchecked_transaction()?;
    let sql = format!(
        "SELECT ch.id, ch.account_id, ch.slot, ch.name, ch.state, ch.created_at, ch.authority, ch.login_blocked_until,
                a.name, ch.saved_at, ch.deleted_at, gm.guild_id, {}
         FROM characters ch JOIN accounts a ON a.id = ch.account_id
              LEFT JOIN guild_members gm ON gm.character_id = ch.id
         WHERE ch.id = ?1",
        CharacterRow::SNAPSHOT_COLUMNS
            .iter()
            .map(|c| format!("ch.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let head: Option<Head> = tx
        .prepare_cached(&sql)?
        .query_row([id.0], |row| Ok(read_head(row)))
        .optional()?
        .transpose()?;
    let Some((identity, account_name, saved_at, deleted_at, guild, row)) = head else {
        return Ok(None);
    };
    let columns: Option<[String; 9]> = tx
        .prepare_cached(
            "SELECT inventory, pockets, skills, quests, taskbar, buffs, sm_timers, honor, settings
             FROM character_docs WHERE character_id = ?1",
        )?
        .query_row([id.0], |r| {
            Ok([
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ])
        })
        .optional()?;
    let docs = match columns {
        Some(c) => CharacterDocs::from_json_columns(&c)?,
        None => CharacterDocs::default(),
    };
    let bank = bank_rows(&tx, "b.character_id = ?1", id.0)?
        .into_iter()
        .next();
    tx.commit()?;
    Ok(Some(CharacterDocsView {
        identity,
        account_name,
        row,
        docs,
        bank,
        saved_at: saved_at.map(UnixTime),
        deleted_at: deleted_at.map(UnixTime),
        guild_id: guild.map(GuildId),
    }))
}

fn read_head(row: &Row<'_>) -> Result<Head, DbError> {
    const NAMES: &[&str] = &[
        "id",
        "account_id",
        "slot",
        "name",
        "state",
        "created_at",
        "authority",
        "login_blocked_until",
        "account_name",
        "saved_at",
        "deleted_at",
        "guild_id",
    ];
    let mut c = Cols::new(row, NAMES, 0);
    let id = CharacterId(c.int()?);
    let account_id = AccountId(c.int()?);
    let slot = c.int()?;
    let name = c.text()?;
    let state = character_state_from_db(c.int::<i64>()?)?;
    let created_at = UnixTime(c.int()?);
    let authority = c.text()?.bytes().next().unwrap_or(0);
    let login_blocked_until = c.opt_int::<i64>()?.map(UnixTime);
    let account_name = c.text()?;
    let saved_at = c.opt_int::<i64>()?;
    let deleted_at = c.opt_int::<i64>()?;
    let guild = c.opt_int::<u32>()?;
    let mut snapshot = Cols::new(row, CharacterRow::SNAPSHOT_COLUMNS, NAMES.len());
    let character_row = read_row(&mut snapshot, id)?;
    Ok((
        CharacterIdentity {
            id,
            account_id,
            slot,
            name,
            state,
            created_at,
            authority,
            login_blocked_until,
        },
        account_name,
        saved_at,
        deleted_at,
        guild,
        character_row,
    ))
}

// ---- AccountBanks --------------------------------------------------------------------------------------------------

/// One bank of an account (Plan/25 §4.4 "Banks (account-scoped)"): the bank of the character in `slot`. The PIN is
/// never part of the view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountBankRow {
    /// The character the compartment belongs to.
    pub character_id: CharacterId,
    /// Its name.
    pub character_name: String,
    /// Its slot `0..=2`.
    pub slot: u8,
    /// `banks.gold`.
    pub gold: i64,
    /// The 42 slots.
    pub doc: BankDoc,
    /// The commit time of the last snapshot of that character (a bank is written with any snapshot of its
    /// account, so this is a lower bound of the freshness).
    pub saved_at: Option<UnixTime>,
}

fn bank_rows(conn: &Connection, filter: &str, arg: u32) -> Result<Vec<AccountBankRow>, DbError> {
    let sql = format!(
        "SELECT b.character_id, ch.name, ch.slot, b.gold, b.container, ch.saved_at
         FROM banks b JOIN characters ch ON ch.id = b.character_id
         WHERE {filter} ORDER BY ch.slot, b.character_id"
    );
    let rows: Vec<(i64, String, i64, i64, String, Option<i64>)> = conn
        .prepare_cached(&sql)?
        .query_map([arg], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (character, name, slot, gold, container, saved_at) in rows {
        out.push(AccountBankRow {
            character_id: CharacterId(
                u32::try_from(character).map_err(|_| invalid("banks.character_id out of range"))?,
            ),
            character_name: name,
            slot: u8::try_from(slot).map_err(|_| invalid("characters.slot out of range"))?,
            gold,
            doc: BankDoc::from_json(&container)?,
            saved_at: saved_at.map(UnixTime),
        });
    }
    Ok(out)
}

// ---- AccountDetail -------------------------------------------------------------------------------------------------

/// The account drawer of the console (Plan/25 §4.7). The password hash is never part of it; `registration_ip`,
/// `last_login_ip` and `email` are PII (the route drops them without `view.pii`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountDetailView {
    /// `accounts.id`.
    pub id: AccountId,
    /// `accounts.name`.
    pub name: String,
    /// The stored authority letter.
    pub authority: u8,
    /// `accounts.created_at`.
    pub created_at: UnixTime,
    /// `accounts.created_via` (`cli`, `web`, `bootstrap`).
    pub created_via: String,
    /// `accounts.registration_ip`.
    pub registration_ip: Option<String>,
    /// `accounts.last_login_at`.
    pub last_login_at: Option<UnixTime>,
    /// `accounts.last_login_ip`.
    pub last_login_ip: Option<String>,
    /// `accounts.email`.
    pub email: Option<String>,
    /// `accounts.must_change_password`.
    pub must_change_password: bool,
    /// `admin_totp.enabled_at`.
    pub totp_enabled_at: Option<UnixTime>,
    /// Every character of the account, any state, by slot then id.
    pub characters: Vec<CharacterHit>,
    /// The ban history, newest first (at most 100).
    pub bans: Vec<AccountBanRecord>,
}

fn account_detail(conn: &Connection, name: &str) -> Result<Option<AccountDetailView>, DbError> {
    let tx = conn.unchecked_transaction()?;
    #[allow(clippy::type_complexity)]
    let head: Option<(
        i64,
        String,
        String,
        i64,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
    )> = tx
        .prepare_cached(
            "SELECT a.id, a.name, a.authority, a.created_at, a.created_via, a.registration_ip, a.last_login_at,
                    a.last_login_ip, a.email, a.must_change_password, t.enabled_at
             FROM accounts a LEFT JOIN admin_totp t ON t.account_id = a.id WHERE a.name = ?1",
        )?
        .query_row([name], |r| {
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
                r.get(9)?,
                r.get(10)?,
            ))
        })
        .optional()?;
    let Some((id, name, authority, created_at, via, reg_ip, last_at, last_ip, email, must, totp)) =
        head
    else {
        return Ok(None);
    };
    let account = AccountId(u32::try_from(id).map_err(|_| invalid("accounts.id out of range"))?);
    let sql = format!(
        "SELECT {HIT_COLUMNS} FROM characters ch JOIN accounts a ON a.id = ch.account_id
         WHERE ch.account_id = ?1 ORDER BY ch.state = 1, ch.slot, ch.id"
    );
    let characters = tx
        .prepare_cached(&sql)?
        .query_map([account.0], |r| Ok(decode_hit(r)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()?;
    let bans = tx
        .prepare_cached(
            "SELECT id, account_id, created_at, until, reason, created_by, lifted_at, lifted_by
             FROM account_bans WHERE account_id = ?1 ORDER BY id DESC LIMIT 100",
        )?
        .query_map([account.0], |r| {
            Ok(AccountBanRecord {
                id: r.get(0)?,
                account_id: account,
                created_at: UnixTime(r.get(2)?),
                until: r.get::<_, Option<i64>>(3)?.map(UnixTime),
                reason: r.get(4)?,
                created_by: r.get(5)?,
                lifted_at: r.get::<_, Option<i64>>(6)?.map(UnixTime),
                lifted_by: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit()?;
    Ok(Some(AccountDetailView {
        id: account,
        name,
        authority: authority.bytes().next().unwrap_or(0),
        created_at: UnixTime(created_at),
        created_via: via,
        registration_ip: reg_ip,
        last_login_at: last_at.map(UnixTime),
        last_login_ip: last_ip,
        email,
        must_change_password: must != 0,
        totp_enabled_at: totp.map(UnixTime),
        characters,
        bans,
    }))
}

// ---- Alts ----------------------------------------------------------------------------------------------------------

/// An account that shares an IP address with another (Plan/25 §4.7 "Alts").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltRow {
    /// `accounts.id`.
    pub account_id: AccountId,
    /// `accounts.name`.
    pub name: String,
    /// The stored authority letter.
    pub authority: u8,
    /// `accounts.created_at`.
    pub created_at: UnixTime,
    /// `accounts.last_login_at`.
    pub last_login_at: Option<UnixTime>,
    /// The addresses the account shares with the subject (registration or last login).
    pub shared_ips: Vec<String>,
    /// A ban is active at the time of the query.
    pub banned: bool,
    /// Active characters of the account.
    pub characters: u32,
}

/// A group of accounts that share one address (Moderation -> Alts).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltCluster {
    /// The shared address.
    pub ip: String,
    /// The accounts that registered from it or logged in from it, by id (at most 20).
    pub accounts: Vec<AltAccount>,
    /// How many accounts share it (may exceed `accounts.len()`).
    pub total: u32,
}

/// An account of an [`AltCluster`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltAccount {
    /// `accounts.id`.
    pub account_id: AccountId,
    /// `accounts.name`.
    pub name: String,
    /// A ban is active.
    pub banned: bool,
}

const BAN_ACTIVE: &str = "EXISTS (SELECT 1 FROM account_bans b WHERE b.account_id = a.id AND b.lifted_at IS NULL \
     AND (b.until IS NULL OR b.until > ?3))";

fn alts(
    conn: &Connection,
    account: AccountId,
    since: UnixTime,
    now: UnixTime,
) -> Result<Vec<AltRow>, DbError> {
    let tx = conn.unchecked_transaction()?;
    let own: Option<(Option<String>, Option<String>)> = tx
        .prepare_cached("SELECT registration_ip, last_login_ip FROM accounts WHERE id = ?1")?
        .query_row([account.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((reg, last)) = own else {
        return Ok(Vec::new());
    };
    let mine: Vec<String> = [reg, last].into_iter().flatten().collect();
    if mine.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT a.id, a.name, a.authority, a.created_at, a.last_login_at, a.registration_ip, a.last_login_ip,
                {BAN_ACTIVE},
                (SELECT COUNT(*) FROM characters ch WHERE ch.account_id = a.id AND ch.state = 0)
         FROM accounts a
         WHERE a.id <> ?1 AND COALESCE(a.last_login_at, a.created_at) >= ?2
           AND (a.registration_ip IN (SELECT value FROM json_each(?4))
                OR a.last_login_ip IN (SELECT value FROM json_each(?4)))
         ORDER BY a.id LIMIT {ALTS_MAX}"
    );
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        i64,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        bool,
        i64,
    )> = tx
        .prepare_cached(&sql)?
        .query_map(
            params![account.0, since.0, now.0, serde_json::to_string(&mine)?],
            |r| {
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
            },
        )?
        .collect::<Result<_, _>>()?;
    tx.commit()?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, name, authority, created, last_at, reg, last, banned, chars) in rows {
        let mut shared: Vec<String> = [reg, last]
            .into_iter()
            .flatten()
            .filter(|ip| mine.contains(ip))
            .collect();
        shared.sort();
        shared.dedup();
        out.push(AltRow {
            account_id: AccountId(
                u32::try_from(id).map_err(|_| invalid("accounts.id out of range"))?,
            ),
            name,
            authority: authority.bytes().next().unwrap_or(0),
            created_at: UnixTime(created),
            last_login_at: last_at.map(UnixTime),
            shared_ips: shared,
            banned,
            characters: u32::try_from(chars).unwrap_or(0),
        });
    }
    Ok(out)
}

fn alt_clusters(
    conn: &Connection,
    since: UnixTime,
    now: UnixTime,
) -> Result<Vec<AltCluster>, DbError> {
    let tx = conn.unchecked_transaction()?;
    let groups: Vec<(String, i64)> = tx
        .prepare_cached(&format!(
            "WITH seen AS (
                 SELECT id, registration_ip AS ip FROM accounts
                  WHERE registration_ip IS NOT NULL AND COALESCE(last_login_at, created_at) >= ?1
                 UNION
                 SELECT id, last_login_ip FROM accounts
                  WHERE last_login_ip IS NOT NULL AND COALESCE(last_login_at, created_at) >= ?1)
             SELECT ip, COUNT(*) AS n FROM seen GROUP BY ip HAVING n >= 2
             ORDER BY n DESC, ip LIMIT {ALT_CLUSTERS_MAX}"
        ))?
        .query_map([since.0], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(groups.len());
    for (ip, total) in groups {
        let sql = format!(
            "SELECT a.id, a.name, {BAN_ACTIVE} FROM accounts a
             WHERE (a.registration_ip = ?1 OR a.last_login_ip = ?1) AND COALESCE(a.last_login_at, a.created_at) >= ?2
             ORDER BY a.id LIMIT 20"
        );
        // ?3 is `now` in BAN_ACTIVE
        let accounts: Vec<AltAccount> = tx
            .prepare_cached(&sql)?
            .query_map(params![ip, since.0, now.0], |r| {
                Ok(AltAccount {
                    account_id: AccountId(r.get::<_, u32>(0)?),
                    name: r.get(1)?,
                    banned: r.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        out.push(AltCluster {
            ip,
            accounts,
            total: u32::try_from(total).unwrap_or(u32::MAX),
        });
    }
    tx.commit()?;
    Ok(out)
}

// ---- active bans ---------------------------------------------------------------------------------------------------

/// An active account ban with the account name (Moderation -> Bans).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BanRow {
    /// The ban.
    pub ban: AccountBanRecord,
    /// `accounts.name`.
    pub account_name: String,
}

fn decode_ban_row(row: &Row<'_>) -> Result<Option<BanRow>, DbError> {
    let account: i64 = row.get(1)?;
    Ok(Some(BanRow {
        ban: AccountBanRecord {
            id: row.get(0)?,
            account_id: AccountId(
                u32::try_from(account)
                    .map_err(|_| invalid("account_bans.account_id out of range"))?,
            ),
            created_at: UnixTime(row.get(2)?),
            until: row.get::<_, Option<i64>>(3)?.map(UnixTime),
            reason: row.get(4)?,
            created_by: row.get(5)?,
            lifted_at: row.get::<_, Option<i64>>(6)?.map(UnixTime),
            lifted_by: row.get(7)?,
        },
        account_name: row.get(8)?,
    }))
}

// ---- account search --------------------------------------------------------------------------------------------------

/// An account in a palette search (Plan/25 §2.3, §4.7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountHit {
    /// `accounts.id`.
    pub id: AccountId,
    /// `accounts.name`.
    pub name: String,
    /// The stored authority letter.
    pub authority: u8,
    /// `accounts.created_at`.
    pub created_at: UnixTime,
    /// `accounts.last_login_at`.
    pub last_login_at: Option<UnixTime>,
    /// Active characters.
    pub characters: u32,
    /// A ban is in force at the time of the search.
    pub banned: bool,
}

fn search_accounts(
    conn: &Connection,
    text: &str,
    include_pii: bool,
    now: UnixTime,
    limit: usize,
) -> Result<Vec<AccountHit>, DbError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("{}%", like_escape(text));
    let mut stmt = conn.prepare_cached(
        "SELECT a.id, a.name, a.authority, a.created_at, a.last_login_at,
                (SELECT COUNT(*) FROM characters ch WHERE ch.account_id = a.id AND ch.state = 0),
                EXISTS (SELECT 1 FROM account_bans b WHERE b.account_id = a.id AND b.lifted_at IS NULL
                         AND (b.until IS NULL OR b.until > ?3))
         FROM accounts a
         WHERE a.name LIKE ?1 ESCAPE '\\'
            OR (?4 AND (a.email LIKE ?1 ESCAPE '\\' OR a.registration_ip LIKE ?1 ESCAPE '\\'
                        OR a.last_login_ip LIKE ?1 ESCAPE '\\'))
         ORDER BY (a.name = ?2) DESC, length(a.name), a.id
         LIMIT ?5",
    )?;
    let rows = stmt
        .query_map(
            params![
                pattern,
                text,
                now.0,
                i64::from(include_pii),
                i64::try_from(limit).unwrap_or(50)
            ],
            |r| {
                let id: i64 = r.get(0)?;
                let authority: String = r.get(2)?;
                let chars: i64 = r.get(5)?;
                Ok(AccountHit {
                    id: AccountId(u32::try_from(id).unwrap_or(u32::MAX)),
                    name: r.get(1)?,
                    authority: authority.bytes().next().unwrap_or(0),
                    created_at: UnixTime(r.get(3)?),
                    last_login_at: r.get::<_, Option<i64>>(4)?.map(UnixTime),
                    characters: u32::try_from(chars).unwrap_or(0),
                    banned: r.get(6)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---- the PersistHandle API -----------------------------------------------------------------------------------------

/// The page size of a search (`0` = the default).
fn search_size(s: &CharacterSearch) -> Result<usize, DbError> {
    page_limit(if s.limit == 0 { DEFAULT_PAGE } else { s.limit })
}

/// The palette form: one bounded read, best match first.
fn ranked_scan(
    s: &CharacterSearch,
    size: usize,
) -> impl super::scan::AdminScan<Item = CharacterHit> {
    let (conditions, params) = search_conditions(s);
    let sql = format!(
        "SELECT {HIT_COLUMNS} FROM characters ch JOIN accounts a ON a.id = ch.account_id
         WHERE ch.id < ?1{conditions}
         ORDER BY (ch.name = ?6) DESC, (ch.state = 0) DESC, length(ch.name), ch.id DESC LIMIT ?2"
    );
    let exact = s.text.trim().to_owned();
    let mut all = vec![
        Value::Integer(i64::MAX),
        Value::Integer(i64::try_from(size).unwrap_or(500)),
    ];
    all.extend(params);
    all.push(Value::Text(exact));
    Once::new(move |conn: &Connection| {
        let mut stmt = conn.prepare_cached(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(all))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            if let Some(h) = decode_hit(row)? {
                out.push(h);
            }
        }
        Ok(out)
    })
}

/// The list form: a keyset walk, newest first, one row more than the page to know whether another follows.
fn list_scan(s: &CharacterSearch, size: usize) -> KeysetScan<CharacterHit> {
    let (conditions, params) = search_conditions(s);
    let sql = format!(
        "SELECT {HIT_COLUMNS} FROM characters ch JOIN accounts a ON a.id = ch.account_id
         WHERE ch.id < ?1{conditions} ORDER BY ch.id DESC LIMIT ?2"
    );
    KeysetScan::descending(sql, params, s.cursor, decode_hit).limited(size + 1)
}

impl PersistHandle {
    /// `SearchCharacters` (Plan/25 §4.2, §11.6): characters by name prefix, account-name prefix or id, in any state,
    /// newest first (or ranked, for the palette), as a cursor page.
    pub async fn admin_search_characters(
        &self,
        search: CharacterSearch,
    ) -> Result<Page<CharacterHit>, DbError> {
        let size = search_size(&search)?;
        let rows = if search.ranked {
            self.admin_collect(ranked_scan(&search, size)).await?
        } else {
            self.admin_collect(list_scan(&search, size)).await?
        };
        Ok(search_page(rows, size))
    }

    /// Blocking variant of [`PersistHandle::admin_search_characters`].
    pub fn admin_search_characters_blocking(
        &self,
        search: CharacterSearch,
    ) -> Result<Page<CharacterHit>, DbError> {
        let size = search_size(&search)?;
        let rows = if search.ranked {
            self.admin_collect_blocking(ranked_scan(&search, size))?
        } else {
            self.admin_collect_blocking(list_scan(&search, size))?
        };
        Ok(search_page(rows, size))
    }

    /// One character as a search row (`None` when the id does not exist).
    pub async fn admin_character(&self, id: CharacterId) -> Result<Option<CharacterHit>, DbError> {
        let scan = Once::new(move |conn: &Connection| {
            let sql = format!(
                "SELECT {HIT_COLUMNS} FROM characters ch JOIN accounts a ON a.id = ch.account_id WHERE ch.id = ?1"
            );
            let hit = conn
                .prepare_cached(&sql)?
                .query_row([id.0], |r| Ok(decode_hit(r)))
                .optional()?
                .transpose()?
                .flatten();
            Ok(hit.into_iter().collect())
        });
        Ok(self.admin_collect(scan).await?.into_iter().next())
    }

    /// `CharacterDocs` (Plan/25 §11.6): a character in any state with its snapshot columns, documents and bank.
    pub async fn admin_character_docs(
        &self,
        id: CharacterId,
    ) -> Result<Option<CharacterDocsView>, DbError> {
        let scan =
            Once::new(move |conn: &Connection| Ok(character_docs(conn, id)?.into_iter().collect()));
        Ok(self.admin_collect(scan).await?.into_iter().next())
    }

    /// Blocking variant of [`PersistHandle::admin_character_docs`].
    pub fn admin_character_docs_blocking(
        &self,
        id: CharacterId,
    ) -> Result<Option<CharacterDocsView>, DbError> {
        let scan =
            Once::new(move |conn: &Connection| Ok(character_docs(conn, id)?.into_iter().collect()));
        Ok(self.admin_collect_blocking(scan)?.into_iter().next())
    }

    /// `AccountBanks` (Plan/25 §4.4, §11.6): the banks of the account's **active** characters, by slot. Only for an
    /// account that is entirely offline: while a character is online its `PlayerItems.banks` is the truth (an
    /// online character's database copy can be 90 s old).
    pub async fn admin_account_banks(
        &self,
        account: AccountId,
    ) -> Result<Vec<AccountBankRow>, DbError> {
        let scan = Once::new(move |conn: &Connection| {
            bank_rows(conn, "ch.account_id = ?1 AND ch.state = 0", account.0)
        });
        self.admin_collect(scan).await
    }

    /// Blocking variant of [`PersistHandle::admin_account_banks`].
    pub fn admin_account_banks_blocking(
        &self,
        account: AccountId,
    ) -> Result<Vec<AccountBankRow>, DbError> {
        let scan = Once::new(move |conn: &Connection| {
            bank_rows(conn, "ch.account_id = ?1 AND ch.state = 0", account.0)
        });
        self.admin_collect_blocking(scan)
    }

    /// Accounts by name prefix (and, with `include_pii`, by e-mail, registration or last-login address prefix),
    /// the exact name first, at most `limit` (1..=50).
    pub async fn admin_search_accounts(
        &self,
        text: &str,
        include_pii: bool,
        now: UnixTime,
        limit: u32,
    ) -> Result<Vec<AccountHit>, DbError> {
        let limit = page_limit(limit.clamp(1, 50))?;
        let text = text.to_owned();
        let scan = Once::new(move |conn: &Connection| {
            search_accounts(conn, &text, include_pii, now, limit)
        });
        self.admin_collect(scan).await
    }

    /// Blocking variant of [`PersistHandle::admin_search_accounts`].
    pub fn admin_search_accounts_blocking(
        &self,
        text: &str,
        include_pii: bool,
        now: UnixTime,
        limit: u32,
    ) -> Result<Vec<AccountHit>, DbError> {
        let limit = page_limit(limit.clamp(1, 50))?;
        let text = text.to_owned();
        let scan = Once::new(move |conn: &Connection| {
            search_accounts(conn, &text, include_pii, now, limit)
        });
        self.admin_collect_blocking(scan)
    }

    /// `AccountDetail` (Plan/25 §4.7, §11.6): the account by name (case-insensitive) with its characters and bans.
    pub async fn admin_account_detail(
        &self,
        name: &str,
    ) -> Result<Option<AccountDetailView>, DbError> {
        let name = name.to_owned();
        let scan = Once::new(move |conn: &Connection| {
            Ok(account_detail(conn, &name)?.into_iter().collect())
        });
        Ok(self.admin_collect(scan).await?.into_iter().next())
    }

    /// Blocking variant of [`PersistHandle::admin_account_detail`].
    pub fn admin_account_detail_blocking(
        &self,
        name: &str,
    ) -> Result<Option<AccountDetailView>, DbError> {
        let name = name.to_owned();
        let scan = Once::new(move |conn: &Connection| {
            Ok(account_detail(conn, &name)?.into_iter().collect())
        });
        Ok(self.admin_collect_blocking(scan)?.into_iter().next())
    }

    /// `Alts` (Plan/25 §4.7, §11.6): the other accounts that registered from or last logged in from an address of
    /// `account`, active since `since` (`days` back from `now`).
    pub async fn admin_alts(
        &self,
        account: AccountId,
        days: u32,
        now: UnixTime,
    ) -> Result<Vec<AltRow>, DbError> {
        let since = UnixTime(now.0.saturating_sub(i64::from(days) * 86_400));
        let scan = Once::new(move |conn: &Connection| alts(conn, account, since, now));
        self.admin_collect(scan).await
    }

    /// Blocking variant of [`PersistHandle::admin_alts`].
    pub fn admin_alts_blocking(
        &self,
        account: AccountId,
        days: u32,
        now: UnixTime,
    ) -> Result<Vec<AltRow>, DbError> {
        let since = UnixTime(now.0.saturating_sub(i64::from(days) * 86_400));
        let scan = Once::new(move |conn: &Connection| alts(conn, account, since, now));
        self.admin_collect_blocking(scan)
    }

    /// The clusters of accounts that share an address, largest first (Moderation -> Alts).
    pub async fn admin_alt_clusters(
        &self,
        days: u32,
        now: UnixTime,
    ) -> Result<Vec<AltCluster>, DbError> {
        let since = UnixTime(now.0.saturating_sub(i64::from(days) * 86_400));
        let scan = Once::new(move |conn: &Connection| alt_clusters(conn, since, now));
        self.admin_collect(scan).await
    }

    /// The bans in force at `now`, newest first, as a cursor page (Moderation -> Bans).
    pub async fn admin_active_bans(
        &self,
        now: UnixTime,
        cursor: Option<i64>,
        limit: u32,
    ) -> Result<Page<BanRow>, DbError> {
        let size = page_limit(if limit == 0 { DEFAULT_PAGE } else { limit })?;
        let scan = KeysetScan::descending(
            "SELECT b.id, b.account_id, b.created_at, b.until, b.reason, b.created_by, b.lifted_at, b.lifted_by, a.name
             FROM account_bans b JOIN accounts a ON a.id = b.account_id
             WHERE b.id < ?1 AND b.lifted_at IS NULL AND (b.until IS NULL OR b.until > ?3)
             ORDER BY b.id DESC LIMIT ?2",
            vec![Value::Integer(now.0)],
            cursor,
            decode_ban_row,
        )
        .limited(size + 1);
        let rows = self.admin_collect(scan).await?;
        Ok(page_of(rows, size, |b| b.ban.id))
    }
}
