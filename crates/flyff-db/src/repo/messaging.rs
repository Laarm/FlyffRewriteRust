//! SQL of the messaging & economy group (migration `0002`, Plan/13 §7): friends, offline notes, mails, pending
//! grants and pending removals. The write functions run on the writer connection inside a command savepoint
//! (`DbOp` dispatch in [`crate::ops`]); the read functions run on the reader connections
//! ([`crate::queries`], [`crate::startup`], `PersistHandle::read_for_game`).
//!
//! Failure mapping (Plan/13 §16.6): a missing referenced character is `NotFound`, a duplicate id or a
//! self-friendship is `Conflict` (both *Expected*: the savepoint rolls back, the other commands go on); a
//! `CHECK` violation of a grant or removal is a malformed record, i.e. a `Bug`.

use crate::error::{DbError, DbErrorKind, is_constraint};
use crate::model::{
    FriendRecord, GrantKind, JsonDoc, MailRecord, MailStateRecord, NoteRecord, PendingGrantRecord,
    PendingRemovalRecord, RemovalLocation, StoredItem,
};
use crate::repo::characters::Cols;
use flyff_common::{CharacterId, GuildId, MailId, UnixTime};
use rusqlite::{Connection, OptionalExtension, params};

/// A poll of the open grants returns at most this many rows (a runaway table must not stall the reader).
pub const MAX_GRANT_POLL_ROWS: usize = 10_000;

fn bug(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Bug, what)
}

/// Maps the constraint failures an insert can hit to *Expected* kinds.
fn map_write_error(e: rusqlite::Error, what: &str) -> DbError {
    if is_constraint(&e, "FOREIGN KEY", "") {
        DbError::new(
            DbErrorKind::NotFound,
            format!("{what}: a referenced character does not exist"),
        )
    } else if is_constraint(&e, "UNIQUE", "") {
        DbError::new(DbErrorKind::Conflict, format!("{what}: id already in use"))
    } else {
        e.into()
    }
}

fn item_json(item: Option<&StoredItem>) -> Result<Option<String>, DbError> {
    item.map(JsonDoc::to_json).transpose()
}

fn serial_to_db(serial: u64) -> Result<i64, DbError> {
    i64::try_from(serial).map_err(|_| bug(format!("item serial {serial} does not fit SQLite")))
}

// ---- friends ---------------------------------------------------------------------------------------------------

/// `UpsertFriend`: one row; a friendship is two rows and the caller sends both directions in one batch. An existing
/// row keeps its `created_at` and takes the new `blocked` flag.
pub fn upsert_friend(conn: &Connection, f: &FriendRecord) -> Result<(), DbError> {
    if f.character_id == f.friend_id {
        return Err(DbError::new(
            DbErrorKind::Conflict,
            format!("character {} cannot be its own friend", f.character_id),
        ));
    }
    conn.prepare_cached(
        "INSERT INTO friends (character_id, friend_id, blocked, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(character_id, friend_id) DO UPDATE SET blocked = excluded.blocked",
    )?
    .execute(params![
        f.character_id.0,
        f.friend_id.0,
        i64::from(f.blocked),
        f.created_at.0
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, "UpsertFriend"))
}

/// `DeleteFriendPair`: both rows of a friendship (either may be missing).
pub fn delete_friend_pair(
    conn: &Connection,
    a: CharacterId,
    b: CharacterId,
) -> Result<usize, DbError> {
    Ok(conn
        .prepare_cached(
            "DELETE FROM friends WHERE (character_id = ?1 AND friend_id = ?2)
                                    OR (character_id = ?2 AND friend_id = ?1)",
        )?
        .execute(params![a.0, b.0])?)
}

/// `DeleteAllFriendsOf`: every row where `character` is the owner or the friend (character delete and seal).
pub fn delete_all_friends_of(conn: &Connection, character: CharacterId) -> Result<usize, DbError> {
    Ok(conn
        .prepare_cached("DELETE FROM friends WHERE character_id = ?1 OR friend_id = ?1")?
        .execute([character.0])?)
}

