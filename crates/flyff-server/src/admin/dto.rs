//! The DTO catalogue of the admin API (Plan/25 §11.10, WP-ADMIN-02): the request and response bodies that cross
//! `/admin/api/*`, plus the error envelope (§11.3) and the session bodies (§1.3).
//!
//! Every type derives serde; with the cargo feature `ts` it also derives `ts_rs::TS`, and `typescript` (the feature-gated renderer) renders the
//! whole catalogue as `web/src/admin/gen/dto.ts` (`cargo xtask admin-dto`, part of `cargo xtask gen`). The wire
//! conventions:
//!
//! * enums are `snake_case`; unit variants are plain strings, data variants are externally tagged
//!   (`{"pocket": 2}`, `{"bank": {"slot": 0}}`);
//! * `u64`/`i64` are JSON numbers (TypeScript `number`; the values that need 64 bits are far below 2^53);
//! * ids are plain `u32` (the aliases below keep the plan's names in Rust);
//! * request types accept a missing or `null` optional field; response types always write the key (`null`).
//!
//! Later work packages add their own DTOs here (one block each) and list them in `typescript`.

use serde::{Deserialize, Serialize};

/// Character id (`CharacterId`, a `u32` on the wire).
pub type CharacterId = u32;
/// Guild id.
pub type GuildId = u32;
/// Party id.
pub type PartyId = u32;
/// World id.
pub type WorldId = u32;

// ---- errors (25 §11.3) -------------------------------------------------------------------------------------------

/// The `code` of an error body (§11.3, §11.10 `ErrorCode`). The payloads of `NeedsFeature{wp}` and
/// `Invalid{field}` travel in [`ErrorDetails`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The identity lacks the capability (403).
    Forbidden,
    /// The target's authority is not below the actor's letter (403).
    TargetOutranks,
    /// A reason is mandatory here (400).
    ReasonRequired,
    /// The typed confirmation is missing or wrong (400).
    ConfirmRequired,
    /// The target changed since the client looked (409).
    StaleTarget,
    /// The character is online / offline where the action needs the other (409).
    CharacterOnline,
    /// A group action over `admin.max_objects_per_action` targets (413).
    TooManyTargets,
    /// The idempotency key was used with another request (422).
    IdempotencyMismatch,
    /// The idempotency key is still in flight (409).
    InFlight,
    /// A pending grant is being delivered right now (409).
    DeliveryInProgress,
    /// The pending grant was delivered already (409).
    AlreadyDelivered,
    /// The bag of the recipient has no room for the basket (409).
    NoSpace,
    /// The mailbox of the recipient would overflow (409).
    MailboxFull,
    /// The audit record could not be acknowledged (503).
    AuditUnavailable,
    /// The staff password must be changed first (428).
    PasswordChangeRequired,
    /// Not written yet (501).
    NotImplemented,
    /// The backend belongs to a work package that has not landed (501); `details.wp` names it.
    NeedsFeature,
    /// A field is invalid (400); `details.field` names it.
    Invalid,
    /// No such record (404).
    NotFound,
    /// A per-request limit (500 recipients, 50 basket lines, ...) was exceeded (413).
    Limit,
    /// Too many mutations or streams (429).
    RateLimited,
    /// No session, an expired session or a revoked one (401).
    Unauthorized,
    /// Wrong account, password, token or TOTP code, a banned account, or an authority below L (401). Always the same
    /// answer.
    InvalidCredentials,
    /// The body is not the JSON the route expects (400).
    BadRequest,
    /// The game thread or the database did not answer (503).
    Unavailable,
    /// The route exists, the method does not (405).
    MethodNotAllowed,
    /// A bug or an unexpected failure (500).
    Internal,
}

