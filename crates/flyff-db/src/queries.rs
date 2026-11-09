//! Typed read queries (Plan/13 §16.7): run inside `PersistHandle::read` closures on the read-only
//! `flyff-persist-ro-*` connections (lobby login, character list, enter-world load, admin tooling). Each
//! function is one read transaction, so what it returns is consistent.

use crate::error::{DbError, DbErrorKind};
use crate::migrate::stored_init_only;
use crate::model::{
    AccountBanRecord, AccountListEntry, AccountListFilter, AccountRecord, BankDoc, BankLoad,
    CharacterDocs, CharacterIdentity, CharacterListEntry, CharacterLoad, CharacterRow,
    CharacterState, CreatedVia, InventoryDoc, JsonDoc, PocketsDoc, RepairNote,
    character_state_from_db,
};
use crate::repo::characters::{Cols, decode_preview, read_row};
use crate::repo::{guilds, houses, messaging};
use flyff_common::{AccountId, CharacterId, GuildId, UnixTime};
use rusqlite::{Connection, OptionalExtension, Row, params};

const ACCOUNT_COLUMNS: &str = "id, name, password_hash, authority, created_at, created_via, \
                               registration_ip, last_login_at, last_login_ip, email, must_change_password";

fn bug(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Bug, what)
}

fn authority_byte(text: &str) -> u8 {
    text.bytes().next().unwrap_or(0)
}

fn account_from_row(r: &Row<'_>) -> rusqlite::Result<(AccountRecord, String)> {
    let id: i64 = r.get(0)?;
    let via: String = r.get(5)?;
    let authority: String = r.get(3)?;
    Ok((
        AccountRecord {
            id: AccountId(u32::try_from(id).unwrap_or(u32::MAX)),
            name: r.get(1)?,
            password_hash: r.get(2)?,
            authority: authority_byte(&authority),
            created_at: UnixTime(r.get(4)?),
            created_via: CreatedVia::parse(&via).unwrap_or_default(),
            registration_ip: r.get(6)?,
            last_login_at: r.get::<_, Option<i64>>(7)?.map(UnixTime),
            last_login_ip: r.get(8)?,
            email: r.get(9)?,
            must_change_password: r.get::<_, i64>(10)? != 0,
        },
        via,
    ))
}

fn checked_account(row: (AccountRecord, String)) -> Result<AccountRecord, DbError> {
    let (account, via) = row;
    if CreatedVia::parse(&via).is_none() {
        return Err(bug(format!(
            "accounts.created_via of '{}' holds '{via}'",
            account.name
        )));
    }
    Ok(account)
}

/// The account with this name (case-insensitive), `None` when it does not exist.
pub fn find_account(c: &Connection, name: &str) -> Result<Option<AccountRecord>, DbError> {
    c.prepare_cached(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE name = ?1"
    ))?
    .query_row([name], account_from_row)
    .optional()?
    .map(checked_account)
    .transpose()
}

/// The account with this id, `None` when it does not exist (doc 12 §5.1).
pub fn account_by_id(c: &Connection, id: AccountId) -> Result<Option<AccountRecord>, DbError> {
    c.prepare_cached(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = ?1"
    ))?
    .query_row([id.0], account_from_row)
    .optional()?
    .map(checked_account)
    .transpose()
}

/// Escapes `%`, `_` and the escape character for a `LIKE ... ESCAPE '\'` pattern.
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

