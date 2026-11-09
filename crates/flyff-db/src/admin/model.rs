//! Records of the admin tables (migration `0008_admin`, Plan/25 §11.6): shared bookmarks, item templates and
//! kits, moderation notes, scheduled notices, IP blocks, TOTP secrets and the idempotency rows, plus the staff
//! row and the offline-edit payload.
//!
//! All records are `Serialize + Deserialize + Clone + PartialEq`. A record that is about to be inserted carries
//! `id = 0` (SQLite assigns the id); an update addresses its row by `id`.

use crate::error::{DbError, DbErrorKind};
use flyff_common::{AccountId, CharacterId, UnixTime};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Longest moderation note, in characters (Plan/25 §4.8).
pub const NOTE_TEXT_MAX_CHARS: usize = 2000;
/// Longest notice text, in bytes (Plan/25 §8.1).
pub const NOTICE_TEXT_MAX_BYTES: usize = 512;
/// Longest `Idempotency-Key`, in bytes.
pub const IDEMPOTENCY_KEY_MAX_BYTES: usize = 128;
/// Largest stored idempotency result (a grant report for 500 recipients is about 100 KiB).
pub const IDEMPOTENCY_RESULT_MAX_BYTES: usize = 1024 * 1024;
/// Idempotency rows are purged this long after their creation by the daily job (Plan/25 §11.6).
pub const IDEMPOTENCY_RETENTION_S: i64 = 24 * 3600;
/// Largest template or kit specification, in bytes (the admin body limit for imports is 256 KiB).
pub const TEMPLATE_SPEC_MAX_BYTES: usize = 256 * 1024;

pub(crate) fn invalid(what: impl Into<String>) -> DbError {
    DbError::new(DbErrorKind::Invalid, what)
}

/// How the live map picks the layer of a location (`admin_bookmarks.layer_policy`, Plan/25 §3.13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerPolicy {
    /// Always layer 0 (`fixed0`, the default).
    #[default]
    Fixed0,
    /// The admin picks among the live layers (`pick_live`: party instance dungeons, guild houses, ...).
    PickLive,
}

impl LayerPolicy {
    /// The column text.
    pub const fn as_str(self) -> &'static str {
        match self {
            LayerPolicy::Fixed0 => "fixed0",
            LayerPolicy::PickLive => "pick_live",
        }
    }

    /// Parses the column text.
    pub fn parse(text: &str) -> Option<LayerPolicy> {
        match text {
            "fixed0" => Some(LayerPolicy::Fixed0),
            "pick_live" => Some(LayerPolicy::PickLive),
            _ => None,
        }
    }
}

/// One row of `admin_bookmarks`: a shared quick location of the live map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminBookmarkRecord {
    /// `id`; `0` in a record about to be inserted.
    pub id: i64,
    /// `grp`: the group button the location sits under.
    pub group: String,
    /// `name`.
    pub name: String,
    /// `world`: the world id.
    pub world_id: u32,
    /// `layer_policy`.
    pub layer_policy: LayerPolicy,
    /// `x` (world coordinates).
    pub x: f32,
    /// `z` (world coordinates).
    pub z: f32,
    /// `zoom`: the map zoom to apply, `None` = the preset of the group.
    pub zoom: Option<f32>,
    /// `sort`: order inside the group.
    pub sort: i32,
    /// `created_by`: the audit actor (`staff:<account>`, `owner:local`, ...).
    pub created_by: String,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `updated_at`.
    pub updated_at: UnixTime,
}

impl AdminBookmarkRecord {
    /// A new bookmark in layer policy `fixed0`, to be inserted.
    pub fn new(
        group: impl Into<String>,
        name: impl Into<String>,
        world_id: u32,
        x: f32,
        z: f32,
        created_by: impl Into<String>,
        at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            group: group.into(),
            name: name.into(),
            world_id,
            layer_policy: LayerPolicy::Fixed0,
            x,
            z,
            zoom: None,
            sort: 0,
            created_by: created_by.into(),
            created_at: at,
            updated_at: at,
        }
    }
}

/// What an `admin_templates` row holds (`kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateKind {
    /// One item specification (`ItemSpec`), e.g. "+10 Fire Angel Sword".
    Item,
    /// A named basket (`Kit`): item specifications plus penya.
    Kit,
}