impl ErrorCode {
    /// The HTTP status of the code (25 §11.3).
    pub const fn status(self) -> u16 {
        match self {
            ErrorCode::Forbidden | ErrorCode::TargetOutranks => 403,
            ErrorCode::ReasonRequired
            | ErrorCode::ConfirmRequired
            | ErrorCode::Invalid
            | ErrorCode::BadRequest => 400,
            ErrorCode::StaleTarget
            | ErrorCode::CharacterOnline
            | ErrorCode::InFlight
            | ErrorCode::DeliveryInProgress
            | ErrorCode::AlreadyDelivered
            | ErrorCode::NoSpace
            | ErrorCode::MailboxFull => 409,
            ErrorCode::TooManyTargets | ErrorCode::Limit => 413,
            ErrorCode::IdempotencyMismatch => 422,
            ErrorCode::AuditUnavailable | ErrorCode::Unavailable => 503,
            ErrorCode::PasswordChangeRequired => 428,
            ErrorCode::NotImplemented | ErrorCode::NeedsFeature => 501,
            ErrorCode::NotFound => 404,
            ErrorCode::RateLimited => 429,
            ErrorCode::Unauthorized | ErrorCode::InvalidCredentials => 401,
            ErrorCode::MethodNotAllowed => 405,
            ErrorCode::Internal => 500,
        }
    }

    /// The wire name (`target_outranks`).
    pub const fn name(self) -> &'static str {
        match self {
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::TargetOutranks => "target_outranks",
            ErrorCode::ReasonRequired => "reason_required",
            ErrorCode::ConfirmRequired => "confirm_required",
            ErrorCode::StaleTarget => "stale_target",
            ErrorCode::CharacterOnline => "character_online",
            ErrorCode::TooManyTargets => "too_many_targets",
            ErrorCode::IdempotencyMismatch => "idempotency_mismatch",
            ErrorCode::InFlight => "in_flight",
            ErrorCode::DeliveryInProgress => "delivery_in_progress",
            ErrorCode::AlreadyDelivered => "already_delivered",
            ErrorCode::NoSpace => "no_space",
            ErrorCode::MailboxFull => "mailbox_full",
            ErrorCode::AuditUnavailable => "audit_unavailable",
            ErrorCode::PasswordChangeRequired => "password_change_required",
            ErrorCode::NotImplemented => "not_implemented",
            ErrorCode::NeedsFeature => "needs_feature",
            ErrorCode::Invalid => "invalid",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Limit => "limit",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::InvalidCredentials => "invalid_credentials",
            ErrorCode::BadRequest => "bad_request",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::MethodNotAllowed => "method_not_allowed",
            ErrorCode::Internal => "internal",
        }
    }
}

/// The `details` of an error body.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ErrorDetails {
    /// `needs_feature`: the work package that lands the backend (`WP-ADMIN-20`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub wp: Option<String>,
    /// `invalid`: the offending field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub field: Option<String>,
    /// `forbidden`: the capability that was missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub cap: Option<String>,
    /// `rate_limited`, `in_flight`: seconds until a retry makes sense.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub retry_after_s: Option<u32>,
    /// `stale_target`: what the object is now (the `ObjectDetail` of `GET /admin/api/objects/{obj}`); absent when it
    /// is gone. Boxed: `ApiError` is returned everywhere and must stay small (`clippy::result_large_err`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable, type = "unknown"))]
    pub current: Option<Box<serde_json::Value>>,
}

/// The `error` object of an error response (`{error: {code, message, details?}}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ErrorBody {
    /// Machine-readable code.
    pub code: ErrorCode,
    /// Human-readable message (English).
    pub message: String,
    /// More context, when the code has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub details: Option<ErrorDetails>,
}

/// The whole error response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ErrorEnvelope {
    /// The error.
    pub error: ErrorBody,
}

// ---- envelopes (25 §11.3, §11.10) ---------------------------------------------------------------------------------

/// A page of a list route (`?cursor=&limit=` -> `{items, next_cursor}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Page<T> {
    /// The rows.
    pub items: Vec<T>,
    /// The cursor of the next page (opaque), `None` on the last.
    pub next_cursor: Option<String>,
}

/// The mutation envelope: the route's body plus the reason and the typed confirmation (§2.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Mutation<T> {
    /// The route's own body.
    pub body: T,
    /// Why (mandatory where §2.3 says so).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub reason: Option<Reason>,
    /// The typed confirmation: the target name, or `CONFIRM` for multi-target actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub confirm: Option<String>,
}

