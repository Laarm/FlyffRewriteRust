//! The mover data model (Plan/16 §3, WP-MOVER-02): `Mover` = `ObjCore` (15) + `SimMover` (31) + `MoverCore`
//! (16) + combat / buffs / status (17) + a kind extension.
//!
//! **One location per datum** (16 §3.1). The position, yaw and scale are `core`'s, the action state, flag word,
//! animation speed, fuel, move pattern and speed factor the sim's, the attack counter and invulnerability
//! ticks `combat`'s, the heal counter `status`'s; `MoverCore` ([`MoverCore`], `Mover.mv`) holds only what no
//! other owner has. The following do not compile, and the matching accessors do:
//!
//! ```
//! use flyff_game::mover::Mover;
//! fn ok(m: &Mover) {
//!     let _ = (m.core.pos, m.core.scale, m.sim.ani_speed, m.sim.params.speed_factor, m.sim.mvrf);
//!     let _ = (m.sim.atk_cnt, m.combat.dead_ticks, m.status.heal_cnt);
//!     let _ = (m.sim.fuel, m.sim.tm_acc_fuel, m.sim.pattern);
//! }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.pos; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.scale; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.atk_cnt; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.dead_ticks; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.heal_cnt; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.ani_speed; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.speed_factor; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { let _ = m.mv.flags; }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::Mover;
//! fn f(m: &Mover) { if let Some(p) = m.player() { let _ = p.progression.fuel; } }
//! ```
//!
//! ```compile_fail,E0609
//! # use flyff_game::mover::MonsterExt;
//! fn f(m: &MonsterExt) { let _ = m.move_pattern; }
//! ```
//!
//! Movers live in the world arenas of doc 15; handlers never hold two `&mut Mover` at once.

mod construct;
pub mod wire;

pub use construct::{LoadRepair, NewPlayer, model_scale, monster_parts, npc_parts, player_parts};
pub use wire::{
    add_game_join, mover_common, npc_part, player_other, player_public, player_self, spawn_buffs,
};

use crate::ai::AiSlot;
use crate::buff::{BuffSet, StatusEffects};
use crate::combat::CombatState;
use crate::player::PlayerExt;
use crate::stats::DstState;
use crate::world::ObjCore;
use flyff_common::{ObjId, math::Vec3};
use flyff_data::{
    GameData,
    props::mover::MoverProp,
    types::{ItemId, MenuId, MoverId},
};
use flyff_rules::consts::{MAX_HUMAN_PARTS, NULL_ID};
use flyff_sim::SimMover;

// The base stats, the equipment cache and the per-prop rates are defined once, in `flyff-rules` (they are
// inputs of the shared formulas, 16 §5.2), and re-exported here under their historical paths.
pub use flyff_rules::stats::{BaseStats, EquipCache, MonsterRates};

/// `m_aEquipInfo[part]` (`EQUIP_INFO`, 12 bytes): the appearance of one equipment part for other clients.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquipInfo {
    /// `dwId`: the item id; [`NULL_ID`] = empty.
    pub item_id: u32,
    /// `nOption`: `GetAttrOption()` = `ability_option | element << 16 | element_level << 24`.
    pub option: i32,
    /// `byFlag`: the item flag byte.
    pub flag: u8,
}

impl EquipInfo {
    /// An empty part.
    pub const EMPTY: EquipInfo = EquipInfo {
        item_id: NULL_ID,
        option: 0,
        flag: 0,
    };

    /// `true` when nothing is equipped in this part.
    pub fn is_empty(&self) -> bool {
        self.item_id == NULL_ID
    }
}

/// What every mover has and no other owner does (16 §3.2).
#[derive(Clone, Debug, PartialEq)]
pub struct MoverCore {
    /// `m_nLevel` (NPCs: `max(prop.level, 1)`).
    pub level: i32,
    /// `m_nJob`: `-1` for monsters and NPCs.
    pub job: i32,
    /// The base stats (NPCs: from the prop).
    pub base: BaseStats,
    /// `m_nHitPoint`: the raw stored pool (the getters clamp, 16 §4.8).
    pub hp: i32,
    /// `m_nManaPoint`.
    pub mp: i32,
    /// `m_nFatiguePoint`.
    pub fp: i32,
    /// `m_adjParamAry` / `m_chgParamAry` and, later, the per-source ledger.
    pub dst: DstState,
    /// The cached equipment values.
    pub equip_cache: EquipCache,
    /// `m_aEquipInfo[31]`; humanoid NPCs from `character.inc`.
    pub equip_info: [EquipInfo; MAX_HUMAN_PARTS],
    /// `m_dwVirtItem`: the attack item (NPC `prop.atk1`; player default `II_WEA_HAN_HAND`).
    pub virt_item: ItemId,
    /// `m_dwMode` (players persisted).
    pub mode: u32,
    /// `m_dwStateMode`.
    pub state_mode: u32,
    /// `m_dwTypeFlag` (`OBJTYPE_FLYING` when `prop.flying == 1`).
    pub type_flags: u32,
    /// `m_nCount`: random start in `0..128` at creation, +1 per tick.
    pub count: u32,
    /// The disguise buff's mover (`m_dwDisguise`), if any.
    pub disguise: Option<MoverId>,
    /// The per-prop GM multipliers.
    pub rates: MonsterRates,
}

