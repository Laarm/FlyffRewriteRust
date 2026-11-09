//! The admin reads (Plan/25 §11.6 "Admin reads"): [`AdminReadQuery`] and the simple reads built on the chunked
//! scan framework ([`super::scan`]): bookmarks, templates, notes, scheduled notices, IP blocks, staff accounts,
//! pending grants and removals, the TOTP secret and idempotency rows.
//!
//! ```text
//! let grants = persist.admin_read(AdminReadQuery::PendingGrants(PendingFilter::open())).await?.grants()?;
//! ```
//!
//! # Adding a read (WP-ADMIN-16/17/20/22)
//!
//! `SearchCharacters`, `CharacterDocs`, `AccountBanks`, `AccountDetail`, `Alts`, `ItemSearch`,
//! `DuplicateSerials` and `EconomySummary` belong to the feature work packages. Each one either
//!
//! * implements [`AdminScan`] in its own module and is run with [`PersistHandle::admin_scan`] (streams, progress,
//!   cancel; heavy ones return `true` from `heavy()`), or
//! * is added as a variant of [`AdminReadQuery`] / [`AdminReadResult`] (both are `#[non_exhaustive]`) with a
//!   `plan` arm below that returns its scan and how the rows are assembled.
//!
//! Lists are cursor paged (`Page`, newest first): `limit` is `1..=500` (0 = 100) and `next_cursor` is the id to
//! pass back as `cursor`.

use super::idempotency;
use super::model::{
    AdminBookmarkRecord, AdminNoteRecord, AdminTemplateRecord, IdempotencyRecord, IpBlockRecord,
    LayerPolicy, NoteSubject, NoteSubjectKind, ScheduledNoticeRecord, StaffAccountRecord,
    TemplateKind, TotpRecord,
};
use super::scan::{AdminScan, KeysetScan, Once, page_limit};
use crate::error::{DbError, DbErrorKind};
use crate::handle::PersistHandle;
use crate::model::{PendingGrantRecord, PendingRemovalRecord};
use crate::repo::messaging;
use flyff_common::{AccountId, CharacterId, UnixTime};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

/// The page size used when a request says `limit = 0`.
pub const DEFAULT_PAGE: u32 = 100;

/// Which pending rows a list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingState {
    /// Neither delivered/applied nor cancelled.
    Open,
    /// Delivered (grants) or applied (removals).
    Done,
    /// Cancelled by an administrator.
    Cancelled,
    /// Any state.
    All,
}

/// Selection and paging of `PendingGrants` / `PendingRemovals` (newest first).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingFilter {
    /// Which state.
    pub state: PendingState,
    /// Only this row (a point read, e.g. to tell "delivered" from "unknown" after a cancel changed 0 rows).
    pub id: Option<i64>,
    /// Only rows of this character.
    pub character: Option<CharacterId>,
    /// Only rows whose state time (creation for `Open`/`All`, delivery for `Done`, cancellation for `Cancelled`)
    /// is at or after this ("the last 30 days", Plan/25 §5.6).
    pub since: Option<UnixTime>,
    /// Continue after this id (the `next_cursor` of the previous page).
    pub cursor: Option<i64>,
    /// Page size, `1..=500`; 0 = [`DEFAULT_PAGE`].
    pub limit: u32,
}

impl PendingFilter {
    /// All rows of `state`, first page.
    pub fn of(state: PendingState) -> Self {
        Self {
            state,
            id: None,
            character: None,
            since: None,
            cursor: None,
            limit: DEFAULT_PAGE,
        }
    }

    /// The open rows, first page.
    pub fn open() -> Self {
        Self::of(PendingState::Open)
    }
}

/// One page of a newest-first list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    /// The rows of the page.
    pub items: Vec<T>,
    /// Pass as `cursor` to get the next page; `None` on the last page.
    pub next_cursor: Option<i64>,
}