/// A reason: a preset (`[admin.reason_presets]`), free text, or both. At least one is required where §2.3 says so.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Reason {
    /// One of the configured presets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub preset: Option<String>,
    /// Free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub text: Option<String>,
}

// ---- sessions (25 §1.3) -------------------------------------------------------------------------------------------

/// `POST /admin/api/session`: a staff login or the owner-token exchange.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum LoginRequest {
    /// `{account, pwhash, totp?}`: `pwhash` is the game's legacy hash (12 L1), never the password.
    Staff {
        /// The account name.
        account: String,
        /// `md5_hex_lower(salt + password)`.
        pwhash: String,
        /// The TOTP code, when the account has one enrolled (WP-ADMIN-28).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional = nullable))]
        totp: Option<String>,
    },
    /// `{token}`: the content of `data/admin.token`.
    Token {
        /// The owner token.
        token: String,
    },
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginRequest::Staff { account, .. } => f
                .debug_struct("LoginRequest::Staff")
                .field("account", account)
                .finish_non_exhaustive(),
            LoginRequest::Token { .. } => f.write_str("LoginRequest::Token(..)"),
        }
    }
}

/// `PUT /admin/api/session/password`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PasswordChange {
    /// The current password's hash.
    pub old_pwhash: String,
    /// The new password's hash.
    pub new_pwhash: String,
}

impl std::fmt::Debug for PasswordChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordChange(..)")
    }
}

/// Which kind of identity a session is (§1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IdentityKind {
    /// A game account with an authority of `L` or higher (`staff:<account>`).
    Staff,
    /// The owner through the token (`owner:token`).
    OwnerToken,
    /// The implicit owner of a loopback request (`owner:local`).
    OwnerLocal,
}

/// The in-world character of the staff account that "Summon to me" and "Go to" move (§1.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ActingCharacter {
    /// Character id.
    pub id: CharacterId,
    /// Character name.
    pub name: String,
}

/// `GET /admin/api/session` (also the answer of a login): who the caller is and what it may do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionInfo {
    /// The audit actor: `staff:<account>`, `owner:token` or `owner:local`.
    pub identity: String,
    /// The kind of identity.
    pub kind: IdentityKind,
    /// The staff account name (`None` for the owners).
    pub account: Option<String>,
    /// The authority letter (`P` for the owners).
    pub authority: String,
    /// The capabilities, as wire names (`view.map`, ...), with the `[admin.capabilities]` overrides applied.
    pub caps: Vec<String>,
    /// The acting GM character of a staff member who is in the world.
    pub acting_character: Option<ActingCharacter>,
    /// Every route but the session routes answers `428` until the password is changed.
    pub must_change_password: bool,
    /// The request did not come from the host itself.
    pub remote: bool,
    /// The request came over TLS.
    pub tls: bool,
    /// The account has a TOTP secret enrolled.
    pub totp_enabled: bool,
    /// `admin.totp_required`: remote staff sign-ins need a code.
    pub totp_required: bool,
    /// Unix seconds when the session expires if it stays idle (`None` without a cookie session).
    pub idle_expires_at: Option<i64>,
    /// Unix seconds of the absolute end of the session (`None` without a cookie session).
    pub expires_at: Option<i64>,
}

/// One admin session as the staff overview lists it (`GET /admin/api/admin-sessions`, WP-ADMIN-17).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AdminSessionRow {
    /// The public handle (not the cookie): what `DELETE /admin/api/admin-sessions/{id}` takes.
    pub id: String,
    /// The audit actor.
    pub identity: String,
    /// The kind of identity.
    pub kind: IdentityKind,
    /// The authority letter.
    pub authority: String,
    /// The client address (`None` without `view.pii`).
    pub ip: Option<String>,
    /// The session did not come from the host itself.
    pub remote: bool,
    /// Unix seconds of the login.
    pub created_at: i64,
    /// Unix seconds of the last request.
    pub last_seen_at: i64,
}