impl TemplateKind {
    /// The column text.
    pub const fn as_str(self) -> &'static str {
        match self {
            TemplateKind::Item => "item",
            TemplateKind::Kit => "kit",
        }
    }

    /// Parses the column text.
    pub fn parse(text: &str) -> Option<TemplateKind> {
        match text {
            "item" => Some(TemplateKind::Item),
            "kit" => Some(TemplateKind::Kit),
            _ => None,
        }
    }
}

/// One row of `admin_templates`: an item template or a kit shared between staff (Plan/25 §5.4). `(kind, name)`
/// is unique case-insensitively.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminTemplateRecord {
    /// `id`; `0` in a record about to be inserted.
    pub id: i64,
    /// `kind`.
    pub kind: TemplateKind,
    /// `name`.
    pub name: String,
    /// `spec`: the JSON of an `ItemSpec` or a `Kit` (the DTOs live in `flyff-server`; this crate stores the value
    /// as given).
    pub spec: serde_json::Value,
    /// `created_by`.
    pub created_by: String,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `updated_at`.
    pub updated_at: UnixTime,
}

/// What a moderation note is about (`admin_notes.subject_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteSubjectKind {
    /// An account.
    Account,
    /// A character.
    Character,
}

impl NoteSubjectKind {
    /// The column text.
    pub const fn as_str(self) -> &'static str {
        match self {
            NoteSubjectKind::Account => "account",
            NoteSubjectKind::Character => "character",
        }
    }

    /// Parses the column text.
    pub fn parse(text: &str) -> Option<NoteSubjectKind> {
        match text {
            "account" => Some(NoteSubjectKind::Account),
            "character" => Some(NoteSubjectKind::Character),
            _ => None,
        }
    }
}

/// The subject of a moderation note: an account or a character id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteSubject {
    /// Account or character.
    pub kind: NoteSubjectKind,
    /// The account id or character id.
    pub id: u32,
}

impl NoteSubject {
    /// The note subject "account".
    pub fn account(id: AccountId) -> Self {
        Self {
            kind: NoteSubjectKind::Account,
            id: id.0,
        }
    }

    /// The note subject "character".
    pub fn character(id: CharacterId) -> Self {
        Self {
            kind: NoteSubjectKind::Character,
            id: id.0,
        }
    }
}

/// One row of `admin_notes`: a moderation note (Plan/25 §4.8). Pinned notes show as a banner in the subject's
/// drawer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminNoteRecord {
    /// `id`; `0` in a record about to be inserted.
    pub id: i64,
    /// `subject_kind` and `subject_id`.
    pub subject: NoteSubject,
    /// `text`, at most [`NOTE_TEXT_MAX_CHARS`] characters.
    pub text: String,
    /// `pinned`.
    pub pinned: bool,
    /// `author`: the audit actor.
    pub author: String,
    /// `created_at`.
    pub created_at: UnixTime,
}

impl AdminNoteRecord {
    /// A new, unpinned note, to be inserted.
    pub fn new(
        subject: NoteSubject,
        text: impl Into<String>,
        author: impl Into<String>,
        at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            subject,
            text: text.into(),
            pinned: false,
            author: author.into(),
            created_at: at,
        }
    }
}

/// One row of `scheduled_notices`: a scheduled or recurring notice (Plan/25 §8.1). A notice repeats every
/// `repeat_min` minutes **or** daily at `daily_at` (`HH:MM` local time), never both; neither = a single notice at
/// `start_at`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledNoticeRecord {
    /// `id`; `0` in a record about to be inserted.
    pub id: i64,
    /// `text`, at most [`NOTICE_TEXT_MAX_BYTES`] bytes.
    pub text: String,
    /// `style`: `system` (the `/sys` notice) or `caption`.
    pub style: String,
    /// `target`: `all`, `world:<id>` or the audience text the notice job understands.
    pub target: String,
    /// `start_at`.
    pub start_at: UnixTime,
    /// `repeat_min`: repeat every N minutes.
    pub repeat_min: Option<u32>,
    /// `daily_at`: `HH:MM`.
    pub daily_at: Option<String>,
    /// `end_at`: last moment the notice may fire.
    pub end_at: Option<UnixTime>,
    /// `paused`.
    pub paused: bool,
    /// `created_by`.
    pub created_by: String,
    /// `created_at`.
    pub created_at: UnixTime,
    /// `last_fired_at`, written by `DbOp::NoticeFired` (never by an update).
    pub last_fired_at: Option<UnixTime>,
}

