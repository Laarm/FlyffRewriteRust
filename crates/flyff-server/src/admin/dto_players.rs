//! The DTOs of the players area (Plan/25 §4, §9.4, §11.3; WP-ADMIN-16, WP-ADMIN-17): character and account views,
//! container snapshots, mail, moderation lists, IP blocks, the staff overview, the palette search and the request
//! bodies of the character edits. They live in their own module so that the shared `dto.rs` stays small; the
//! generated `web/src/admin/gen/dto.ts` lists them after the catalogue of `dto.rs` (`declare`).
//!
//! Conventions as in [`super::dto`]: enums are `snake_case`, 64-bit values are plain JSON numbers, ids are `u32`,
//! response types always write every key (`null` when empty), request types accept a missing optional field.

use super::dto::{AdminSessionRow, CharacterId, ContainerLoc, WorldId};
use serde::{Deserialize, Serialize};

// ---- characters ---------------------------------------------------------------------------------------------------

/// The lifecycle of a character (Plan/13 §14.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CharacterLife {
    /// Playable.
    Active,
    /// Soft-deleted: restorable until the purge.
    Deleted,
    /// Sealed by an item.
    Sealed,
}

/// A character in a list or a search result (`GET /admin/api/characters`, the account drawer).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CharacterListItem {
    /// `characters.id`.
    pub id: CharacterId,
    /// The name.
    pub name: String,
    /// The owning account.
    pub account_id: u32,
    /// Its name.
    pub account: String,
    /// The slot `0..=2`.
    pub slot: u8,
    /// Active, deleted or sealed.
    pub status: CharacterLife,
    /// In the world right now.
    pub online: bool,
    /// Level (live when online, else the last save).
    pub level: u32,
    /// Job id.
    pub job: u32,
    /// World id.
    pub world: u32,
    /// The character's own authority letter.
    pub authority: String,
    /// Unix seconds of the creation.
    pub created_at: i64,
    /// Unix seconds of the last logout or save; `0` = never.
    pub last_seen: i64,
    /// Unix seconds of the soft delete.
    pub deleted_at: Option<i64>,
    /// Days left until the purge job removes a deleted character.
    pub purge_in_days: Option<i64>,
    /// Unix seconds the login block ends, while one is in force.
    pub login_blocked_until: Option<i64>,
}

/// One skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SkillEntry {
    /// Skill id.
    pub skill: u32,
    /// Level.
    pub level: u32,
}

/// One buff. `buff` is the legacy key `(kind << 16) | id` (kind 0 item, 1 skill, 2 pet, 3 absolute-expiry item,
/// 4 equipment).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BuffEntry {
    /// The buff key.
    pub buff: u32,
    /// Skill level (see the kind).
    pub level: u32,
    /// Milliseconds left (an offline character: the remainder as saved).
    pub remaining_ms: u32,
}

/// One quest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QuestEntry {
    /// Quest id.
    pub quest: u32,
    /// `0..=13` active, `14` finished.
    pub state: u32,
}

/// The state of a character (the overview tab): live for an online character, the saved state for an offline one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CharacterDetailView {
    /// The character.
    pub character_id: CharacterId,
    /// The name.
    pub name: String,
    /// `true`: the numbers are live.
    pub online: bool,
    /// Level.
    pub level: u32,
    /// Job id.
    pub job: u32,
    /// 0 male, 1 female: the requirement lines of an item tooltip are evaluated for this sex (Plan/25 §4.4).
    pub sex: u8,
    /// EXP inside the level (saved value; the client divides by the table of its rules).
    pub exp: i64,
    /// EXP in permille of the level (live only).
    pub exp_permille: Option<u32>,
    /// Strength.
    pub str: u32,
    /// Stamina.
    pub sta: u32,
    /// Dexterity.
    pub dex: u32,
    /// Intelligence.
    pub int: u32,
    /// Free stat points.
    pub stat_points: u32,
    /// Free skill points.
    pub skill_points: u32,
    /// Hit points.
    pub hp: u32,
    /// Maximum hit points (live only).
    pub hp_max: Option<u32>,
    /// Mana points.
    pub mp: u32,
    /// Maximum mana points (live only).
    pub mp_max: Option<u32>,
    /// Fatigue points.
    pub fp: u32,
    /// Maximum fatigue points (live only).
    pub fp_max: Option<u32>,
    /// Penya in the wallet.
    pub gold: i64,
    /// The skills.
    pub skills: Vec<SkillEntry>,
    /// The buffs.
    pub buffs: Vec<BuffEntry>,
    /// The quests (active and finished).
    pub quests: Vec<QuestEntry>,
    /// `m_dwMode`.
    pub mode_flags: u32,
    /// Unix seconds the mute ends; `0` = not muted (and for an offline character: a saved mute counts down only
    /// while the character is online, see `mute_remaining_s`).
    pub mute_until: i64,
    /// A saved mute's remaining seconds (offline characters).
    pub mute_remaining_s: u32,
    /// World id.
    pub world: u32,
    /// Layer id.
    pub layer: i32,
    /// Position x.
    pub x: f32,
    /// Position y.
    pub y: f32,
    /// Position z.
    pub z: f32,
    /// Guild id, `0` = none.
    pub guild_id: u32,
    /// Party id, `0` = none (live only).
    pub party_id: u32,
    /// Accumulated play time in seconds (saved value).
    pub play_time_s: i64,
    /// Unix seconds of the last logout; `0` = never.
    pub last_logout_at: i64,
    /// Unix seconds of the last save the saved values come from.
    pub saved_at: Option<i64>,
}