// ---- items, grants, guilds, character edits (25 §11.10) -----------------------------------------------------------

/// Where an item lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ContainerLoc {
    /// The equipment slots.
    Equipment,
    /// The inventory.
    Inventory,
    /// A pocket (`0..`).
    Pocket(u8),
    /// A bank of the account (account-scoped; `slot` is the character slot `0..=2`, `3` the comm bank).
    Bank {
        /// The bank slot.
        slot: u8,
    },
    /// The guild bank.
    GuildBank(GuildId),
    /// The mailbox.
    Mailbox,
}

/// How long an item lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum KeepUntil {
    /// The item's own lifetime from the game data.
    Item,
    /// No expiry.
    None,
    /// Expires after a duration.
    Duration {
        /// Seconds.
        secs: u64,
    },
    /// Expires at a time.
    At {
        /// Unix seconds.
        unix: i64,
    },
}

/// One random option line (awakening / blessing): `dst` 1..=127, `adj` -511..=511 (19 §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RandomOptLine {
    /// The destination parameter.
    pub dst: u8,
    /// The adjustment.
    pub adj: i16,
}

/// The element of an upgraded item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Element {
    /// Fire.
    Fire,
    /// Water.
    Water,
    /// Electricity.
    Electricity,
    /// Wind.
    Wind,
    /// Earth.
    Earth,
}

/// One vis of a vis-capable pet with its keep-until time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct VisSpec {
    /// The vis item id.
    pub item_id: u32,
    /// How long it lives.
    pub keep_until: KeepUntil,
}

/// System pet data of an egg or a pet item (13 §6.2 `PetRecord`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PetRecord {
    /// Kind `0..=3`.
    pub kind: u8,
    /// `0` egg ... `5` S.
    pub level: u8,
    /// Experience.
    pub exp: u32,
    /// Energy.
    pub energy: u16,
    /// Life `<= 99`.
    pub life: u16,
    /// The stat-gain roll per level D..S (`0` = not rolled).
    pub avail_levels: [u8; 6],
    /// Name, at most 16 bytes; empty = unnamed.
    pub name: String,
}

/// One item as the builder describes it (§5.2); the server validates and normalises it into a stored item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemSpec {
    /// The item id.
    pub item_id: u32,
    /// The stack count.
    pub count: u16,
    /// The upgrade level.
    pub upgrade: u8,
    /// The element, when the item has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub element: Option<Element>,
    /// The element level `0..=20`.
    pub element_level: u8,
    /// Up to 3 random option lines.
    #[serde(default)]
    pub random_options: Vec<RandomOptLine>,
    /// The safe flag of the random options.
    #[serde(default)]
    pub safe: bool,
    /// Piercing cards.
    #[serde(default)]
    pub piercing: Vec<u32>,
    /// Ultimate gems.
    #[serde(default)]
    pub ultimate_gems: Vec<u32>,
    /// Vis of a vis-capable pet.
    #[serde(default)]
    pub vis: Vec<VisSpec>,
    /// Pet data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub pet: Option<PetRecord>,
    /// The charged flag.
    #[serde(default)]
    pub charged: bool,
    /// The expiry.
    pub keep_until: KeepUntil,
    /// The binds flag (`None` = the item default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub bound: Option<bool>,
    /// The item text, at most 31 bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub text: Option<String>,
    /// The guild of a guild cloak.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub guild_id: Option<GuildId>,
}

/// A named basket: item specs plus penya.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Kit {
    /// The kit name.
    pub name: String,
    /// The items.
    pub items: Vec<ItemSpec>,
    /// The penya.
    pub gold: u64,
}

/// Who receives a grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Recipients {
    /// These characters.
    Characters(Vec<CharacterId>),
    /// Every online player.
    AllOnline {
        /// Staff members are included only when asked for.
        include_staff: bool,
    },
    /// Every online player of a world.
    World(WorldId),
    /// The members of a party.
    Party(PartyId),
    /// The members of a guild.
    Guild(GuildId),
}