impl ScheduledNoticeRecord {
    /// A single system notice to all players at `start_at`, to be inserted.
    pub fn once(
        text: impl Into<String>,
        start_at: UnixTime,
        created_by: impl Into<String>,
        at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            text: text.into(),
            style: "system".to_owned(),
            target: "all".to_owned(),
            start_at,
            repeat_min: None,
            daily_at: None,
            end_at: None,
            paused: false,
            created_by: created_by.into(),
            created_at: at,
            last_fired_at: None,
        }
    }

    /// Checks the limits of the record (the op refuses a violation with `Invalid`).
    pub fn validate(&self) -> Result<(), DbError> {
        if self.text.is_empty() || self.text.len() > NOTICE_TEXT_MAX_BYTES {
            return Err(invalid(format!(
                "notice text must be 1..={NOTICE_TEXT_MAX_BYTES} bytes, not {}",
                self.text.len()
            )));
        }
        if self.repeat_min == Some(0) {
            return Err(invalid("repeat_min must be at least 1"));
        }
        if self.repeat_min.is_some() && self.daily_at.is_some() {
            return Err(invalid(
                "a notice repeats every N minutes or daily, not both",
            ));
        }
        if let Some(daily) = &self.daily_at
            && !valid_hh_mm(daily)
        {
            return Err(invalid(format!("daily_at '{daily}' is not HH:MM")));
        }
        Ok(())
    }
}

fn valid_hh_mm(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 5 || bytes.get(2) != Some(&b':') {
        return false;
    }
    let digit = |i: usize| {
        bytes
            .get(i)
            .filter(|b| b.is_ascii_digit())
            .map(|b| b - b'0')
    };
    let (Some(h1), Some(h2), Some(m1), Some(m2)) = (digit(0), digit(1), digit(3), digit(4)) else {
        return false;
    };
    h1 * 10 + h2 < 24 && m1 * 10 + m2 < 60
}

/// One row of `admin_ip_blocks`: a live IP block merged into the `net.ip_block` set (Plan/25 §4.8). `cidr` is
/// stored normalised by [`normalize_cidr`], so `1.2.3.4` and `1.2.3.4/32` are the same row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpBlockRecord {
    /// `id`; `0` in a record about to be inserted.
    pub id: i64,
    /// `cidr` (normalised).
    pub cidr: String,
    /// `reason`.
    pub reason: String,
    /// `until`: `None` = permanent.
    pub until: Option<UnixTime>,
    /// `created_by`.
    pub created_by: String,
    /// `created_at`.
    pub created_at: UnixTime,
}

impl IpBlockRecord {
    /// A new block, to be inserted (the CIDR is normalised by the op).
    pub fn new(
        cidr: impl Into<String>,
        reason: impl Into<String>,
        until: Option<UnixTime>,
        created_by: impl Into<String>,
        at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            cidr: cidr.into(),
            reason: reason.into(),
            until,
            created_by: created_by.into(),
            created_at: at,
        }
    }

    /// `true` while the block applies at `now`.
    pub fn is_active(&self, now: UnixTime) -> bool {
        self.until.is_none_or(|u| u > now)
    }
}

/// Normalises an address or a `addr/prefix` CIDR: the host bits are cleared and the prefix is always written
/// (`1.2.3.4` -> `1.2.3.4/32`, `10.1.2.3/8` -> `10.0.0.0/8`, IPv6 in the compressed form `std` prints).
/// `Invalid` for anything that is not an IPv4 or IPv6 address with an optional valid prefix.
pub fn normalize_cidr(text: &str) -> Result<String, DbError> {
    let text = text.trim();
    let (addr_text, prefix_text) = match text.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (text, None),
    };
    let addr: IpAddr = addr_text
        .parse()
        .map_err(|_| invalid(format!("'{text}' is not an IP address or CIDR")))?;
    let bits: u32 = if addr.is_ipv4() { 32 } else { 128 };
    let prefix: u32 = match prefix_text {
        None => bits,
        Some(p) => p
            .parse::<u32>()
            .ok()
            .filter(|p| *p <= bits)
            .ok_or_else(|| invalid(format!("'{text}': the prefix must be 0..={bits}")))?,
    };
    let masked = match addr {
        IpAddr::V4(v4) => {
            let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
            IpAddr::from((u32::from(v4) & mask).to_be_bytes())
        }
        IpAddr::V6(v6) => {
            let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
            IpAddr::from((u128::from(v6) & mask).to_be_bytes())
        }
    };
    Ok(format!("{masked}/{prefix}"))
}

