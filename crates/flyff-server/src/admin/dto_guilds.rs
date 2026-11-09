//! The DTOs of the guild area (Plan/25 §6, §11.3, §11.10; WP-ADMIN-23): the list row, the detail with the roster and
//! the bank, the metadata of the wizard, the bank log and war pages, the name check and the answers of the
//! mutations. `GuildSpec`, `GuildEdit` and `GuildRank` stay in [`super::dto`] (the plan's catalogue); this module is
//! declared after them in the generated `web/src/admin/gen/dto.ts` (`declare`).
//!
//! The list is served as the generic [`super::dto::Page`] of [`GuildRow`]; the cursor is the offset into the sorted
//! list as an opaque string. Conventions as in [`super::dto`]: enums `snake_case`, ids `u32`, response types always
//! write every key (`null` when empty), request types accept a missing optional field.

use super::dto::{CharacterId, GuildId, GuildRank};
use super::dto_players::ContainerView;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---- the list -------------------------------------------------------------------------------------------------------

/// `GET /admin/api/guilds?q=&level_min=&level_max=&at_war=&member=&sort=&dir=&cursor=&limit=`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildListQuery {
    /// A name fragment (case-insensitive), or the exact guild id.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub q: Option<String>,
    /// At least this level.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub level_min: Option<u8>,
    /// At most this level.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub level_max: Option<u8>,
    /// Only guilds at war (`true`) or at peace (`false`).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub at_war: Option<bool>,
    /// Only the guild of this character.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub member: Option<CharacterId>,
    /// `name`, `level`, `members`, `gold`, `pxp` or `created` (default `name`).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub sort: Option<String>,
    /// `asc` or `desc` (default `asc`).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub dir: Option<String>,
    /// The cursor of a previous page.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub cursor: Option<String>,
    /// Rows per page, 1 to 500 (default 100).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub limit: Option<u32>,
}

/// A character with its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildPerson {
    /// The character.
    pub id: CharacterId,
    /// Its name.
    pub name: String,
}

/// The war a guild fights right now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildWarState {
    /// The war.
    pub war_id: u32,
    /// The enemy guild.
    pub enemy_id: GuildId,
    /// Its name (`""` when unnamed or gone).
    pub enemy_name: String,
}

/// One row of the guild list (Plan/25 §6.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildRow {
    /// The guild.
    pub id: GuildId,
    /// The name; empty until the master named the guild.
    pub name: String,
    /// `true` once the guild has its name.
    pub named: bool,
    /// Level `1..=50`.
    pub level: u32,
    /// The logo: 0 none, 1..=27, 999 a custom cloak.
    pub logo: u32,
    /// The master (`null` for a guild whose master was purged).
    pub master: Option<GuildPerson>,
    /// Members.
    pub members: u32,
    /// Members in the world.
    pub online: u32,
    /// The member limit of the level.
    pub max_members: u32,
    /// The penya pool.
    pub gold: u64,
    /// The contribution points.
    pub pxp: u32,
    /// The running war.
    pub war: Option<GuildWarState>,
    /// Creation time (unix seconds).
    pub created_at: i64,
}

// ---- the wizard's metadata ------------------------------------------------------------------------------------------

/// The member limit of one guild level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildLevelRow {
    /// The level `1..=50`.
    pub level: u32,
    /// How many members a guild of this level may have.
    pub max_members: u32,
}

/// `GET /admin/api/guilds/meta`: what the wizard and the drawer clamp to, built without a game round trip.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildMeta {
    /// The member limit of every level.
    pub levels: Vec<GuildLevelRow>,
    /// How many members the ranks Master, Kingpin, Captain and Supporter may have; the Rookie cap is the member
    /// limit of the level.
    pub rank_caps: Vec<u32>,
    /// The highest logo (27).
    pub logo_max: u32,
    /// The first logo that is GM-only in the game (21).
    pub logo_gm_from: u32,
    /// The shortest guild name.
    pub name_min: u32,
    /// The longest guild name.
    pub name_max: u32,
    /// The longest notice in bytes.
    pub notice_max_bytes: u32,
    /// The largest guild penya pool.
    pub gold_max: u64,
    /// `true` when the rejoin cooldown exists (`features.remove_guild_cooldown` is off): the override applies.
    pub cooldowns: bool,
    /// Penya changes above this need a typed confirmation (`admin.confirm_gold_threshold`).
    pub confirm_gold_threshold: u64,
}