impl<T> Page<T> {
    fn of(mut items: Vec<T>, limit: usize, id_of: fn(&T) -> i64) -> Page<T> {
        let next_cursor = if items.len() > limit {
            items.truncate(limit);
            items.last().map(id_of)
        } else {
            None
        };
        Page { items, next_cursor }
    }
}

/// A pending grant with the name of the receiving character.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminGrantRow {
    /// The row.
    pub grant: PendingGrantRecord,
    /// `characters.name` of `grant.character_id`.
    pub character_name: String,
}

/// A pending removal with the names of its character or guild.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminRemovalRow {
    /// The row.
    pub removal: PendingRemovalRecord,
    /// `characters.name` of a character removal.
    pub character_name: Option<String>,
    /// `guilds.name` of a guild-bank removal.
    pub guild_name: Option<String>,
}

/// An admin read (Plan/25 §11.6). Run with [`PersistHandle::admin_read`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AdminReadQuery {
    /// Every shared bookmark, ordered by group, `sort`, name -> [`AdminReadResult::Bookmarks`].
    Bookmarks,
    /// Item templates and kits, optionally of one kind, ordered by kind and name -> `Templates`.
    Templates {
        /// Only this kind.
        kind: Option<TemplateKind>,
    },
    /// The moderation notes of one subject, pinned first, then newest first -> `Notes`.
    Notes {
        /// Account or character.
        subject: NoteSubject,
    },
    /// All moderation notes, newest first, paged (Moderation -> Notes) -> `NotePage`.
    RecentNotes {
        /// Continue after this id.
        cursor: Option<i64>,
        /// Page size (`0` = [`DEFAULT_PAGE`]).
        limit: u32,
    },
    /// Every scheduled notice by id -> `ScheduledNotices`.
    ScheduledNotices,
    /// The IP blocks by id; with `active_at` only those in force at that time (the live set the `net.ip_block`
    /// middleware merges) -> `IpBlocks`.
    IpBlocks {
        /// Only blocks active at this time.
        active_at: Option<UnixTime>,
    },
    /// Accounts whose stored authority letter is at least `min_authority`, plus the accounts named in
    /// `extra_accounts` (those that `[gm]` grants raise above the letter), by id -> `StaffAccounts`.
    StaffAccounts {
        /// Lowest stored letter that counts as staff (`b'L'`).
        min_authority: u8,
        /// More account names (case-insensitive).
        extra_accounts: Vec<String>,
    },
    /// Pending grants, newest first -> `Grants`.
    PendingGrants(PendingFilter),
    /// Pending removals, newest first -> `Removals`.
    PendingRemovals(PendingFilter),
    /// The TOTP secret of a staff account -> `Totp`.
    Totp {
        /// The account.
        account: AccountId,
    },
    /// One idempotency row -> `Idempotency`.
    Idempotency {
        /// The `Idempotency-Key`.
        key: String,
    },
}

/// The answer to an [`AdminReadQuery`] (one variant per query).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AdminReadResult {
    /// `Bookmarks`.
    Bookmarks(Vec<AdminBookmarkRecord>),
    /// `Templates`.
    Templates(Vec<AdminTemplateRecord>),
    /// `Notes`.
    Notes(Vec<AdminNoteRecord>),
    /// `RecentNotes`.
    NotePage(Page<AdminNoteRecord>),
    /// `ScheduledNotices`.
    ScheduledNotices(Vec<ScheduledNoticeRecord>),
    /// `IpBlocks`.
    IpBlocks(Vec<IpBlockRecord>),
    /// `StaffAccounts`.
    StaffAccounts(Vec<StaffAccountRecord>),
    /// `PendingGrants`.
    Grants(Page<AdminGrantRow>),
    /// `PendingRemovals`.
    Removals(Page<AdminRemovalRow>),
    /// `Totp`.
    Totp(Option<TotpRecord>),
    /// `Idempotency`.
    Idempotency(Option<IdempotencyRecord>),
}