/// A monster, guard, pet or event monster (16 §3.4).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonsterExt {
    /// The AI (doc 18).
    pub ai: AiSlot,
    /// The owner of a pet or summon.
    pub owner: Option<ObjId>,
    /// `dwClass` (`RANK_*`).
    pub class: u32,
    /// `m_dwBelligerence`.
    pub belligerence: u32,
    /// `m_bActiveAttack`: set by the respawn cap at spawn (15 §15.4), read by the AI.
    pub active_attack: bool,
    /// `m_nGold` of the monster: `level * 15` at respawn (`SetGold`, 15 §15.3); what Hit of Penya can take.
    pub gold: i32,
    /// The owner of a monster an item summoned (`CCreateMonster`, 18 §14, WP-AI-16): only the owner and the
    /// owner's party may attack it. The global registry of the limit and the expiry is
    /// `Services.summons`; this copy lets the damage pipeline decide without the services.
    pub summon: Option<crate::ai::create_monster::SummonTag>,
}

/// A `character.inc` NPC (16 §3.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NpcExt {
    /// The `character.inc` key (`m_szCharacterKey`; empty for NPCs without a definition).
    pub character_key: String,
    /// `m_dwHairMesh`.
    pub hair_mesh: u32,
    /// `m_dwHairColor` (ARGB).
    pub hair_color: u32,
    /// `m_dwHeadMesh`.
    pub head_mesh: u32,
    /// `m_dwBelligerence` (from the prop; also carried by monsters in [`MonsterExt`]).
    pub belligerence: u32,
    /// The enabled `MMI_*` menu entries (`NpcDef.menus`): non-empty means the NPC is registered in doc 20's
    /// proximity index at `_add` (15 §13.3).
    pub menus: Box<[MenuId]>,
}

/// The kind extension of a mover (16 §3.1).
#[derive(Clone, Debug)]
pub enum MoverKind {
    /// A player character.
    Player(Box<PlayerExt>),
    /// A monster, guard, pet or event monster.
    Monster(Box<MonsterExt>),
    /// A `character.inc` NPC.
    Npc(Box<NpcExt>),
}

/// A mover: player, monster or NPC (16 §3.1).
#[derive(Clone, Debug)]
pub struct Mover {
    /// The object header (doc 15, embedded unchanged).
    pub core: ObjCore,
    /// The kinematic state of the shared simulation (doc 31).
    pub sim: SimMover,
    /// What no other owner has.
    pub mv: MoverCore,
    /// Attack counter, invulnerability, damage ledger, PK timers (doc 17).
    pub combat: CombatState,
    /// The live buffs (doc 17).
    pub buffs: BuffSet,
    /// Abnormal-state counters (doc 17).
    pub status: StatusEffects,
    /// The server-side movement bookkeeping: pending intents, sim events, keyframe state (doc 18 §5.2).
    pub sync: crate::movement::MoveSync,
    /// Player, monster or NPC.
    pub kind: MoverKind,
}

impl Mover {
    /// The object id (`core.id`; `ObjId::NULL` until the object store assigns one).
    pub fn id(&self) -> ObjId {
        self.core.id
    }

    /// The `MoverProp` id (`m_dwIndex`; players `MI_MALE` 11 / `MI_FEMALE` 12) = `core.index`.
    pub fn prop_id(&self) -> MoverId {
        self.core.index
    }

    /// `true` for a player.
    pub fn is_player(&self) -> bool {
        matches!(self.kind, MoverKind::Player(_))
    }

    /// The player extension.
    pub fn player(&self) -> Option<&PlayerExt> {
        match &self.kind {
            MoverKind::Player(p) => Some(p),
            _ => None,
        }
    }

    /// The player extension, mutably.
    pub fn player_mut(&mut self) -> Option<&mut PlayerExt> {
        match &mut self.kind {
            MoverKind::Player(p) => Some(p),
            _ => None,
        }
    }

    /// The monster extension.
    pub fn monster(&self) -> Option<&MonsterExt> {
        match &self.kind {
            MoverKind::Monster(m) => Some(m),
            _ => None,
        }
    }

    /// The monster extension, mutably.
    pub fn monster_mut(&mut self) -> Option<&mut MonsterExt> {
        match &mut self.kind {
            MoverKind::Monster(m) => Some(m),
            _ => None,
        }
    }

    /// The NPC extension.
    pub fn npc(&self) -> Option<&NpcExt> {
        match &self.kind {
            MoverKind::Npc(n) => Some(n),
            _ => None,
        }
    }

    /// `gd.movers[prop_id()]`.
    pub fn prop<'g>(&self, gd: &'g GameData) -> Option<&'g MoverProp> {
        gd.mover(self.prop_id())
    }

    /// The position (the read-only copy of the sim position, 15 §5.5).
    pub fn pos(&self) -> Vec3 {
        self.core.pos
    }
}