// ---- the detail -----------------------------------------------------------------------------------------------------

/// The war record of a guild.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildWarRecord {
    /// Wins.
    pub wins: i32,
    /// Losses.
    pub losses: i32,
    /// Surrenders.
    pub surrenders: i32,
    /// War points.
    pub points: i32,
}

/// One member of the roster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildMemberView {
    /// The character.
    pub character: CharacterId,
    /// Its name.
    pub name: String,
    /// Its account.
    pub account_id: u32,
    /// The rank (`member` is the Rookie).
    pub rank: GuildRank,
    /// The class within the rank `0..=2`.
    pub class: u32,
    /// The nickname.
    pub alias: String,
    /// The character level.
    pub level: u32,
    /// The job id.
    pub job: u32,
    /// 0 male, 1 female.
    pub sex: u32,
    /// In the world.
    pub online: bool,
    /// Penya contributed.
    pub give_gold: i64,
    /// Contribution points given.
    pub give_pxp: u32,
    /// Surrenders of this member.
    pub surrenders: i32,
    /// When the member joined (unix seconds).
    pub joined_at: i64,
    /// The last time an offline member was in the world (`null` while online or unknown).
    pub last_seen: Option<i64>,
}

/// How many bank log rows the in-memory tail holds per kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildLogTail {
    /// Kind `A`.
    pub put: u32,
    /// Kind `D`.
    pub taken: u32,
    /// Kind `I`.
    pub penya_in: u32,
    /// Kind `O`.
    pub penya_out: u32,
}

/// `GET /admin/api/guilds/{id}`: the drawer's data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildDetailView {
    /// The list row.
    pub guild: GuildRow,
    /// The notice.
    pub notice: String,
    /// The permission bits per rank (Master .. Rookie).
    pub powers: Vec<u32>,
    /// The daily salary per rank.
    pub salaries: Vec<u32>,
    /// Wins, losses, surrenders and points.
    pub war_record: GuildWarRecord,
    /// The running war.
    pub war: Option<GuildWarState>,
    /// How many members each rank may have at this level (Master .. Rookie).
    pub rank_caps: Vec<u32>,
    /// The roster in rank order, then by name.
    pub members: Vec<GuildMemberView>,
    /// The bank: 42 cells and the pool. Without `view.inventory` the cells are stripped (`bank_hidden`).
    pub bank: ContainerView,
    /// The cells of the bank were left out for lack of `view.inventory`.
    pub bank_hidden: bool,
    /// The log rows the game keeps in memory per kind.
    pub log_tail: GuildLogTail,
}

// ---- the bank log and the wars --------------------------------------------------------------------------------------

/// One row of the bank log (`GET /admin/api/guilds/{id}/log`, served as a `Page`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildLogRow {
    /// The row id (the `before` cursor).
    pub id: i64,
    /// The acting character, `0` = the system.
    pub character_id: CharacterId,
    /// Its name (`""` for the system, `#id` when gone).
    pub character_name: String,
    /// `A`, `D`, `I`, `O`, `C`, `P`, `R` or `L`.
    pub kind: String,
    /// The item (kinds `A`, `D`, `C`, `R`), else 0.
    pub item_id: u32,
    /// `+N` of the item.
    pub upgrade_level: i32,
    /// The item count, or the penya for `I`, `O`, `P`, `L`.
    pub count: i64,
    /// When (unix seconds).
    pub at: i64,
}