/// A moderation note (Plan/25 §4.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NoteView {
    /// `admin_notes.id`.
    pub id: i64,
    /// `account` or `character`.
    pub subject_kind: String,
    /// The subject's id.
    pub subject_id: u32,
    /// The text (at most 2,000 characters).
    pub text: String,
    /// Pinned notes show as a banner in the subject's drawer.
    pub pinned: bool,
    /// The author (`staff:<account>`, `owner:token`, ...).
    pub author: String,
    /// Unix seconds.
    pub created_at: i64,
}

/// `GET /admin/api/characters/{id}`: the drawer's data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CharacterProfile {
    /// The list row.
    pub character: CharacterListItem,
    /// The overview.
    pub detail: CharacterDetailView,
    /// The notes of the character and of its account, pinned first.
    pub notes: Vec<NoteView>,
    /// Open pending grants of the character.
    pub pending_grants: u32,
    /// Open pending removals of the character.
    pub pending_removals: u32,
    /// The account's online character through which the banks are live, if any.
    pub live_via: Option<CharacterId>,
}

// ---- containers and mail -------------------------------------------------------------------------------------------

/// One cell of a container: the fields the grid and the filters need. The tooltip comes from `pb`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemCell {
    /// The display slot (the equipment part for the equipment).
    pub slot: u32,
    /// The item id.
    pub item_id: u32,
    /// The serial (`RemoveItem` needs it).
    pub serial: u64,
    /// The stack size.
    pub count: i32,
    /// "+N".
    pub upgrade: i32,
    /// The element byte.
    pub element: u32,
    /// The element level.
    pub element_level: i32,
    /// The item flag byte (bound, expired, ...).
    pub flag: u32,
    /// Unix seconds the item expires; `0` = never.
    pub keep_time_at: i64,
}

/// A container snapshot as the REST routes answer it. `pb` is the exact frame the WebSocket sends for the same
/// container: an `AdminServerMsg` carrying the `ContainerSnapshot` (`sub_id` 0), standard base64. The console decodes
/// it with the code of the socket and feeds the tooltip composer from its items; `items` is the summary for tables
/// and filters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ContainerView {
    /// `equipment`, `inventory`, `pocket`, `bank`.
    pub kind: String,
    /// Pocket / bank slot.
    pub index: u32,
    /// The character the container belongs to.
    pub owner_id: CharacterId,
    /// Its name.
    pub owner_name: String,
    /// Revision (0 for a one-shot read).
    pub rev: u32,
    /// The grid size.
    pub capacity: u32,
    /// Penya (inventory, bank).
    pub gold: u64,
    /// `true`: live from the game; `false`: the saved state.
    pub live: bool,
    /// Unix seconds of the save a saved container shows.
    pub saved_at: i64,
    /// The online character through which an account-scoped bank is live.
    pub live_via: Option<CharacterId>,
    /// The name of that character.
    pub live_via_name: Option<String>,
    /// The cells.
    pub items: Vec<ItemCell>,
    /// The `AdminServerMsg` frame with the `ContainerSnapshot`, base64.
    pub pb: String,
}

/// `GET /admin/api/characters/{id}/items`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ContainersView {
    /// The requested containers (all of them without `?container=`).
    pub containers: Vec<ContainerView>,
}

