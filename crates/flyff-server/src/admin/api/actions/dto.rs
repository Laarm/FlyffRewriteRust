//! The bodies and answers of `POST /admin/api/actions/{action}` and `GET /admin/api/objects/{obj}` (Plan/25 §3.12,
//! §11.3, §11.10; WP-ADMIN-12).
//!
//! Every action takes the `Mutation<T>` envelope (`{body, reason?, confirm?}`) of the DTO catalogue; `T` is the body
//! named here. Positions are `[x, y, z]` in metres (a zero `y` is resolved to the land height by the move or the
//! spawn). A layer that is left out is layer 0. Object ids are the ids of the map frames.
//!
//! The answer of an action is an [`ActionResult`]: one [`TargetOutcome`] per target for the group actions (some of
//! them may have failed: `target_outranks`, `stale_target`, `character_online`, ...), the ids of the new objects for
//! the spawn and the drop, the text lines of a GM line. A request with exactly one target whose action failed answers
//! the error instead (`409 stale_target` with `details.current`, `403 target_outranks`, ...).
//!
//! The types derive serde; with the cargo feature `ts` they derive `ts_rs::TS` as well and are rendered into
//! `web/src/admin/gen/dto.ts` together with the rest of the catalogue (`dto::typescript`).

use crate::admin::dto::{CharacterEdit, CharacterId, ErrorCode, WorldId};
use flyff_game::admin::events::{EntityDetail, HateEntry};
use flyff_game::admin::map::EntityKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An object id of a map frame.
pub type ObjId = u32;

// ---- bodies ---------------------------------------------------------------------------------------------------------

/// Why a teleport was asked for, for the audit (`admin.teleport.mode`): a clicked point, or a quick location.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TeleportLabel {
    /// A point of the map.
    #[default]
    Point,
    /// A quick location (25 §3.13).
    Location,
}

/// `teleport`: move online characters to a point (cap `act.teleport.others`; typed confirmation above 10 characters).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TeleportBody {
    /// Who moves (at most `admin.max_objects_per_action`).
    pub characters: Vec<CharacterId>,
    /// The destination world.
    pub world: WorldId,
    /// The destination layer (default 0).
    #[serde(default)]
    pub layer: i32,
    /// The destination.
    pub pos: [f32; 3],
    /// Point or location (default point).
    #[serde(default)]
    pub label: TeleportLabel,
}

/// `unstuck`: return online characters to the revival point of their continent (cap `act.unstuck`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UnstuckBody {
    /// Who returns.
    pub characters: Vec<CharacterId>,
}

/// `summon`: move a player to the acting GM character (`/su`, cap `act.teleport.self`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SummonBody {
    /// The player to bring.
    pub character: CharacterId,
    /// The acting GM character (25 §1.5): a staff member's own in-world character (left out = chosen
    /// automatically); an owner names a character with authority `L` or above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub as_character: Option<CharacterId>,
}

/// `goto`: move the acting GM character to a player (`/te <name>`) or to a point (`/te <world> <x> <z>`), cap
/// `act.teleport.self`. Give `character`, or `world` and `pos`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GotoBody {
    /// Go to this player.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub character: Option<CharacterId>,
    /// Go to this world ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub world: Option<WorldId>,
    /// ... and this point (the legacy command takes whole metres, both greater than 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub pos: Option<[f32; 3]>,
    /// The acting GM character, as in [`SummonBody`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub as_character: Option<CharacterId>,
}

/// `heal`: HP, MP and FP of live movers to full (cap `act.combat`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HealBody {
    /// The movers (object ids).
    pub objs: Vec<ObjId>,
    /// The world the map showed them in (an object that is elsewhere is a stale target).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub world: Option<WorldId>,
    /// The layer the map showed them in (default 0 when `world` is given).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub layer: Option<i32>,
}

/// `resurrect`: bring a dead character back (cap `act.combat`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ResurrectBody {
    /// The character.
    pub character: CharacterId,
}

/// `kill`: kill monsters or players through the damage pipeline (cap `act.combat`; typed confirmation when a player is
/// among the targets or above 10 targets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct KillBody {
    /// The objects.
    pub objs: Vec<ObjId>,
    /// `true` (default): the normal death with EXP and drops; `false`: nobody is credited.
    #[serde(default = "yes")]
    pub drops: bool,
    /// The world the map showed them in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub world: Option<WorldId>,
    /// The layer the map showed them in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub layer: Option<i32>,
}

fn yes() -> bool {
    true
}