/// How a grant reaches its recipients (§5.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    /// Direct when online with space, else mail or pending.
    Auto,
    /// Into the inventory now (online only).
    Direct,
    /// A pending grant, delivered at the next enter-world and by the poll.
    Pending,
    /// A system mail with the basket attached.
    Mail {
        /// The subject.
        subject: String,
        /// The text.
        text: String,
    },
}

/// `POST /admin/api/grants/v2` (inside a [`Mutation`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GrantRequest {
    /// Who receives it (at most 500 characters).
    pub recipients: Recipients,
    /// The basket (at most 50 lines).
    pub items: Vec<ItemSpec>,
    /// Penya on top.
    pub gold: u64,
    /// How it is delivered.
    pub mode: DeliveryMode,
}

/// What happened to one recipient.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GrantOutcome {
    /// Into the inventory.
    Delivered {
        /// The serials of the created items, one per item line.
        serials: Vec<u64>,
        /// The penya added to the wallet.
        gold: u64,
    },
    /// As system mails (one per item line; the penya rides on the first).
    Mailed {
        /// The mail ids.
        mail_ids: Vec<u64>,
    },
    /// As pending grants (one row per item line and one for the penya).
    Pending {
        /// The grant ids.
        grant_ids: Vec<i64>,
    },
    /// Not at all.
    Failed {
        /// Why.
        code: ErrorCode,
        /// For the operator.
        message: String,
    },
}

/// The per-recipient report of a grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GrantReport {
    /// The request id (a UUID text).
    pub request_id: String,
    /// One row per recipient.
    pub results: Vec<(CharacterId, GrantOutcome)>,
    /// The recipients' names, in the order of `results`.
    pub names: Vec<String>,
}

/// A guild member's rank.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GuildRank {
    /// The guild master.
    Master,
    /// Kingpin.
    Kingpin,
    /// Captain.
    Captain,
    /// Supporter.
    Supporter,
    /// Ordinary member.
    Member,
}

/// `POST /admin/api/guilds` (inside a [`Mutation`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildSpec {
    /// The guild name.
    pub name: String,
    /// The master.
    pub master: CharacterId,
    /// More members with their ranks.
    pub members: Vec<(CharacterId, GuildRank)>,
    /// Guild level.
    pub level: u8,
    /// Guild penya.
    pub gold: u64,
    /// Guild points.
    pub pxp: u32,
    /// Logo `0..=27`.
    pub logo: u8,
    /// The notice.
    pub notice: String,
    /// Give the master the guild cloak.
    pub give_cloak: bool,
    /// Ignore the re-creation cooldown.
    pub override_cooldown: bool,
}

/// One guild edit (`PATCH /admin/api/guilds/{id}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GuildEdit {
    /// Rename.
    Rename(String),
    /// Change the logo.
    Logo(u8),
    /// Hand the guild to another member.
    Master(CharacterId),
    /// Set the level.
    Level(u8),
    /// Set the points.
    Pxp(u32),
    /// Change the guild penya by a delta.
    Gold(i64),
    /// Set the notice.
    Notice(String),
    /// Add a member.
    AddMember(CharacterId, GuildRank),
    /// Remove a member.
    RemoveMember(CharacterId),
    /// Change a rank.
    Rank(CharacterId, GuildRank),
    /// Take an item out of the guild bank.
    BankRemove {
        /// The bank slot.
        slot: u8,
        /// The item serial (a stale serial is refused).
        serial: u64,
        /// How many.
        count: u16,
    },
}

/// A quest operation (Phase C).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QuestOp {
    /// Start the quest.
    Start,
    /// Complete the quest.
    Complete,
    /// Remove the quest.
    Remove,
}

/// An edit of an online character (`POST /admin/api/actions/edit-character`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CharacterEdit {
    /// Set the level (and the job).
    Level {
        /// The level.
        level: u16,
        /// The job, when it changes too.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional = nullable))]
        job: Option<u8>,
    },
    /// Set the job.
    Job(u8),
    /// Set the base stats.
    Stats {
        /// Strength.
        str: u16,
        /// Stamina.
        sta: u16,
        /// Dexterity.
        dex: u16,
        /// Intelligence.
        int: u16,
    },
    /// Give the stat points back.
    Restat,
    /// Add (or remove) skill points.
    SkillPoints(i32),
    /// Reset the skills.
    ResetSkills,
    /// Rename.
    Rename(String),
    /// A quest operation.
    Quest {
        /// What to do.
        op: QuestOp,
        /// The quest id.
        quest: u32,
    },
}