const FRIEND_COLUMNS: &[&str] = &["character_id", "friend_id", "blocked", "created_at"];

/// The friend rows owned by `character` (enter-world load), oldest friendship first.
pub fn friends_of(conn: &Connection, character: CharacterId) -> Result<Vec<FriendRecord>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT character_id, friend_id, blocked, created_at FROM friends
         WHERE character_id = ?1 ORDER BY created_at, friend_id",
    )?;
    let mut rows = stmt.query([character.0])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut cols = Cols::new(row, FRIEND_COLUMNS, 0);
        out.push(FriendRecord {
            character_id: CharacterId(cols.int()?),
            friend_id: CharacterId(cols.int()?),
            blocked: cols.int::<i64>()? != 0,
            created_at: UnixTime(cols.int()?),
        });
    }
    Ok(out)
}

// ---- notes -----------------------------------------------------------------------------------------------------

/// `InsertNote`: returns the new `notes.id` (allocated by the database when the record's id is 0).
pub fn insert_note(conn: &Connection, n: &NoteRecord) -> Result<i64, DbError> {
    conn.prepare_cached(
        "INSERT INTO notes (id, to_id, from_id, text, created_at, delivered_at)
         VALUES (NULLIF(?1, 0), ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        n.id,
        n.to_id.0,
        n.from_id.0,
        n.text,
        n.created_at.0,
        n.delivered_at.map(|t| t.0)
    ])
    .map_err(|e| map_write_error(e, "InsertNote"))?;
    Ok(conn.last_insert_rowid())
}

/// `MarkNotesDelivered`: the undelivered notes of `to` with `id <= up_to_id` are delivered at `at`. The game calls it
/// only after the join succeeded (the legacy server marked them during the database load).
pub fn mark_notes_delivered(
    conn: &Connection,
    to: CharacterId,
    up_to_id: i64,
    at: UnixTime,
) -> Result<usize, DbError> {
    Ok(conn
        .prepare_cached(
            "UPDATE notes SET delivered_at = ?3
             WHERE to_id = ?1 AND id <= ?2 AND delivered_at IS NULL",
        )?
        .execute(params![to.0, up_to_id, at.0])?)
}

const NOTE_COLUMNS: &[&str] = &[
    "id",
    "to_id",
    "from_id",
    "text",
    "created_at",
    "delivered_at",
];

/// The undelivered notes of `to`, oldest first (enter-world load; the 20-note cap is a game rule).
pub fn undelivered_notes(conn: &Connection, to: CharacterId) -> Result<Vec<NoteRecord>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, to_id, from_id, text, created_at, delivered_at FROM notes
         WHERE to_id = ?1 AND delivered_at IS NULL ORDER BY id",
    )?;
    let mut rows = stmt.query([to.0])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut cols = Cols::new(row, NOTE_COLUMNS, 0);
        out.push(NoteRecord {
            id: cols.int()?,
            to_id: CharacterId(cols.int()?),
            from_id: CharacterId(cols.int()?),
            text: cols.text()?,
            created_at: UnixTime(cols.int()?),
            delivered_at: cols.opt_int::<i64>()?.map(UnixTime),
        });
    }
    Ok(out)
}

// ---- mails -----------------------------------------------------------------------------------------------------