/// `despawn`: remove monsters, NPCs, pets, ground items or ctrls without a death (cap `act.combat`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DespawnBody {
    /// The objects.
    pub objs: Vec<ObjId>,
    /// The world the map showed them in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub world: Option<WorldId>,
    /// The layer the map showed them in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub layer: Option<i32>,
}

/// A kind of object an area clear removes (never a player).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AreaKind {
    /// Monsters.
    Monster,
    /// Pets.
    Pet,
    /// `character.inc` NPCs.
    Npc,
    /// Ground items.
    Item,
    /// Chests, doors, traps and triggers.
    Ctrl,
}

/// `despawn-area`: remove every object of some kinds inside a rectangle (cap `act.combat`; always a typed
/// confirmation; more than `admin.max_objects_per_action` objects answer `413` and remove nothing).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DespawnAreaBody {
    /// The world.
    pub world: WorldId,
    /// The layer (default 0).
    #[serde(default)]
    pub layer: i32,
    /// `[x0, z0, x1, z1]` in metres.
    pub rect: [f32; 4],
    /// What to remove.
    pub kinds: Vec<AreaKind>,
}

/// `spawn`: create monsters at a point (cap `act.combat`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SpawnBody {
    /// The world.
    pub world: WorldId,
    /// The layer (default 0).
    #[serde(default)]
    pub layer: i32,
    /// The centre.
    pub pos: [f32; 3],
    /// The `MoverProp` id (the AI kinds of `/createnpc` only).
    pub prop: u32,
    /// How many, 1..=100.
    pub count: u32,
    /// Scatter radius in metres, 0..=30 (default 0).
    #[serde(default)]
    pub spread: f32,
    /// Spawn them aggressive (default false).
    #[serde(default)]
    pub aggressive: bool,
    /// Remove them after this many seconds (default: they stay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub lifetime_s: Option<u32>,
}

/// `drop-item`: put an item on the ground (cap `act.items`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DropItemBody {
    /// The world.
    pub world: WorldId,
    /// The layer (default 0).
    #[serde(default)]
    pub layer: i32,
    /// Where it lies.
    pub pos: [f32; 3],
    /// The normalised item of `POST /admin/api/items/validate` (a bare `{"item_id": 2800, "count": 3}` is accepted).
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub item: flyff_db::StoredItem,
    /// The character with loot priority; left out = anyone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub owner: Option<CharacterId>,
}

/// `respawn-region`: make a respawn line due now (cap `act.world`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RespawnBody {
    /// The world.
    pub world: WorldId,
    /// The layer (default 0).
    #[serde(default)]
    pub layer: i32,
    /// The respawn line (`EntityDetail.spawn_region - 1`).
    pub region: u32,
}

/// `mute`: mute an online character (cap `act.mute`; a reason is required).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MuteBody {
    /// The character.
    pub character: CharacterId,
    /// For how long; left out = permanent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub seconds: Option<u32>,
}

/// `unmute`: lift a mute (cap `act.mute`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UnmuteBody {
    /// The character.
    pub character: CharacterId,
}

/// How a message is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MessageStyle {
    /// The large notice line.
    Notice,
    /// A `[System]` line in the chat window.
    Whisper,
}

/// `message`: a message to online characters (cap `act.message.player`; typed confirmation above 10 recipients).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MessageBody {
    /// The recipients.
    pub characters: Vec<CharacterId>,
    /// The text, at most 511 bytes.
    pub text: String,
    /// The style.
    pub style: MessageStyle,
}

/// The mode bits the console can name (23 §2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ModeFlagName {
    /// `/ud`: no damage.
    Matchless,
    /// `/inv`: invisible.
    Transparent,
    /// `/ok`: every hit kills.
    Onekill,
    /// `/fr`: frozen (the client blocks movement).
    Donmove,
    /// Whispers and `/say` are dropped.
    Saytalk,
    /// `/ud2`: damage taken, never dies.
    Matchless2,
    /// `/gmattck`: no attacks.
    NoAttack,
    /// `/gmitem`: no pick-up, no drop.
    Item,
    /// `/gmcommunity`.
    Community,
    /// Normal chat is dropped.
    Talk,
    /// Shouts are dropped.
    Shouttalk,
    /// `/es`: kills give no EXP.
    ExpupStop,
    /// `Talk | Saytalk | Shouttalk` (`/notalk`).
    Dontalk,
    /// `Item | NoAttack` (`/gmobserve`).
    Observe,
}