/// One mail of a mailbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MailRow {
    /// `mails.id`.
    pub id: u32,
    /// The sender's character id (`0` = the system).
    pub sender_id: u32,
    /// The sender's name.
    pub sender_name: String,
    /// The title.
    pub title: String,
    /// The text.
    pub text: String,
    /// The attached penya.
    pub gold: i64,
    /// The attached item.
    pub item: Option<ItemCell>,
    /// The attached item as a `flyff.v1.ItemElem` (base64), for the tooltip.
    pub item_pb: Option<String>,
    /// Unix seconds the mail was sent.
    pub created_at: i64,
    /// Unix seconds the mail is deleted.
    pub expires_at: i64,
    /// Read by the receiver.
    pub read: bool,
}

/// `GET /admin/api/characters/{id}/mail`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MailboxView {
    /// The mails, oldest first.
    pub mails: Vec<MailRow>,
    /// The mailbox size limit (`MAX_MAIL`, 50).
    pub limit: u32,
}

// ---- requests of the character actions ---------------------------------------------------------------------------

/// `POST /admin/api/characters/{id}/items/remove`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RemoveItemBody {
    /// Which container.
    pub container: ContainerLoc,
    /// The display slot of the cell (`ItemCell.slot`).
    pub slot: u32,
    /// The serial the console saw there; a mismatch is `409 stale_target`.
    pub serial: u64,
    /// How many pieces.
    pub count: u32,
}

/// `POST /admin/api/characters/{id}/gold`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GoldBody {
    /// The wallet (`inventory`) or a bank.
    pub container: ContainerLoc,
    /// The change; negative removes.
    pub delta: i64,
}

/// Where `POST /admin/api/characters/{id}/position` sends the character.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PositionTo {
    /// The revival point of the continent (or world) the character is in: "return to town" / unstuck.
    Revival,
    /// A point (online characters only; needs `act.teleport.others`).
    Point {
        /// World.
        world: WorldId,
        /// Layer; `0` for the main layer.
        #[serde(default)]
        layer: i32,
        /// `[x, y, z]` in metres.
        pos: [f32; 3],
    },
}

/// `POST /admin/api/characters/{id}/position`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PositionBody {
    /// The destination.
    pub to: PositionTo,
}

/// `POST /admin/api/characters/{id}/rename`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RenameBody {
    /// The new name (the rules of 23 §7).
    pub name: String,
}

/// `POST /admin/api/characters/{id}/undelete`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UndeleteBody {
    /// The slot `0..=2` the character returns to.
    pub slot: u8,
}

/// `POST /admin/api/characters/{id}/pet`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PetBody {
    /// Take the summoned pet back into its egg.
    Release,
    /// Revive a dead pet item of the inventory.
    Revive {
        /// The inventory cell.
        slot: u32,
        /// The serial the console saw there.
        serial: u64,
    },
}

/// What a character action did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ActionDone {
    /// `live` (applied to the online character), `pending` (a row waits for the next login) or `offline` (the
    /// stored character was changed under a hold).
    pub mode: String,
    /// `pending_removals.id` or `pending_grants.id` of a pending action.
    pub pending_id: Option<i64>,
    /// A one-line summary of the effect ("removed 3 of 10, 7 left").
    pub summary: String,
}

/// `PUT /admin/api/characters/{id}/login-block`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LoginBlockBody {
    /// Unix seconds the block ends; `None` with `lift = false` blocks for ever.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub until: Option<i64>,
    /// `true` lifts the block (the other fields are ignored).
    #[serde(default)]
    pub lift: bool,
}

/// `POST /admin/api/characters/{id}/ban`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BanBody {
    /// Unix seconds the ban ends; `None` = permanent.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub until: Option<i64>,
}

// ---- accounts, bans, alts -------------------------------------------------------------------------------------------

/// An account ban.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BanView {
    /// `account_bans.id`.
    pub id: i64,
    /// The account.
    pub account_id: u32,
    /// Its name.
    pub account: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds the ban ends; `None` = permanent.
    pub until: Option<i64>,
    /// The reason.
    pub reason: String,
    /// Who banned (`staff:<account>`, `cli`, ...).
    pub by: String,
    /// Unix seconds the ban was lifted.
    pub lifted_at: Option<i64>,
    /// Who lifted it.
    pub lifted_by: Option<String>,
    /// In force now.
    pub active: bool,
}