/// `InsertMail`: the mail with the id the `MailService` allocated. `Conflict` for a used id, `NotFound` for an
/// unknown receiver. The sender has no foreign key (it may be purged, 0 = system).
pub fn insert_mail(conn: &Connection, m: &MailRecord) -> Result<(), DbError> {
    let item = item_json(m.item.as_ref())?;
    conn.prepare_cached(
        "INSERT INTO mails (id, receiver_id, sender_id, title, body, gold, item, created_at,
                            read_at, gold_taken_at, item_taken_at, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?
    .execute(params![
        m.id.0,
        m.receiver_id.0,
        m.sender_id.0,
        m.title,
        m.body,
        m.gold,
        item,
        m.created_at.0,
        m.read_at.map(|t| t.0),
        m.gold_taken_at.map(|t| t.0),
        m.item_taken_at.map(|t| t.0),
        m.deleted_at.map(|t| t.0),
    ])
    .map(|_| ())
    .map_err(|e| map_write_error(e, &format!("InsertMail {}", m.id)))
}

/// `UpdateMail`: applies the transitions of `u` (see [`MailStateRecord`]). `NotFound` for an unknown mail;
/// `Conflict` when a take finds the attachment gone or the mail deleted (the guard against duplicated items and
/// gold).
pub fn update_mail(conn: &Connection, u: &MailStateRecord) -> Result<(), DbError> {
    let current: Option<(i64, bool, Option<i64>)> = conn
        .prepare_cached("SELECT gold, item IS NOT NULL, deleted_at FROM mails WHERE id = ?1")?
        .query_row([u.id.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((gold, has_item, deleted_at)) = current else {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("UpdateMail: mail {} does not exist", u.id),
        ));
    };
    if u.take_gold_at.is_some() && (gold <= 0 || deleted_at.is_some()) {
        return Err(DbError::new(
            DbErrorKind::Conflict,
            format!(
                "UpdateMail: mail {} has no gold to take (gold {gold}, deleted {})",
                u.id,
                deleted_at.is_some()
            ),
        ));
    }
    if u.take_item_at.is_some() && (!has_item || deleted_at.is_some()) {
        return Err(DbError::new(
            DbErrorKind::Conflict,
            format!(
                "UpdateMail: mail {} has no item to take (item {has_item}, deleted {})",
                u.id,
                deleted_at.is_some()
            ),
        ));
    }
    conn.prepare_cached(
        "UPDATE mails SET
           read_at       = CASE WHEN ?2 IS NULL THEN read_at ELSE COALESCE(read_at, ?2) END,
           gold          = CASE WHEN ?3 IS NULL THEN gold ELSE 0 END,
           gold_taken_at = CASE WHEN ?3 IS NULL THEN gold_taken_at ELSE ?3 END,
           item          = CASE WHEN ?4 IS NULL THEN item ELSE NULL END,
           item_taken_at = CASE WHEN ?4 IS NULL THEN item_taken_at ELSE ?4 END,
           deleted_at    = CASE WHEN ?5 IS NULL THEN deleted_at ELSE COALESCE(deleted_at, ?5) END
         WHERE id = ?1",
    )?
    .execute(params![
        u.id.0,
        u.read_at.map(|t| t.0),
        u.take_gold_at.map(|t| t.0),
        u.take_item_at.map(|t| t.0),
        u.deleted_at.map(|t| t.0),
    ])?;
    Ok(())
}

/// `ExpireMails`: soft-deletes every undeleted mail sent before `created_before` (attachments are lost, KEEP).
/// Idempotent; returns the number of mails expired.
pub fn expire_mails(
    conn: &Connection,
    created_before: UnixTime,
    at: UnixTime,
) -> Result<usize, DbError> {
    Ok(conn
        .prepare_cached(
            "UPDATE mails SET deleted_at = ?2 WHERE deleted_at IS NULL AND created_at < ?1",
        )?
        .execute(params![created_before.0, at.0])?)
}

const MAIL_COLUMNS: &[&str] = &[
    "id",
    "receiver_id",
    "sender_id",
    "title",
    "body",
    "gold",
    "item",
    "created_at",
    "read_at",
    "gold_taken_at",
    "item_taken_at",
    "deleted_at",
];

const MAIL_SELECT: &str = "SELECT id, receiver_id, sender_id, title, body, gold, item, created_at, read_at, \
                           gold_taken_at, item_taken_at, deleted_at FROM mails";

/// Decodes one stored attachment. A newer document version refuses the load; a malformed one keeps the mail
/// without its item and logs the raw text (the legacy loader dropped the whole mail, PL-054).
fn stored_item(mail: u32, text: Option<String>) -> Result<Option<StoredItem>, DbError> {
    let Some(text) = text else { return Ok(None) };
    match StoredItem::from_json(&text) {
        Ok(item) => Ok(Some(item)),
        Err(e) if e.kind == DbErrorKind::NewerSchema => Err(e),
        Err(e) => {
            let raw: String = text.chars().take(512).collect();
            tracing::error!(mail, error = %e, raw, "mail attachment is unreadable: the mail is kept without its item");
            Ok(None)
        }
    }
}

fn read_mail(row: &rusqlite::Row<'_>) -> Result<MailRecord, DbError> {
    let mut cols = Cols::new(row, MAIL_COLUMNS, 0);
    let id: u32 = cols.int()?;
    let receiver_id = CharacterId(cols.int()?);
    let sender_id = CharacterId(cols.int()?);
    let title = cols.text()?;
    let body = cols.text()?;
    let gold = cols.int()?;
    let item_text = cols.opt_text()?;
    Ok(MailRecord {
        id: MailId(id),
        receiver_id,
        sender_id,
        title,
        body,
        gold,
        item: stored_item(id, item_text)?,
        created_at: UnixTime(cols.int()?),
        read_at: cols.opt_int::<i64>()?.map(UnixTime),
        gold_taken_at: cols.opt_int::<i64>()?.map(UnixTime),
        item_taken_at: cols.opt_int::<i64>()?.map(UnixTime),
        deleted_at: cols.opt_int::<i64>()?.map(UnixTime),
    })
}

/// Every undeleted mail, ordered by id (`StartupState.mails`; legacy `LoadPost`).
pub fn open_mails(conn: &Connection) -> Result<Vec<MailRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{MAIL_SELECT} WHERE deleted_at IS NULL ORDER BY id"
    ))?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_mail(row)?);
    }
    Ok(out)
}