impl ModeFlagName {
    /// The `m_dwMode` bits of the name.
    pub fn bits(self) -> u32 {
        use flyff_common::gm::ModeFlags as M;
        match self {
            ModeFlagName::Matchless => M::MATCHLESS,
            ModeFlagName::Transparent => M::TRANSPARENT,
            ModeFlagName::Onekill => M::ONEKILL,
            ModeFlagName::Donmove => M::DONMOVE,
            ModeFlagName::Saytalk => M::SAYTALK,
            ModeFlagName::Matchless2 => M::MATCHLESS2,
            ModeFlagName::NoAttack => M::NO_ATTACK,
            ModeFlagName::Item => M::ITEM,
            ModeFlagName::Community => M::COMMUNITY,
            ModeFlagName::Talk => M::TALK,
            ModeFlagName::Shouttalk => M::SHOUTTALK,
            ModeFlagName::ExpupStop => M::EXPUP_STOP,
            ModeFlagName::Dontalk => M::DONTALK,
            ModeFlagName::Observe => M::OBSERVE,
        }
        .bits()
    }
}

/// `mode-flags`: set and clear mode bits of an online character (cap `act.character.edit`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ModeFlagsBody {
    /// The character.
    pub character: CharacterId,
    /// The flags to set.
    #[serde(default)]
    pub set: Vec<ModeFlagName>,
    /// The flags to clear.
    #[serde(default)]
    pub clear: Vec<ModeFlagName>,
}

/// `edit-character`: level, job, stats, skill points, skill reset (cap `act.character.edit`). A `rename` also needs
/// `act.accounts` and a typed confirmation (WP-ADMIN-16 executes it); `quest` edits answer `501 needs_feature` until
/// WP-ADMIN-31.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EditCharacterBody {
    /// The character.
    pub character: CharacterId,
    /// The edit.
    pub edit: CharacterEdit,
}

// ---- answers --------------------------------------------------------------------------------------------------------

/// What one outcome of an action is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TargetRef {
    /// A world object.
    Object(ObjId),
    /// A character.
    Character(CharacterId),
    /// A guild (the guild commands never travel through `/admin/api/actions`; the wire enum stays total).
    Guild(u32),
}

/// Why one target was skipped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TargetError {
    /// The code (`stale_target`, `target_outranks`, `character_online`, `not_found`, `invalid`, ...).
    pub code: ErrorCode,
    /// English message.
    pub message: String,
}

/// The outcome for one target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TargetOutcome {
    /// The target.
    pub target: TargetRef,
    /// It was handled.
    pub ok: bool,
    /// Why not.
    pub error: Option<TargetError>,
    /// What the action did to it: the place a teleport came from, the values an edit replaced, the kind of a removed
    /// object.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub info: Option<Value>,
}

/// The answer of `POST /admin/api/actions/{action}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ActionResult {
    /// The action.
    pub action: String,
    /// Targets handled (new objects for a spawn).
    pub ok: u32,
    /// Targets skipped.
    pub failed: u32,
    /// One entry per target in request order (empty for a spawn or a GM line).
    pub results: Vec<TargetOutcome>,
    /// The objects a spawn or a drop created.
    pub spawned: Vec<ObjId>,
    /// The output lines of a GM line (`summon`, `goto`).
    pub lines: Vec<String>,
    /// The number of things changed, for the actions that count (`respawn-region`: objects the line was missing).
    pub affected: Option<u32>,
}

// ---- objects ----------------------------------------------------------------------------------------------------------

/// One entry of a monster's hate list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HateRow {
    /// The enemy.
    pub obj_id: ObjId,
    /// Its name.
    pub name: String,
    /// The summed damage.
    pub hate: u32,
}

/// `GET /admin/api/objects/{obj}`: the inspector card of an object (25 §3.10).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ObjectDetail {
    /// The object.
    pub obj_id: ObjId,
    /// `player`, `monster`, `npc`, `pet`, `item` or `ctrl`.
    pub kind: String,
    /// `MoverProp` / `ItemProp` / `CtrlProp` id.
    pub prop: u32,
    /// The display name.
    pub name: String,
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: i32,
    /// Position x.
    pub x: f32,
    /// Position y.
    pub y: f32,
    /// Position z.
    pub z: f32,
    /// Level.
    pub level: u32,
    /// Hit points.
    pub hp: u32,
    /// Maximum hit points.
    pub hp_max: u32,
    /// Mana points.
    pub mp: u32,
    /// Maximum mana points.
    pub mp_max: u32,
    /// Fatigue points.
    pub fp: u32,
    /// Maximum fatigue points.
    pub fp_max: u32,
    /// The map flag bits.
    pub flags: u32,
    /// The names of the set flags (`asleep`, `dead`, `fighting`, `aggressive`, `pk`, `gm`, `invisible`, `owner_only`,
    /// `boss`, `flying`).
    pub flag_names: Vec<String>,
    /// The AI state (`idle`, `chase`, `asleep`, `dead`, ...); empty for players, items and ctrls.
    pub ai_state: String,
    /// The monster's target, 0 = none.
    pub target: ObjId,
    /// The top of the hate list.
    pub hate: Vec<HateRow>,
    /// The respawn line plus one; 0 = it did not come from one.
    pub spawn_region: u32,
    /// The tick of the line's next refill check; 0 = unknown.
    pub respawn_tick: u64,
    /// The character id (players), 0 otherwise.
    pub character_id: CharacterId,
    /// The owner of a pet or a summon, the loot owner of a ground item; 0 = none.
    pub owner: ObjId,
    /// The `m_dwMode` word of a player.
    pub mode_flags: u32,
}