macro_rules! accessors {
    ($($name:ident, $variant:ident, $ty:ty;)+) => {
        impl AdminReadResult {
            $(
                #[doc = concat!("The `", stringify!($variant), "` answer; `Bug` when the result is another one.")]
                pub fn $name(self) -> Result<$ty, DbError> {
                    match self {
                        AdminReadResult::$variant(v) => Ok(v),
                        other => Err(DbError::new(
                            DbErrorKind::Bug,
                            format!("expected {} but got {other:?}", stringify!($variant)),
                        )),
                    }
                }
            )+
        }
    };
}

accessors! {
    bookmarks, Bookmarks, Vec<AdminBookmarkRecord>;
    templates, Templates, Vec<AdminTemplateRecord>;
    notes, Notes, Vec<AdminNoteRecord>;
    note_page, NotePage, Page<AdminNoteRecord>;
    scheduled_notices, ScheduledNotices, Vec<ScheduledNoticeRecord>;
    ip_blocks, IpBlocks, Vec<IpBlockRecord>;
    staff_accounts, StaffAccounts, Vec<StaffAccountRecord>;
    grants, Grants, Page<AdminGrantRow>;
    removals, Removals, Page<AdminRemovalRow>;
    totp, Totp, Option<TotpRecord>;
    idempotency, Idempotency, Option<IdempotencyRecord>;
}

/// One row of whatever a planned scan produces.
pub(crate) enum AdminRow {
    Bookmark(AdminBookmarkRecord),
    Template(AdminTemplateRecord),
    Note(AdminNoteRecord),
    Notice(ScheduledNoticeRecord),
    IpBlock(IpBlockRecord),
    Staff(StaffAccountRecord),
    Grant(AdminGrantRow),
    Removal(AdminRemovalRow),
    Totp(TotpRecord),
    Idempotency(IdempotencyRecord),
}

/// How the rows of a planned scan become the result.
#[derive(Clone, Copy)]
enum Shape {
    Bookmarks,
    Templates,
    Notes,
    NotePage(usize),
    ScheduledNotices,
    IpBlocks,
    StaffAccounts,
    Grants(usize),
    Removals(usize),
    Totp,
    Idempotency,
}

type BoxedScan = Box<dyn AdminScan<Item = AdminRow>>;