/// An edit of an offline character (`POST /admin/api/characters/{id}/...`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OfflineEdit {
    /// Put the character somewhere.
    SetPosition {
        /// The world.
        world: WorldId,
        /// `[x, y, z]`.
        pos: [f32; 3],
    },
    /// Rename.
    Rename(String),
    /// Restore a deleted character into a slot.
    Undelete {
        /// The character slot `0..=2`.
        slot: u8,
    },
}

// ---- the TypeScript file ------------------------------------------------------------------------------------------

/// The text of `web/src/admin/gen/dto.ts`: the declarations of the whole catalogue (feature `ts`).
#[cfg(feature = "ts")]
pub fn typescript() -> String {
    use ts_rs::{Config, TS};

    fn push<T: TS + 'static>(out: &mut String, cfg: &Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }

    let cfg = Config::new().with_large_int("number");
    let mut out = String::from(
        "// @generated by `cargo xtask admin-dto` from crates/flyff-server/src/admin/dto.rs (feature `ts`). Do not edit this file manually.\n\
         // The DTO catalogue of the admin API (Plan/25 §11.10): bodies, the error envelope and the session bodies.\n\n",
    );
    push::<ErrorCode>(&mut out, &cfg);
    push::<ErrorDetails>(&mut out, &cfg);
    push::<ErrorBody>(&mut out, &cfg);
    push::<ErrorEnvelope>(&mut out, &cfg);
    push::<Page<()>>(&mut out, &cfg);
    push::<Mutation<()>>(&mut out, &cfg);
    push::<Reason>(&mut out, &cfg);
    push::<LoginRequest>(&mut out, &cfg);
    push::<PasswordChange>(&mut out, &cfg);
    push::<IdentityKind>(&mut out, &cfg);
    push::<ActingCharacter>(&mut out, &cfg);
    push::<SessionInfo>(&mut out, &cfg);
    push::<AdminSessionRow>(&mut out, &cfg);
    push::<ContainerLoc>(&mut out, &cfg);
    push::<KeepUntil>(&mut out, &cfg);
    push::<RandomOptLine>(&mut out, &cfg);
    push::<Element>(&mut out, &cfg);
    push::<VisSpec>(&mut out, &cfg);
    push::<PetRecord>(&mut out, &cfg);
    push::<ItemSpec>(&mut out, &cfg);
    push::<Kit>(&mut out, &cfg);
    push::<Recipients>(&mut out, &cfg);
    push::<DeliveryMode>(&mut out, &cfg);
    push::<GrantRequest>(&mut out, &cfg);
    push::<GrantOutcome>(&mut out, &cfg);
    push::<GrantReport>(&mut out, &cfg);
    push::<GuildRank>(&mut out, &cfg);
    push::<GuildSpec>(&mut out, &cfg);
    push::<GuildEdit>(&mut out, &cfg);
    push::<QuestOp>(&mut out, &cfg);
    push::<CharacterEdit>(&mut out, &cfg);
    push::<OfflineEdit>(&mut out, &cfg);
    super::dto_items::declare(&mut out, &cfg);
    super::dto_guilds::declare(&mut out, &cfg);
    super::api::actions::dto::push_all(&mut out, &cfg);
    super::api::comm::dto::declare(&mut out, &cfg);
    super::api::content::dto::declare(&mut out, &cfg);
    super::dto_players::declare(&mut out, &cfg);
    super::api::world::meta::declare_ts(&mut out, &cfg);
    super::api::world::find::declare_ts(&mut out, &cfg);
    super::api::locations::declare_ts(&mut out, &cfg);
    super::api::npc_script::declare_ts(&mut out, &cfg);
    super::api::totp::declare_ts(&mut out, &cfg);
    out
}