/// The name of an [`EntityKind`] on the wire.
pub fn kind_name(k: EntityKind) -> &'static str {
    match k {
        EntityKind::Player => "player",
        EntityKind::Monster => "monster",
        EntityKind::Npc => "npc",
        EntityKind::Pet => "pet",
        EntityKind::Item => "item",
        EntityKind::Ctrl => "ctrl",
    }
}

fn flag_names(bits: u32) -> Vec<String> {
    use flyff_game::admin::map::MapFlags as F;
    [
        (F::ASLEEP, "asleep"),
        (F::DEAD, "dead"),
        (F::FIGHTING, "fighting"),
        (F::AGGRESSIVE, "aggressive"),
        (F::PK, "pk"),
        (F::GM, "gm"),
        (F::INVISIBLE, "invisible"),
        (F::OWNER_ONLY, "owner_only"),
        (F::BOSS, "boss"),
        (F::FLYING, "flying"),
    ]
    .into_iter()
    .filter(|(bit, _)| bits & u32::from(*bit) != 0)
    .map(|(_, n)| n.to_owned())
    .collect()
}

impl From<&EntityDetail> for ObjectDetail {
    fn from(d: &EntityDetail) -> ObjectDetail {
        ObjectDetail {
            obj_id: d.obj_id,
            kind: kind_name(d.kind).to_owned(),
            prop: d.prop,
            name: d.name.clone(),
            world: d.world,
            layer: d.layer,
            x: d.x,
            y: d.y,
            z: d.z,
            level: d.level,
            hp: d.hp,
            hp_max: d.hp_max,
            mp: d.mp,
            mp_max: d.mp_max,
            fp: d.fp,
            fp_max: d.fp_max,
            flags: d.flags,
            flag_names: flag_names(d.flags),
            ai_state: d.ai_state.clone(),
            target: d.target,
            hate: d.hate.iter().map(HateRow::from).collect(),
            spawn_region: d.spawn_region,
            respawn_tick: d.respawn_tick,
            character_id: d.character_id,
            owner: d.owner,
            mode_flags: d.mode_flags,
        }
    }
}

impl From<&HateEntry> for HateRow {
    fn from(h: &HateEntry) -> HateRow {
        HateRow {
            obj_id: h.obj_id,
            name: h.name.clone(),
            hate: h.hate,
        }
    }
}

/// Adds the declarations of this module to the TypeScript file of the DTO catalogue (`dto::typescript`).
#[cfg(feature = "ts")]
pub fn push_all(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;

    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }

    push::<TeleportLabel>(out, cfg);
    push::<TeleportBody>(out, cfg);
    push::<UnstuckBody>(out, cfg);
    push::<SummonBody>(out, cfg);
    push::<GotoBody>(out, cfg);
    push::<HealBody>(out, cfg);
    push::<ResurrectBody>(out, cfg);
    push::<KillBody>(out, cfg);
    push::<DespawnBody>(out, cfg);
    push::<AreaKind>(out, cfg);
    push::<DespawnAreaBody>(out, cfg);
    push::<SpawnBody>(out, cfg);
    push::<DropItemBody>(out, cfg);
    push::<RespawnBody>(out, cfg);
    push::<MuteBody>(out, cfg);
    push::<UnmuteBody>(out, cfg);
    push::<MessageStyle>(out, cfg);
    push::<MessageBody>(out, cfg);
    push::<ModeFlagName>(out, cfg);
    push::<ModeFlagsBody>(out, cfg);
    push::<EditCharacterBody>(out, cfg);
    push::<TargetRef>(out, cfg);
    push::<TargetError>(out, cfg);
    push::<TargetOutcome>(out, cfg);
    push::<ActionResult>(out, cfg);
    push::<HateRow>(out, cfg);
    push::<ObjectDetail>(out, cfg);
}