fn page_size(limit: u32) -> Result<usize, DbError> {
    page_limit(if limit == 0 { DEFAULT_PAGE } else { limit })
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn row_err(table: &str, what: &str) -> DbError {
    DbError::new(
        DbErrorKind::Bug,
        format!("{table}: unreadable row ({what}); the row is skipped"),
    )
}

// ---- decoders --------------------------------------------------------------------------------------------------

fn decode_bookmark(row: &Row<'_>) -> Result<Option<AdminBookmarkRecord>, DbError> {
    let policy: String = row.get(4)?;
    let layer_policy = LayerPolicy::parse(&policy).unwrap_or_else(|| {
        tracing::warn!(
            policy,
            "admin_bookmarks: unknown layer_policy, using fixed0"
        );
        LayerPolicy::Fixed0
    });
    Ok(Some(AdminBookmarkRecord {
        id: row.get(0)?,
        group: row.get(1)?,
        name: row.get(2)?,
        world_id: row.get(3)?,
        layer_policy,
        x: row.get::<_, f64>(5)? as f32,
        z: row.get::<_, f64>(6)? as f32,
        zoom: row.get::<_, Option<f64>>(7)?.map(|z| z as f32),
        sort: row.get(8)?,
        created_by: row.get(9)?,
        created_at: UnixTime(row.get(10)?),
        updated_at: UnixTime(row.get(11)?),
    }))
}

fn decode_template(row: &Row<'_>) -> Result<Option<AdminTemplateRecord>, DbError> {
    let id: i64 = row.get(0)?;
    let kind: String = row.get(1)?;
    let spec: String = row.get(3)?;
    let (Some(kind), Ok(spec)) = (TemplateKind::parse(&kind), serde_json::from_str(&spec)) else {
        tracing::error!(
            template = id,
            "{}",
            row_err("admin_templates", "kind or spec")
        );
        return Ok(None);
    };
    Ok(Some(AdminTemplateRecord {
        id,
        kind,
        name: row.get(2)?,
        spec,
        created_by: row.get(4)?,
        created_at: UnixTime(row.get(5)?),
        updated_at: UnixTime(row.get(6)?),
    }))
}

fn decode_note(row: &Row<'_>) -> Result<Option<AdminNoteRecord>, DbError> {
    let id: i64 = row.get(0)?;
    let kind: String = row.get(1)?;
    let Some(kind) = NoteSubjectKind::parse(&kind) else {
        tracing::error!(note = id, "{}", row_err("admin_notes", "subject_kind"));
        return Ok(None);
    };
    Ok(Some(AdminNoteRecord {
        id,
        subject: NoteSubject {
            kind,
            id: row.get(2)?,
        },
        text: row.get(3)?,
        pinned: row.get::<_, i64>(4)? != 0,
        author: row.get(5)?,
        created_at: UnixTime(row.get(6)?),
    }))
}

fn decode_notice(row: &Row<'_>) -> Result<Option<ScheduledNoticeRecord>, DbError> {
    Ok(Some(ScheduledNoticeRecord {
        id: row.get(0)?,
        text: row.get(1)?,
        style: row.get(2)?,
        target: row.get(3)?,
        start_at: UnixTime(row.get(4)?),
        repeat_min: row.get(5)?,
        daily_at: row.get(6)?,
        end_at: row.get::<_, Option<i64>>(7)?.map(UnixTime),
        paused: row.get::<_, i64>(8)? != 0,
        created_by: row.get(9)?,
        created_at: UnixTime(row.get(10)?),
        last_fired_at: row.get::<_, Option<i64>>(11)?.map(UnixTime),
    }))
}

fn decode_ip_block(row: &Row<'_>) -> Result<Option<IpBlockRecord>, DbError> {
    Ok(Some(IpBlockRecord {
        id: row.get(0)?,
        cidr: row.get(1)?,
        reason: row.get(2)?,
        until: row.get::<_, Option<i64>>(3)?.map(UnixTime),
        created_by: row.get(4)?,
        created_at: UnixTime(row.get(5)?),
    }))
}

fn decode_staff(row: &Row<'_>) -> Result<Option<StaffAccountRecord>, DbError> {
    let authority: String = row.get(2)?;
    Ok(Some(StaffAccountRecord {
        id: AccountId(row.get(0)?),
        name: row.get(1)?,
        authority: authority.bytes().next().unwrap_or(0),
        created_at: UnixTime(row.get(3)?),
        last_login_at: row.get::<_, Option<i64>>(4)?.map(UnixTime),
        last_login_ip: row.get(5)?,
        must_change_password: row.get::<_, i64>(6)? != 0,
        totp_enabled_at: row.get::<_, Option<i64>>(7)?.map(UnixTime),
    }))
}

fn decode_grant(row: &Row<'_>) -> Result<Option<AdminGrantRow>, DbError> {
    let Some(grant) = messaging::read_grant(row)? else {
        return Ok(None);
    };
    Ok(Some(AdminGrantRow {
        grant,
        character_name: row.get(11)?,
    }))
}

fn decode_removal(row: &Row<'_>) -> Result<Option<AdminRemovalRow>, DbError> {
    let Some(removal) = messaging::read_removal(row)? else {
        return Ok(None);
    };
    Ok(Some(AdminRemovalRow {
        removal,
        character_name: row.get(17)?,
        guild_name: row.get(18)?,
    }))
}

// ---- pending lists ---------------------------------------------------------------------------------------------

/// The `WHERE` extension and parameters shared by the two pending lists. `alias` is `g` or `r`, `done` the
/// column that closes the row (`delivered_at` / `done_at`).
fn pending_conditions(filter: &PendingFilter, alias: &str, done: &str) -> (String, Vec<Value>) {
    let mut sql = match filter.state {
        PendingState::Open => {
            format!(" AND {alias}.{done} IS NULL AND {alias}.cancelled_at IS NULL")
        }
        PendingState::Done => format!(" AND {alias}.{done} IS NOT NULL"),
        PendingState::Cancelled => format!(" AND {alias}.cancelled_at IS NOT NULL"),
        PendingState::All => String::new(),
    };
    let mut params = Vec::new();
    let mut next = 3;
    if let Some(character) = filter.character {
        sql.push_str(&format!(" AND {alias}.character_id = ?{next}"));
        params.push(Value::Integer(i64::from(character.0)));
        next += 1;
    }
    if let Some(id) = filter.id {
        sql.push_str(&format!(" AND {alias}.id = ?{next}"));
        params.push(Value::Integer(id));
        next += 1;
    }
    if let Some(since) = filter.since {
        let column = match filter.state {
            PendingState::Done => format!("{alias}.{done}"),
            PendingState::Cancelled => format!("{alias}.cancelled_at"),
            PendingState::Open | PendingState::All => format!("{alias}.created_at"),
        };
        sql.push_str(&format!(" AND {column} >= ?{next}"));
        params.push(Value::Integer(since.0));
    }
    (sql, params)
}

// ---- planning --------------------------------------------------------------------------------------------------

fn plan(query: &AdminReadQuery) -> Result<(BoxedScan, Shape), DbError> {
    use super::scan::AdminScanExt;
    Ok(match query {
        AdminReadQuery::Bookmarks => (
            Box::new(
                KeysetScan::ascending(
                    "SELECT id, grp, name, world, layer_policy, x, z, zoom, sort, created_by, created_at, updated_at
                     FROM admin_bookmarks WHERE id > ?1 ORDER BY id LIMIT ?2",
                    Vec::new(),
                    decode_bookmark,
                )
                .mapped(AdminRow::Bookmark),
            ),
            Shape::Bookmarks,
        ),
        AdminReadQuery::Templates { kind } => (
            Box::new(
                KeysetScan::ascending(
                    "SELECT id, kind, name, spec, created_by, created_at, updated_at
                     FROM admin_templates WHERE id > ?1 AND (?3 IS NULL OR kind = ?3) ORDER BY id LIMIT ?2",
                    vec![kind.map_or(Value::Null, |k| text(k.as_str()))],
                    decode_template,
                )
                .mapped(AdminRow::Template),
            ),
            Shape::Templates,
        ),
        AdminReadQuery::Notes { subject } => (
            Box::new(
                KeysetScan::ascending(
                    "SELECT id, subject_kind, subject_id, text, pinned, author, created_at
                     FROM admin_notes WHERE id > ?1 AND subject_kind = ?3 AND subject_id = ?4
                     ORDER BY id LIMIT ?2",
                    vec![text(subject.kind.as_str()), Value::Integer(i64::from(subject.id))],
                    decode_note,
                )
                .mapped(AdminRow::Note),
            ),
            Shape::Notes,
        ),
        AdminReadQuery::RecentNotes { cursor, limit } => {
            let size = page_size(*limit)?;
            (
                Box::new(
                    KeysetScan::descending(
                        "SELECT id, subject_kind, subject_id, text, pinned, author, created_at
                         FROM admin_notes WHERE id < ?1 ORDER BY id DESC LIMIT ?2",
                        Vec::new(),
                        *cursor,
                        decode_note,
                    )
                    .limited(size + 1)
                    .mapped(AdminRow::Note),
                ),
                Shape::NotePage(size),
            )
        }
        AdminReadQuery::ScheduledNotices => (
            Box::new(
                KeysetScan::ascending(
                    "SELECT id, text, style, target, start_at, repeat_min, daily_at, end_at, paused, created_by,
                            created_at, last_fired_at
                     FROM scheduled_notices WHERE id > ?1 ORDER BY id LIMIT ?2",
                    Vec::new(),
                    decode_notice,
                )
                .mapped(AdminRow::Notice),
            ),
            Shape::ScheduledNotices,
        ),
        AdminReadQuery::IpBlocks { active_at } => (
            Box::new(
                KeysetScan::ascending(
                    "SELECT id, cidr, reason, until, created_by, created_at FROM admin_ip_blocks
                     WHERE id > ?1 AND (?3 IS NULL OR until IS NULL OR until > ?3) ORDER BY id LIMIT ?2",
                    vec![active_at.map_or(Value::Null, |t| Value::Integer(t.0))],
                    decode_ip_block,
                )
                .mapped(AdminRow::IpBlock),
            ),
            Shape::IpBlocks,
        ),
        AdminReadQuery::StaffAccounts {
            min_authority,
            extra_accounts,
        } => {
            if !min_authority.is_ascii_alphabetic() {
                return Err(super::model::invalid("min_authority must be an ASCII letter"));
            }
            (
                Box::new(
                    KeysetScan::ascending(
                        "SELECT a.id, a.name, a.authority, a.created_at, a.last_login_at, a.last_login_ip,
                                a.must_change_password, t.enabled_at
                         FROM accounts a LEFT JOIN admin_totp t ON t.account_id = a.id
                         WHERE a.id > ?1
                           AND (a.authority >= ?3 OR a.name IN (SELECT value FROM json_each(?4)))
                         ORDER BY a.id LIMIT ?2",
                        vec![
                            text(&char::from(*min_authority).to_string()),
                            Value::Text(serde_json::to_string(extra_accounts)?),
                        ],
                        decode_staff,
                    )
                    .mapped(AdminRow::Staff),
                ),
                Shape::StaffAccounts,
            )
        }
        AdminReadQuery::PendingGrants(filter) => {
            let size = page_size(filter.limit)?;
            let (conditions, params) = pending_conditions(filter, "g", "delivered_at");
            let sql = format!(
                "SELECT {}, c.name FROM pending_grants g JOIN characters c ON c.id = g.character_id
                 WHERE g.id < ?1{conditions} ORDER BY g.id DESC LIMIT ?2",
                messaging::GRANT_COLUMNS_G
            );
            (
                Box::new(
                    KeysetScan::descending(sql, params, filter.cursor, decode_grant)
                        .limited(size + 1)
                        .mapped(AdminRow::Grant),
                ),
                Shape::Grants(size),
            )
        }
        AdminReadQuery::PendingRemovals(filter) => {
            let size = page_size(filter.limit)?;
            let (conditions, params) = pending_conditions(filter, "r", "done_at");
            let sql = format!(
                "SELECT {}, c.name, gu.name FROM pending_removals r
                 LEFT JOIN characters c ON c.id = r.character_id
                 LEFT JOIN guilds gu ON gu.id = r.guild_id
                 WHERE r.id < ?1{conditions} ORDER BY r.id DESC LIMIT ?2",
                messaging::REMOVAL_COLUMNS_R
            );
            (
                Box::new(
                    KeysetScan::descending(sql, params, filter.cursor, decode_removal)
                        .limited(size + 1)
                        .mapped(AdminRow::Removal),
                ),
                Shape::Removals(size),
            )
        }
        AdminReadQuery::Totp { account } => {
            let account = *account;
            (
                Box::new(
                    Once::new(move |conn: &Connection| Ok(totp(conn, account)?.into_iter().collect()))
                        .mapped(AdminRow::Totp),
                ),
                Shape::Totp,
            )
        }
        AdminReadQuery::Idempotency { key } => {
            let key = key.clone();
            (
                Box::new(
                    Once::new(move |conn: &Connection| {
                        Ok(idempotency::get(conn, &key)?.into_iter().collect())
                    })
                    .mapped(AdminRow::Idempotency),
                ),
                Shape::Idempotency,
            )
        }
    })
}

macro_rules! take {
    ($rows:expr, $variant:ident) => {
        $rows
            .into_iter()
            .filter_map(|row| match row {
                AdminRow::$variant(v) => Some(v),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
}

fn assemble(shape: Shape, rows: Vec<AdminRow>) -> AdminReadResult {
    match shape {
        Shape::Bookmarks => {
            let mut list = take!(rows, Bookmark);
            list.sort_by(|a, b| {
                (a.group.to_lowercase(), a.sort, a.name.to_lowercase(), a.id).cmp(&(
                    b.group.to_lowercase(),
                    b.sort,
                    b.name.to_lowercase(),
                    b.id,
                ))
            });
            AdminReadResult::Bookmarks(list)
        }
        Shape::Templates => {
            let mut list = take!(rows, Template);
            list.sort_by(|a, b| {
                (a.kind.as_str(), a.name.to_lowercase(), a.id).cmp(&(
                    b.kind.as_str(),
                    b.name.to_lowercase(),
                    b.id,
                ))
            });
            AdminReadResult::Templates(list)
        }
        Shape::Notes => {
            let mut list = take!(rows, Note);
            list.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.id.cmp(&a.id)));
            AdminReadResult::Notes(list)
        }
        Shape::NotePage(limit) => {
            AdminReadResult::NotePage(Page::of(take!(rows, Note), limit, |n| n.id))
        }
        Shape::ScheduledNotices => AdminReadResult::ScheduledNotices(take!(rows, Notice)),
        Shape::IpBlocks => AdminReadResult::IpBlocks(take!(rows, IpBlock)),
        Shape::StaffAccounts => AdminReadResult::StaffAccounts(take!(rows, Staff)),
        Shape::Grants(limit) => {
            AdminReadResult::Grants(Page::of(take!(rows, Grant), limit, |g| g.grant.id))
        }
        Shape::Removals(limit) => {
            AdminReadResult::Removals(Page::of(take!(rows, Removal), limit, |r| r.removal.id))
        }
        Shape::Totp => AdminReadResult::Totp(take!(rows, Totp).into_iter().next()),
        Shape::Idempotency => {
            AdminReadResult::Idempotency(take!(rows, Idempotency).into_iter().next())
        }
    }
}

/// The TOTP secret of `account` (`None` = TOTP is not enabled). Also usable inside a `PersistHandle::read`
/// closure, like [`crate::queries`].
pub fn totp(conn: &Connection, account: AccountId) -> Result<Option<TotpRecord>, DbError> {
    Ok(conn
        .prepare_cached(
            "SELECT account_id, secret, enabled_at FROM admin_totp WHERE account_id = ?1",
        )?
        .query_row(params![account.0], |r| {
            Ok(TotpRecord {
                account_id: AccountId(r.get(0)?),
                secret: r.get(1)?,
                enabled_at: UnixTime(r.get(2)?),
            })
        })
        .optional()?)
}

impl PersistHandle {
    /// Runs an admin read (chunked, behind lobby work) and returns its result.
    pub async fn admin_read(&self, query: AdminReadQuery) -> Result<AdminReadResult, DbError> {
        let (scan, shape) = plan(&query)?;
        let rows = self.admin_scan(scan)?.collect().await?;
        Ok(assemble(shape, rows))
    }

    /// Blocking variant of [`PersistHandle::admin_read`] for threads outside the async runtime (console, CLI,
    /// tests).
    pub fn admin_read_blocking(&self, query: AdminReadQuery) -> Result<AdminReadResult, DbError> {
        let (scan, shape) = plan(&query)?;
        let rows = self.admin_scan(scan)?.collect_blocking()?;
        Ok(assemble(shape, rows))
    }
}