/// One mail by id, deleted or not (admin tools, tests).
pub fn mail_by_id(conn: &Connection, id: MailId) -> Result<Option<MailRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!("{MAIL_SELECT} WHERE id = ?1"))?;
    let mut rows = stmt.query([id.0])?;
    rows.next()?.map(read_mail).transpose()
}

// ---- pending grants --------------------------------------------------------------------------------------------

/// `InsertPendingGrant`: returns the new `pending_grants.id`. The database enforces the shape (`item` for an item
/// grant, `gold > 0` for a gold grant): a record that violates it is a `Bug`.
pub fn insert_pending_grant(conn: &Connection, g: &PendingGrantRecord) -> Result<i64, DbError> {
    let item = item_json(g.item.as_ref())?;
    conn.prepare_cached(
        "INSERT INTO pending_grants (id, character_id, kind, item, gold, source, note, created_at, delivered_at)
         VALUES (NULLIF(?1, 0), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?
    .execute(params![
        g.id,
        g.character_id.0,
        g.kind.as_str(),
        item,
        g.gold,
        g.source,
        g.note,
        g.created_at.0,
        g.delivered_at.map(|t| t.0),
    ])
    .map_err(|e| map_write_error(e, "InsertPendingGrant"))?;
    Ok(conn.last_insert_rowid())
}

/// `MarkGrantsDelivered`: closes the grants `ids` at `at`. A grant that is unknown, already delivered or
/// **cancelled** (`delivered_at` stays `NULL` for a cancelled row: a row is never both, Plan/25 §5.6) was
/// delivered in memory before, so the batch is **not** rolled back: it is logged as CRITICAL and counted in the
/// return value `(closed, anomalies)`.
pub fn mark_grants_delivered(
    conn: &Connection,
    ids: &[i64],
    at: UnixTime,
) -> Result<(usize, usize), DbError> {
    let mut stmt = conn.prepare_cached(
        "UPDATE pending_grants SET delivered_at = ?2
         WHERE id = ?1 AND delivered_at IS NULL AND cancelled_at IS NULL",
    )?;
    let mut closed = 0;
    let mut anomalies = 0;
    for id in ids {
        let n = stmt.execute(params![id, at.0])?;
        if n == 0 {
            anomalies += 1;
            tracing::error!(
                grant = id,
                "CRITICAL: grant delivered twice, cancelled or unknown (MarkGrantsDelivered found no open grant)"
            );
        }
        closed += n;
    }
    Ok((closed, anomalies))
}

/// `CancelPendingGrant`: cancels a grant that is neither delivered nor cancelled yet. Returns the number of rows
/// changed: `0` when the grant was delivered (or cancelled before, or does not exist) - the conditional update of
/// Plan/25 §5.6, so a cancel racing a delivery can never produce a row that is both.
pub fn cancel_pending_grant(
    conn: &Connection,
    id: i64,
    by: &str,
    at: UnixTime,
) -> Result<u32, DbError> {
    let n = conn
        .prepare_cached(
            "UPDATE pending_grants SET cancelled_at = ?2, cancelled_by = ?3
             WHERE id = ?1 AND delivered_at IS NULL AND cancelled_at IS NULL",
        )?
        .execute(params![id, at.0, by])?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

const GRANT_COLUMNS: &[&str] = &[
    "id",
    "character_id",
    "kind",
    "item",
    "gold",
    "source",
    "note",
    "created_at",
    "delivered_at",
    "cancelled_at",
    "cancelled_by",
];

const GRANT_SELECT: &str = "SELECT id, character_id, kind, item, gold, source, note, created_at, delivered_at, \
                            cancelled_at, cancelled_by FROM pending_grants";

/// The columns of [`GRANT_COLUMNS`] with the alias `g.` (admin lists that join the character name).
pub(crate) const GRANT_COLUMNS_G: &str = "g.id, g.character_id, g.kind, g.item, g.gold, g.source, g.note, \
                                          g.created_at, g.delivered_at, g.cancelled_at, g.cancelled_by";

/// Decodes a grant row; `None` (logged) when it cannot be delivered because its kind or item is unreadable:
/// the row stays in the table for the operator and never blocks a character from entering the world.
pub(crate) fn read_grant(row: &rusqlite::Row<'_>) -> Result<Option<PendingGrantRecord>, DbError> {
    let mut cols = Cols::new(row, GRANT_COLUMNS, 0);
    let id: i64 = cols.int()?;
    let character_id = CharacterId(cols.int()?);
    let kind_text = cols.text()?;
    let item_text = cols.opt_text()?;
    let gold = cols.int()?;
    let source = cols.text()?;
    let note = cols.text()?;
    let created_at = UnixTime(cols.int()?);
    let delivered_at = cols.opt_int::<i64>()?.map(UnixTime);
    let cancelled_at = cols.opt_int::<i64>()?.map(UnixTime);
    let cancelled_by = cols.opt_text()?;
    let Some(kind) = GrantKind::parse(&kind_text) else {
        tracing::error!(grant = id, kind = kind_text, "grant skipped: unknown kind");
        return Ok(None);
    };
    let item = match item_text {
        None => None,
        Some(text) => match StoredItem::from_json(&text) {
            Ok(item) => Some(item),
            Err(e) if e.kind == DbErrorKind::NewerSchema => return Err(e),
            Err(e) => {
                tracing::error!(grant = id, error = %e, "grant skipped: its item is unreadable");
                return Ok(None);
            }
        },
    };
    Ok(Some(PendingGrantRecord {
        id,
        character_id,
        kind,
        item,
        gold,
        source,
        note,
        created_at,
        delivered_at,
        cancelled_at,
        cancelled_by,
    }))
}

/// The open grants of one character, oldest first (enter-world load).
pub fn open_grants_of(
    conn: &Connection,
    character: CharacterId,
) -> Result<Vec<PendingGrantRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{GRANT_SELECT} WHERE character_id = ?1 AND delivered_at IS NULL AND cancelled_at IS NULL ORDER BY id"
    ))?;
    let mut rows = stmt.query([character.0])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.extend(read_grant(row)?);
    }
    Ok(out)
}