/// The account list of the admin tooling (doc 12 §5.5-§5.6): ordered by id, without the password hash, with
/// the number of active characters and the ban that applies at `now`.
pub fn list_accounts(
    c: &Connection,
    filter: &AccountListFilter,
    now: UnixTime,
) -> Result<Vec<AccountListEntry>, DbError> {
    let pattern = filter
        .name_substr
        .as_deref()
        .map(|s| format!("%{}%", like_escape(s)));
    let mut stmt = c.prepare_cached(
        "SELECT a.id, a.name, a.authority, a.created_at, a.created_via, a.last_login_at, a.last_login_ip,
                (SELECT COUNT(*) FROM characters ch WHERE ch.account_id = a.id AND ch.state = 0),
                EXISTS (SELECT 1 FROM account_bans b WHERE b.account_id = a.id AND b.lifted_at IS NULL
                         AND (b.until IS NULL OR b.until > ?1)),
                (SELECT b.until FROM account_bans b WHERE b.account_id = a.id AND b.lifted_at IS NULL
                   AND (b.until IS NULL OR b.until > ?1)
                  ORDER BY b.until IS NULL DESC, b.until DESC LIMIT 1)
         FROM accounts a
         WHERE (?2 IS NULL OR a.name LIKE ?2 ESCAPE '\\')
           AND (?3 = 0 OR a.authority >= 'L')
         ORDER BY a.id
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![
            now.0,
            pattern,
            i64::from(filter.gm_only),
            i64::from(filter.limit)
        ],
        |r| {
            let id: i64 = r.get(0)?;
            let authority: String = r.get(2)?;
            let via: String = r.get(4)?;
            let banned: bool = r.get(8)?;
            let until: Option<i64> = r.get(9)?;
            let count: i64 = r.get(7)?;
            Ok(AccountListEntry {
                id: AccountId(u32::try_from(id).unwrap_or(u32::MAX)),
                name: r.get(1)?,
                authority: authority_byte(&authority),
                created_at: UnixTime(r.get(3)?),
                created_via: CreatedVia::parse(&via).unwrap_or_default(),
                last_login_at: r.get::<_, Option<i64>>(5)?.map(UnixTime),
                last_login_ip: r.get(6)?,
                banned_until: banned.then_some(until.map(UnixTime)),
                characters: u32::try_from(count).unwrap_or(0),
            })
        },
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The ban that applies to `account` at `now` (`lifted_at IS NULL AND (until IS NULL OR until > now)`). With
/// several, the permanent one wins, then the one that lasts longest.
pub fn active_ban(
    c: &Connection,
    account: AccountId,
    now: UnixTime,
) -> Result<Option<AccountBanRecord>, DbError> {
    Ok(c.prepare_cached(
        "SELECT id, account_id, created_at, until, reason, created_by, lifted_at, lifted_by
         FROM account_bans
         WHERE account_id = ?1 AND lifted_at IS NULL AND (until IS NULL OR until > ?2)
         ORDER BY until IS NULL DESC, until DESC, id DESC
         LIMIT 1",
    )?
    .query_row(params![account.0, now.0], |r| {
        let account_id: i64 = r.get(1)?;
        Ok(AccountBanRecord {
            id: r.get(0)?,
            account_id: AccountId(u32::try_from(account_id).unwrap_or(u32::MAX)),
            created_at: UnixTime(r.get(2)?),
            until: r.get::<_, Option<i64>>(3)?.map(UnixTime),
            reason: r.get(4)?,
            created_by: r.get(5)?,
            lifted_at: r.get::<_, Option<i64>>(6)?.map(UnixTime),
            lifted_by: r.get(7)?,
        })
    })
    .optional()?)
}

/// The character-select list: at most three active characters of `account`, ordered by slot.
pub fn character_list(
    c: &Connection,
    account: AccountId,
) -> Result<Vec<CharacterListEntry>, DbError> {
    let tx = c.unchecked_transaction()?;
    let mut stmt = tx.prepare_cached(
        "SELECT ch.id, ch.slot, ch.name, ch.sex, ch.mover_index, ch.skin_set, ch.hair_mesh, ch.hair_color,
                ch.head_mesh, ch.hide_coat_mask, ch.job, ch.level, ch.stat_str, ch.stat_sta, ch.stat_dex,
                ch.stat_int, ch.mode_flags, ch.world_id, ch.pos_x, ch.pos_y, ch.pos_z,
                ch.equipment_preview, ch.login_blocked_until, gm.guild_id
         FROM characters ch LEFT JOIN guild_members gm ON gm.character_id = ch.id
         WHERE ch.account_id = ?1 AND ch.state = 0
         ORDER BY ch.slot
         LIMIT 3",
    )?;
    let mut rows = stmt.query([account.0])?;
    let mut list = Vec::new();
    while let Some(row) = rows.next()? {
        const NAMES: &[&str] = &[
            "id",
            "slot",
            "name",
            "sex",
            "mover_index",
            "skin_set",
            "hair_mesh",
            "hair_color",
            "head_mesh",
            "hide_coat_mask",
            "job",
            "level",
            "stat_str",
            "stat_sta",
            "stat_dex",
            "stat_int",
            "mode_flags",
            "world_id",
            "pos_x",
            "pos_y",
            "pos_z",
            "equipment_preview",
            "login_blocked_until",
            "guild_id",
        ];
        let mut cols = Cols::new(row, NAMES, 0);
        let id = CharacterId(cols.int()?);
        let slot = cols.int()?;
        let name = cols.text()?;
        let sex = cols.int()?;
        let mover_index = cols.int()?;
        let skin_set = cols.int()?;
        let hair_mesh = cols.int()?;
        let hair_color = cols.int()?;
        let head_mesh = cols.int()?;
        let hide_coat_mask = cols.int()?;
        let job = cols.int()?;
        let level = cols.int()?;
        let stats = [cols.int()?, cols.int()?, cols.int()?, cols.int()?];
        let mode_flags = cols.int()?;
        let world_id = cols.int()?;
        let pos = [cols.f32()?, cols.f32()?, cols.f32()?];
        let preview = cols.text()?;
        let login_blocked_until = cols.opt_int::<i64>()?.map(UnixTime);
        let guild_id = cols.opt_int::<u32>()?.map(GuildId);
        list.push(CharacterListEntry {
            id,
            slot,
            name,
            sex,
            mover_index,
            skin_set,
            hair_mesh,
            hair_color,
            head_mesh,
            hide_coat_mask,
            job,
            level,
            stats,
            mode_flags,
            world_id,
            pos,
            equipment_preview: decode_preview(&preview, id),
            login_blocked_until,
            guild_id,
        });
    }
    drop(rows);
    drop(stmt);
    tx.commit()?;
    Ok(list)
}

fn character_not_found(account: AccountId, id: CharacterId) -> DbError {
    DbError::new(
        DbErrorKind::NotFound,
        format!("character {id} of account {account} does not exist"),
    )
}

/// The enter-world load (Plan/13 §16.10, §17.4 step 1): identity, snapshot columns, the nine documents
/// upgraded to the current schema version with their containers validated and repaired, and a bank for every
/// active character of the account, all from one read transaction.
///
/// `NotFound` unless the character exists, belongs to `account` and is active (`state = 0`). Container
/// capacities follow the init-only values recorded in `meta` (`features.inventory_168`,
/// `features.bigger_pocket`).
pub fn load_character(
    c: &Connection,
    account: AccountId,
    id: CharacterId,
) -> Result<CharacterLoad, DbError> {
    let tx = c.unchecked_transaction()?;
    let sql = format!(
        "SELECT id, account_id, slot, name, state, created_at, authority, login_blocked_until, {}
         FROM characters WHERE id = ?1",
        CharacterRow::SNAPSHOT_COLUMNS.join(", ")
    );
    let found = tx
        .prepare_cached(&sql)?
        .query_row([id.0], |row| Ok(read_identity_and_row(row)))
        .optional()?
        .transpose()?;
    let Some((identity, row)) = found else {
        return Err(character_not_found(account, id));
    };
    if identity.account_id != account || identity.state != CharacterState::Active {
        return Err(character_not_found(account, id));
    }

    let columns: [String; 9] = tx
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
        .optional()?
        .ok_or_else(|| bug(format!("character {id} has no character_docs row")))?;
    let mut docs = CharacterDocs::from_json_columns(&columns)?;

    let init = stored_init_only(&tx)?.unwrap_or_default();
    let inventory_capacity = if init.inventory_168 {
        InventoryDoc::CAPACITY_168
    } else {
        42 + crate::model::EQUIP_SLOTS
    };
    let pocket_capacities = if init.bigger_pocket {
        PocketsDoc::CAPACITIES_BIG
    } else {
        [6, 24, 24]
    };
    let mut repairs: Vec<RepairNote> = docs.inventory.validate_and_repair(inventory_capacity);
    repairs.extend(docs.pockets.validate_and_repair(pocket_capacities));

    let banks = load_banks(&tx, account, &mut repairs)?;
    for note in &repairs {
        tracing::warn!(character = id.0, ?note, "container repaired at load");
    }
    // The messaging group (0002): all of it from the same read transaction.
    let friends = messaging::friends_of(&tx, id)?;
    let notes = messaging::undelivered_notes(&tx, id)?;
    let grants = messaging::open_grants_of(&tx, id)?;
    let removals = messaging::open_removals_of(&tx, id)?;
    let party_name = tx
        .prepare_cached("SELECT name FROM party_names WHERE character_id = ?1")?
        .query_row([id.0], |r| r.get::<_, String>(0))
        .optional()?;
    // the guild group (0003): the raw end of the rejoin cooldown, the game compares it with its own clock
    let guild_rejoin_until = guilds::rejoin_until(&tx, id)?;
    // the houses group (0007): the character's own mini room and who may visit it
    let housing = houses::load_housing(&tx, id)?;
    tx.commit()?;
    Ok(CharacterLoad {
        identity,
        row,
        docs,
        repairs,
        banks,
        friends,
        notes,
        grants,
        removals,
        party_name,
        guild_rejoin_until,
        housing,
    })
}

fn read_identity_and_row(row: &Row<'_>) -> Result<(CharacterIdentity, CharacterRow), DbError> {
    const NAMES: &[&str] = &[
        "id",
        "account_id",
        "slot",
        "name",
        "state",
        "created_at",
        "authority",
        "login_blocked_until",
    ];
    let mut cols = Cols::new(row, NAMES, 0);
    let id = CharacterId(cols.int()?);
    let account_id = AccountId(cols.int()?);
    let slot = cols.int()?;
    let name = cols.text()?;
    let state = character_state_from_db(cols.int::<i64>()?)?;
    let created_at = UnixTime(cols.int()?);
    let authority = authority_byte(&cols.text()?);
    let login_blocked_until = cols.opt_int::<i64>()?.map(UnixTime);
    let mut snapshot_cols = Cols::new(row, CharacterRow::SNAPSHOT_COLUMNS, NAMES.len());
    let character_row = read_row(&mut snapshot_cols, id)?;
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
        character_row,
    ))
}