/// One side of a war.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildWarSide {
    /// Members at the start.
    pub members: i32,
    /// Deaths.
    pub deaths: i32,
    /// Surrenders.
    pub surrenders: i32,
    /// Seconds the master was offline.
    pub absent_s: i32,
}

/// One row of the war history (`GET /admin/api/guilds/{id}/wars`, served as a `Page`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildWarRow {
    /// The war (the `before` cursor).
    pub id: u32,
    /// The declaring guild.
    pub declarer: GuildId,
    /// Its name (`#id` when the guild is gone).
    pub declarer_name: String,
    /// The accepting guild.
    pub acceptor: GuildId,
    /// Its name (`#id` when the guild is gone).
    pub acceptor_name: String,
    /// Start (unix seconds).
    pub started_at: i64,
    /// End; `null` while the war runs.
    pub ended_at: Option<i64>,
    /// The `WR_*` code `0..=9`; `null` while the war runs.
    pub result: Option<u32>,
    /// `declarer`, `acceptor`, `truce` or `draw`; `null` while the war runs.
    pub outcome: Option<String>,
    /// The declarer's counters.
    pub decl: GuildWarSide,
    /// The acceptor's counters.
    pub acpt: GuildWarSide,
    /// The declarer's win points after the war.
    pub decl_points_after: Option<i32>,
    /// The acceptor's win points after the war.
    pub acpt_points_after: Option<i32>,
}

// ---- the name check and the mutations -------------------------------------------------------------------------------

/// What is wrong with a guild name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildNameProblem {
    /// `length`, `chars`, `forbidden` or `taken`.
    pub code: String,
    /// For the operator.
    pub message: String,
}

/// `GET /admin/api/guilds/validate-name?name=`: always 200, the problem is in the body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildNameCheck {
    /// The name can be given to a guild now.
    pub ok: bool,
    /// The trimmed name.
    pub name: String,
    /// What is wrong.
    pub error: Option<GuildNameProblem>,
}

/// `POST /admin/api/guilds/{id}/members` (inside a `Mutation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AddMemberBody {
    /// The character to add.
    pub character: CharacterId,
    /// Its rank.
    pub rank: GuildRank,
}

/// `201` of `POST /admin/api/guilds`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildCreated {
    /// The new guild.
    pub id: GuildId,
    /// The delivery of the guild cloak to the master (`null` when none was asked for); a failed cloak never undoes the
    /// guild: `{failed: {code, message}}`.
    pub cloak: Option<super::dto::GrantOutcome>,
}

/// The answer of `PATCH /admin/api/guilds/{id}`, the member routes and `DELETE /admin/api/guilds/{id}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GuildChanged {
    /// `false` when the target state was reached already (an idempotent repeat).
    pub changed: bool,
    /// The `admin.guild` audit data of every change: `{action, guild, character?, before, after}`.
    #[cfg_attr(feature = "ts", ts(type = "unknown[]"))]
    pub changes: Vec<Value>,
}

/// Declares the types of this module in the generated `dto.ts`.
#[cfg(feature = "ts")]
pub fn declare(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;

    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }

    push::<GuildListQuery>(out, cfg);
    push::<GuildPerson>(out, cfg);
    push::<GuildWarState>(out, cfg);
    push::<GuildRow>(out, cfg);
    push::<GuildLevelRow>(out, cfg);
    push::<GuildMeta>(out, cfg);
    push::<GuildWarRecord>(out, cfg);
    push::<GuildMemberView>(out, cfg);
    push::<GuildLogTail>(out, cfg);
    push::<GuildDetailView>(out, cfg);
    push::<GuildLogRow>(out, cfg);
    push::<GuildWarSide>(out, cfg);
    push::<GuildWarRow>(out, cfg);
    push::<GuildNameProblem>(out, cfg);
    push::<GuildNameCheck>(out, cfg);
    push::<AddMemberBody>(out, cfg);
    push::<GuildCreated>(out, cfg);
    push::<GuildChanged>(out, cfg);
}