/// `GET /admin/api/accounts/{name}`: the account drawer. `registration_ip`, `last_login_ip` and `email` are `null`
/// without `view.pii`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AccountView {
    /// `accounts.id`.
    pub id: u32,
    /// The name.
    pub name: String,
    /// The stored authority letter.
    pub authority: String,
    /// The authority letter after the `[gm]` grants (what rank protection compares).
    pub effective_authority: String,
    /// Unix seconds.
    pub created_at: i64,
    /// `cli`, `web` or `bootstrap`.
    pub created_via: String,
    /// Unix seconds of the last login.
    pub last_login_at: Option<i64>,
    /// PII.
    pub last_login_ip: Option<String>,
    /// PII.
    pub registration_ip: Option<String>,
    /// PII.
    pub email: Option<String>,
    /// The password must be changed at the next staff login.
    pub must_change_password: bool,
    /// A TOTP secret is enrolled.
    pub totp_enabled: bool,
    /// A character of the account is in the world.
    pub online: bool,
    /// The ban in force now.
    pub banned: Option<BanView>,
    /// Every character of the account.
    pub characters: Vec<CharacterListItem>,
    /// The ban history, newest first.
    pub bans: Vec<BanView>,
    /// The notes of the account, pinned first.
    pub notes: Vec<NoteView>,
}

/// An account that shares an address with another.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AltView {
    /// `accounts.id`.
    pub account_id: u32,
    /// The name.
    pub name: String,
    /// The stored authority letter.
    pub authority: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub last_login_at: Option<i64>,
    /// The shared addresses (PII; `view.pii` is required for the route).
    pub shared_ips: Vec<String>,
    /// A ban is in force.
    pub banned: bool,
    /// Active characters.
    pub characters: u32,
}

/// `GET /admin/api/accounts/{name}/alts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AltsView {
    /// The window in days.
    pub days: u32,
    /// The other accounts of the same addresses.
    pub alts: Vec<AltView>,
}

/// One account of an alt cluster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ClusterAccount {
    /// `accounts.id`.
    pub account_id: u32,
    /// The name.
    pub name: String,
    /// A ban is in force.
    pub banned: bool,
}

/// Accounts that share one address (Moderation -> Alts).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AltClusterView {
    /// The address (PII).
    pub ip: String,
    /// The accounts (at most 20).
    pub accounts: Vec<ClusterAccount>,
    /// How many share it.
    pub total: u32,
}

/// `GET /admin/api/moderation/alts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AltClustersView {
    /// The window in days.
    pub days: u32,
    /// The clusters, largest first.
    pub clusters: Vec<AltClusterView>,
}

// ---- moderation ---------------------------------------------------------------------------------------------------------

/// An active mute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MuteView {
    /// The muted character.
    pub character_id: CharacterId,
    /// Its account.
    pub account_id: u32,
    /// Its name.
    pub name: String,
    /// Seconds left.
    pub remaining_s: u32,
    /// Unix seconds the mute ends.
    pub until: i64,
}

/// `POST /admin/api/notes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NoteBody {
    /// `account` or `character`.
    pub subject_kind: String,
    /// The subject's id.
    pub subject_id: u32,
    /// The text, 1 to 2,000 characters.
    pub text: String,
    /// Pin it as a banner.
    #[serde(default)]
    pub pinned: bool,
}

/// An IP block of the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IpBlockView {
    /// `admin_ip_blocks.id`.
    pub id: i64,
    /// The network.
    pub cidr: String,
    /// The reason.
    pub reason: String,
    /// Unix seconds the block ends; `None` = for ever.
    pub until: Option<i64>,
    /// Who added it.
    pub created_by: String,
    /// Unix seconds.
    pub created_at: i64,
    /// In force now.
    pub active: bool,
}

/// `GET /admin/api/ip-blocks`: the TOML entries (read-only) and the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IpBlocksView {
    /// `net.ip_block` of `flyff.toml`.
    pub config: Vec<String>,
    /// The `admin_ip_blocks` rows.
    pub table: Vec<IpBlockView>,
}

/// `POST /admin/api/ip-blocks`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IpBlockBody {
    /// A network (`203.0.113.0/24`) or an address.
    pub cidr: String,
    /// Unix seconds the block ends; `None` = for ever.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub until: Option<i64>,
}

// ---- staff -----------------------------------------------------------------------------------------------------------

/// A staff account (Staff & access).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct StaffRow {
    /// `accounts.id`.
    pub account_id: u32,
    /// The name.
    pub account: String,
    /// The resolved letter (account + `[gm]` grants).
    pub authority: String,
    /// The stored letter.
    pub stored_authority: String,
    /// The capabilities of the resolved letter.
    pub caps: Vec<String>,
    /// Unix seconds of the last game login.
    pub last_login_at: Option<i64>,
    /// PII.
    pub last_login_ip: Option<String>,
    /// A TOTP secret is enrolled.
    pub totp_enabled: bool,
    /// The password must be changed.
    pub must_change_password: bool,
    /// The open admin sessions of the account.
    pub sessions: Vec<AdminSessionRow>,
}