/// One row of `admin_totp` (Plan/25 §1.3): the TOTP secret of a staff account. The secret is never logged: the
/// failed-command dump and the audit writer scrub it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotpRecord {
    /// `account_id`.
    pub account_id: AccountId,
    /// `secret` (raw bytes).
    pub secret: Vec<u8>,
    /// `enabled_at`.
    pub enabled_at: UnixTime,
}

impl std::fmt::Debug for TotpRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TotpRecord")
            .field("account_id", &self.account_id)
            .field("secret", &"<redacted>")
            .field("enabled_at", &self.enabled_at)
            .finish()
    }
}

/// One staff account row (`AdminReadQuery::StaffAccounts`, Plan/25 §9.4): the account columns the Staff & access
/// tab shows, without the password hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaffAccountRecord {
    /// `accounts.id`.
    pub id: AccountId,
    /// `accounts.name`.
    pub name: String,
    /// The stored account-level authority letter (the effective one also depends on `[gm]` grants, which the
    /// server resolves).
    pub authority: u8,
    /// `accounts.created_at`.
    pub created_at: UnixTime,
    /// `accounts.last_login_at`.
    pub last_login_at: Option<UnixTime>,
    /// `accounts.last_login_ip` (only shown with `view.pii`).
    pub last_login_ip: Option<String>,
    /// `accounts.must_change_password`.
    pub must_change_password: bool,
    /// `admin_totp.enabled_at`, `None` = no TOTP.
    pub totp_enabled_at: Option<UnixTime>,
}

/// State of an idempotency row (`admin_idempotency.state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyState {
    /// The request is being executed.
    InFlight,
    /// The request finished; `result` holds the stored response.
    Done,
    /// The request failed before it changed anything: the same key may run again.
    Failed,
}

impl IdempotencyState {
    /// The column text.
    pub const fn as_str(self) -> &'static str {
        match self {
            IdempotencyState::InFlight => "in_flight",
            IdempotencyState::Done => "done",
            IdempotencyState::Failed => "failed",
        }
    }

    /// Parses the column text.
    pub fn parse(text: &str) -> Option<IdempotencyState> {
        match text {
            "in_flight" => Some(IdempotencyState::InFlight),
            "done" => Some(IdempotencyState::Done),
            "failed" => Some(IdempotencyState::Failed),
            _ => None,
        }
    }
}

/// One row of `admin_idempotency`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    /// `key`: the `Idempotency-Key` header.
    pub key: String,
    /// `actor`: the audit actor of the request.
    pub actor: String,
    /// `route`: method and path.
    pub route: String,
    /// `body_sha256`.
    pub body_sha256: [u8; 32],
    /// `state`.
    pub state: IdempotencyState,
    /// `result`: the stored response body of a finished request.
    pub result: Option<String>,
    /// `created_at`.
    pub created_at: UnixTime,
}

/// The answer to `DbOp::IdempotencyBegin` (Plan/25 §11.6): what the route does with the request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempotencyDecision {
    /// A new row was written (or a `failed` one re-armed): dispatch the request, then `IdempotencyFinish`.
    Started,
    /// The same request is still running: `409 in_flight`.
    InFlight {
        /// When the running request started.
        since: UnixTime,
    },
    /// The same request already finished: answer with the stored result.
    Done {
        /// The stored response body.
        result: Option<String>,
    },
    /// The key belongs to another body, route or actor: `422 idempotency_mismatch`.
    Mismatch,
}

/// A change to an offline character that goes through the game thread under an online-registry hold and is
/// persisted as one `AdminOfflineEdit` batch (Plan/25 §4.6, [`crate::AtomicBatch::admin_offline_edit`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AdminOfflineEdit {
    /// `SetCharacterPosition`: unstuck / return to town to `pos` in `world_id` (layer 0).
    SetPosition {
        /// The character.
        character_id: CharacterId,
        /// Target world.
        world_id: u32,
        /// Target position.
        pos: [f32; 3],
    },
    /// `RenameCharacter`.
    Rename {
        /// The character.
        character_id: CharacterId,
        /// The new name (unique among live characters).
        name: String,
    },
    /// `UndeleteCharacter`.
    Undelete {
        /// The character.
        character_id: CharacterId,
        /// The slot it returns to.
        slot: u8,
    },
}