fn load_banks(
    tx: &Connection,
    account: AccountId,
    repairs: &mut Vec<RepairNote>,
) -> Result<Vec<BankLoad>, DbError> {
    let mut stmt = tx.prepare_cached(
        "SELECT b.character_id, ch.slot, b.pin, b.gold, b.container
         FROM banks b JOIN characters ch ON ch.id = b.character_id
         WHERE ch.account_id = ?1 AND ch.state = 0
         ORDER BY ch.slot",
    )?;
    let rows: Vec<(i64, i64, String, i64, String)> = stmt
        .query_map([account.0], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<_, _>>()?;
    let mut banks = Vec::with_capacity(rows.len());
    for (character, slot, pin, gold, container) in rows {
        let pin: [u8; 4] = pin.as_bytes().try_into().map_err(|_| {
            bug(format!(
                "banks.pin of character {character} is not 4 characters"
            ))
        })?;
        let mut doc = BankDoc::from_json(&container)?;
        repairs.extend(doc.validate_and_repair());
        banks.push(BankLoad {
            character_id: CharacterId(
                u32::try_from(character).map_err(|_| bug("banks.character_id out of range"))?,
            ),
            slot: u8::try_from(slot).map_err(|_| bug("characters.slot out of range"))?,
            pin,
            gold,
            doc,
        });
    }
    Ok(banks)
}