/// One row of the capability matrix (Staff & access): what a capability grants and the letter that holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CapRow {
    /// The wire name (`view.map`).
    pub cap: String,
    /// One line of what it grants.
    pub grants: String,
    /// The lowest letter that holds it now (the `[admin.capabilities]` overrides applied).
    pub min: String,
    /// The lowest letter of `caps.toml`; a different `min` is an override.
    pub default_min: String,
}

/// `GET /admin/api/staff`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct StaffView {
    /// The staff accounts (resolved letter `L` or higher).
    pub staff: Vec<StaffRow>,
    /// The sessions of the owner identities (token and local).
    pub owner_sessions: Vec<AdminSessionRow>,
    /// The capability matrix, read-only, with the overrides visible.
    pub matrix: Vec<CapRow>,
}

// ---- search ------------------------------------------------------------------------------------------------------------

/// One palette hit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SearchHit {
    /// `character`, `account`, `guild`, `item` or `location`.
    pub kind: String,
    /// The id as text (a character id, an account name, an item id).
    pub id: String,
    /// The primary label.
    pub label: String,
    /// A secondary line ("Lv 60 Knight, Flaris").
    pub detail: String,
    /// The subject is in the world (characters, accounts).
    pub online: bool,
    /// A location hit: its world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub world: Option<u32>,
    /// A location hit: position x in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub x: Option<f32>,
    /// A location hit: position z in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub z: Option<f32>,
}

/// `GET /admin/api/search?q=`: at most 8 hits per kind, online characters first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SearchResults {
    /// The query as the server read it.
    pub q: String,
    /// Characters.
    pub characters: Vec<SearchHit>,
    /// Accounts.
    pub accounts: Vec<SearchHit>,
    /// Guilds (since WP-ADMIN-23: by name or id, ranked exact, prefix, contains).
    pub guilds: Vec<SearchHit>,
    /// Items by name or id.
    pub items: Vec<SearchHit>,
    /// Locations: the quick locations and shared bookmarks of the map by name or group (WP-ADMIN-14).
    pub locations: Vec<SearchHit>,
}

// ---- the TypeScript file ------------------------------------------------------------------------------------------------

/// Appends the declarations of this module to `out` (the generator of `dto.rs` calls it).
#[cfg(feature = "ts")]
pub fn declare(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;

    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }

    push::<CharacterLife>(out, cfg);
    push::<CharacterListItem>(out, cfg);
    push::<SkillEntry>(out, cfg);
    push::<BuffEntry>(out, cfg);
    push::<QuestEntry>(out, cfg);
    push::<CharacterDetailView>(out, cfg);
    push::<NoteView>(out, cfg);
    push::<CharacterProfile>(out, cfg);
    push::<ItemCell>(out, cfg);
    push::<ContainerView>(out, cfg);
    push::<ContainersView>(out, cfg);
    push::<MailRow>(out, cfg);
    push::<MailboxView>(out, cfg);
    push::<RemoveItemBody>(out, cfg);
    push::<GoldBody>(out, cfg);
    push::<PositionTo>(out, cfg);
    push::<PositionBody>(out, cfg);
    push::<RenameBody>(out, cfg);
    push::<UndeleteBody>(out, cfg);
    push::<PetBody>(out, cfg);
    push::<ActionDone>(out, cfg);
    push::<LoginBlockBody>(out, cfg);
    push::<BanBody>(out, cfg);
    push::<BanView>(out, cfg);
    push::<AccountView>(out, cfg);
    push::<AltView>(out, cfg);
    push::<AltsView>(out, cfg);
    push::<ClusterAccount>(out, cfg);
    push::<AltClusterView>(out, cfg);
    push::<AltClustersView>(out, cfg);
    push::<MuteView>(out, cfg);
    push::<NoteBody>(out, cfg);
    push::<IpBlockView>(out, cfg);
    push::<IpBlocksView>(out, cfg);
    push::<IpBlockBody>(out, cfg);
    push::<StaffRow>(out, cfg);
    push::<CapRow>(out, cfg);
    push::<StaffView>(out, cfg);
    push::<SearchHit>(out, cfg);
    push::<SearchResults>(out, cfg);
}