/// `GameReadQuery::OpenGrants`: the open grants with `id > after_id` of the given (online) characters, oldest
/// first, at most [`MAX_GRANT_POLL_ROWS`].
pub fn open_grants(
    conn: &Connection,
    characters: &[CharacterId],
    after_id: i64,
) -> Result<Vec<PendingGrantRecord>, DbError> {
    if characters.is_empty() {
        return Ok(Vec::new());
    }
    let ids = serde_json::to_string(&characters.iter().map(|c| c.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(&format!(
        "{GRANT_SELECT} WHERE delivered_at IS NULL AND cancelled_at IS NULL AND id > ?1
           AND character_id IN (SELECT value FROM json_each(?2))
         ORDER BY id LIMIT {MAX_GRANT_POLL_ROWS}"
    ))?;
    let mut rows = stmt.query(params![after_id, ids])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.extend(read_grant(row)?);
    }
    Ok(out)
}

// ---- pending removals ------------------------------------------------------------------------------------------

/// `InsertPendingRemoval`: returns the new `pending_removals.id`. Exactly one of `character_id` / `guild_id`
/// must be set and the location must agree (`guild_bank` <=> guild); the schema checks it.
pub fn insert_pending_removal(conn: &Connection, r: &PendingRemovalRecord) -> Result<i64, DbError> {
    let serial = r.serial.map(serial_to_db).transpose()?;
    conn.prepare_cached(
        "INSERT INTO pending_removals (id, character_id, guild_id, location, item_id, serial, upgrade_level,
                                       element, element_level, count, requested_by, reason, created_at,
                                       done_at, removed_count)
         VALUES (NULLIF(?1, 0), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )?
    .execute(params![
        r.id,
        r.character_id.map(|c| c.0),
        r.guild_id.map(|g| g.0),
        r.location.as_str(),
        r.item_id,
        serial,
        r.upgrade_level,
        r.element,
        r.element_level,
        r.count,
        r.requested_by,
        r.reason,
        r.created_at.0,
        r.done_at.map(|t| t.0),
        r.removed_count,
    ])
    .map_err(|e| map_write_error(e, "InsertPendingRemoval"))?;
    Ok(conn.last_insert_rowid())
}

/// `MarkRemovalsDone`: records what was actually removed for each `(id, removed_count)`. Like the grants, an
/// unknown, already closed or cancelled removal is logged as CRITICAL and does not roll the batch back.
pub fn mark_removals_done(
    conn: &Connection,
    done: &[(i64, u32)],
    at: UnixTime,
) -> Result<(usize, usize), DbError> {
    let mut stmt = conn.prepare_cached(
        "UPDATE pending_removals SET done_at = ?2, removed_count = ?3
         WHERE id = ?1 AND done_at IS NULL AND cancelled_at IS NULL",
    )?;
    let mut closed = 0;
    let mut anomalies = 0;
    for (id, removed) in done {
        let n = stmt.execute(params![id, at.0, removed])?;
        if n == 0 {
            anomalies += 1;
            tracing::error!(
                removal = id,
                "CRITICAL: removal applied twice, cancelled or unknown (MarkRemovalsDone found no open removal)"
            );
        }
        closed += n;
    }
    Ok((closed, anomalies))
}

const REMOVAL_COLUMNS: &[&str] = &[
    "id",
    "character_id",
    "guild_id",
    "location",
    "item_id",
    "serial",
    "upgrade_level",
    "element",
    "element_level",
    "count",
    "requested_by",
    "reason",
    "created_at",
    "done_at",
    "removed_count",
    "cancelled_at",
    "cancelled_by",
];

const REMOVAL_SELECT: &str = "SELECT id, character_id, guild_id, location, item_id, serial, upgrade_level, \
                              element, element_level, count, requested_by, reason, created_at, done_at, \
                              removed_count, cancelled_at, cancelled_by FROM pending_removals";

/// The columns of [`REMOVAL_COLUMNS`] with the alias `r.` (admin lists that join the character name).
pub(crate) const REMOVAL_COLUMNS_R: &str = "r.id, r.character_id, r.guild_id, r.location, r.item_id, r.serial, \
     r.upgrade_level, r.element, r.element_level, r.count, r.requested_by, r.reason, r.created_at, r.done_at, \
     r.removed_count, r.cancelled_at, r.cancelled_by";

/// `CancelPendingRemoval`: cancels a removal that is neither applied nor cancelled yet; `0` rows changed when it
/// was applied already (see [`cancel_pending_grant`]).
pub fn cancel_pending_removal(
    conn: &Connection,
    id: i64,
    by: &str,
    at: UnixTime,
) -> Result<u32, DbError> {
    let n = conn
        .prepare_cached(
            "UPDATE pending_removals SET cancelled_at = ?2, cancelled_by = ?3
             WHERE id = ?1 AND done_at IS NULL AND cancelled_at IS NULL",
        )?
        .execute(params![id, at.0, by])?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

pub(crate) fn read_removal(
    row: &rusqlite::Row<'_>,
) -> Result<Option<PendingRemovalRecord>, DbError> {
    let mut cols = Cols::new(row, REMOVAL_COLUMNS, 0);
    let id: i64 = cols.int()?;
    let character_id = cols.opt_int::<u32>()?.map(CharacterId);
    let guild_id = cols.opt_int::<u32>()?.map(GuildId);
    let location_text = cols.text()?;
    let Some(location) = RemovalLocation::parse(&location_text) else {
        tracing::error!(
            removal = id,
            location = location_text,
            "removal skipped: unknown location"
        );
        return Ok(None);
    };
    Ok(Some(PendingRemovalRecord {
        id,
        character_id,
        guild_id,
        location,
        item_id: cols.int()?,
        serial: cols.opt_int::<u64>()?,
        upgrade_level: cols.opt_int()?,
        element: cols.opt_int()?,
        element_level: cols.opt_int()?,
        count: cols.int()?,
        requested_by: cols.text()?,
        reason: cols.text()?,
        created_at: UnixTime(cols.int()?),
        done_at: cols.opt_int::<i64>()?.map(UnixTime),
        removed_count: cols.opt_int()?,
        cancelled_at: cols.opt_int::<i64>()?.map(UnixTime),
        cancelled_by: cols.opt_text()?,
    }))
}

/// The open character removals of one character, oldest first (enter-world load, applied before the join).
pub fn open_removals_of(
    conn: &Connection,
    character: CharacterId,
) -> Result<Vec<PendingRemovalRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{REMOVAL_SELECT} WHERE character_id = ?1 AND done_at IS NULL AND cancelled_at IS NULL ORDER BY id"
    ))?;
    let mut rows = stmt.query([character.0])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.extend(read_removal(row)?);
    }
    Ok(out)
}

/// `GameReadQuery::OpenGuildRemovals`: the open guild-bank removals with `id > after_id`, oldest first.
pub fn open_guild_removals(
    conn: &Connection,
    after_id: i64,
) -> Result<Vec<PendingRemovalRecord>, DbError> {
    let mut stmt = conn.prepare_cached(&format!(
        "{REMOVAL_SELECT} WHERE guild_id IS NOT NULL AND done_at IS NULL AND cancelled_at IS NULL AND id > ?1 ORDER BY id"
    ))?;
    let mut rows = stmt.query([after_id])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.extend(read_removal(row)?);
    }
    Ok(out)
}
