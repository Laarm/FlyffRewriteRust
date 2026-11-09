//! Construction of movers (Plan/16 §3.5): `Mover::new_monster`, `new_npc` and `new_player`.
//!
//! The constructors take the pieces that doc 15's spawn API owns, an [`ObjCore`] (kind, prop index, position,
//! angle, scale; world and layer are stamped by `Worlds::spawn_mover`, the id by the object store) and the
//! [`SimMover`] (`SimMover::new(id, pos, angle, params)`); [`monster_parts`], [`npc_parts`] and
//! [`player_parts`] build both from the prop so a spawner needs no knowledge of the sim parameters.
//! `combat`, `buffs` and `status` start at their owners' `Default`.

use super::{
    BaseStats, EquipCache, EquipInfo, MonsterExt, MonsterRates, Mover, MoverCore, MoverKind, NpcExt,
};
use crate::ai::{AI_STATE_DEFAULT, AiSlot};
use crate::buff::{BuffSet, StatusEffects};
use crate::combat::CombatState;
use crate::input::Authority;
use crate::item::PlayerItems;
use crate::player::{
    Appearance, DeathFlags, PkFields, PlayerExt, PlayerIdent, PlayerMisc, Progression, RegenTimers,
    SmState,
};
use crate::quest::QuestBook;
use crate::skill::SkillRuntime;
use crate::social::PlayerSocial;
use crate::world::{ObjCore, ObjKind, PlayerView};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, LayerId, ObjId, WorldId};
use flyff_data::character::NpcDef;
use flyff_data::{GameData, props::mover::MoverProp};
use flyff_db::{CharacterLoad, ContainerRecord, InventoryDoc, ItemRecord};
use flyff_rules::consts::defs::item::II_WEA_HAN_HAND;
use flyff_rules::consts::defs::neuz::{AII_EGG, AII_MONSTER, AII_PET};
use flyff_rules::consts::defs::obj::{MI_FEMALE, MI_MALE};
use flyff_rules::consts::{MAX_HUMAN_PARTS, MAX_LEGEND_LEVEL, OBJTYPE_FLYING, dst::*};
use flyff_rules::stats::{final_max, npc_max_fp_origin, npc_max_hp_origin};
use flyff_sim::anim::motion_table::ModelKey;
use flyff_sim::mover::{SimMover, SimMoverParams};
use flyff_sim::types::{MoverKind as SimKind, ObjState, StateFlags, defs::OT_MOVER};

/// `m_nCount` starts in `0..COUNT_START`.
const COUNT_START: u32 = 128;

/// The sim parameters of a prop-based mover (31 §5.4): kind, ids, flying flag, rank, speed (halved for
/// `AII_MONSTER`, `CreateObj.cpp:696-705`), frame factor, level, model key.
fn sim_params(prop: &MoverProp, kind: SimKind) -> SimMoverParams {
    let is_pet = prop.ai == AII_PET || prop.ai == AII_EGG;
    let kind = if is_pet && kind != SimKind::Player {
        SimKind::Pet
    } else {
        kind
    };
    let mut p = SimMoverParams::default_for(kind);
    p.mover_index = prop.id;
    p.ai = prop.ai;
    p.flying_npc = kind != SimKind::Player && prop.flying == 1;
    p.rank = prop.class;
    p.killable = prop.killable != 0;
    p.base_speed = if prop.ai == AII_MONSTER {
        prop.speed / 2.0_f32
    } else {
        prop.speed
    };
    p.frame_factor = prop.frame;
    p.level = prop.level.max(1);
    p.prop_flying = prop.flying;
    p.is_pet = is_pet;
    p.model_key = ModelKey {
        ot: OT_MOVER,
        index: prop.id,
    };
    p
}

/// The default scale of a mover model: `CObj::ResetScale()`, which the original calls when an object is created,
/// sets `m_vScale` to the `fScale` column of the model's `mdlDyna.inc` entry (`1.0` for a prop without an entry,
/// as `GetModelElem` returns `NULL` then and the scale stays at the constructor's 1). The real registry has 350
/// of its 781 mover models at a scale other than 1 (2.0 for 104 of them).
pub fn model_scale(gd: &GameData, prop_id: u32) -> f32 {
    gd.models
        .get(OT_MOVER, prop_id)
        .map(|e| e.scale)
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0)
}

/// The header and the sim of a prop-based mover standing at `pos` facing `angle` (the spawn API stamps the
/// world, layer and id). The scale is the registry scale of the model (`ResetScale()` at creation): the header
/// (what the wire and the client draw) and the sim (radius, `IsRangeObj`) carry the same value.
fn prop_parts(
    gd: &GameData,
    prop: &MoverProp,
    kind: SimKind,
    pos: Vec3,
    angle: f32,
) -> (ObjCore, SimMover) {
    let mut core = ObjCore::new(ObjId::NULL, ObjKind::Mover, prop.id, WorldId(0), LayerId(0));
    core.pos = pos;
    core.angle = flyff_sim::num::set_angle_quantized(angle);
    core.scale = model_scale(gd, prop.id);
    let mut params = sim_params(prop, kind);
    params.scale = core.scale;
    let sim = SimMover::new(ObjId::NULL, pos, angle, params);
    (core, sim)
}

/// The `(ObjCore, SimMover)` of a monster (or pet) built from `prop`, at the registry scale of its model.
pub fn monster_parts(
    gd: &GameData,
    prop: &MoverProp,
    pos: Vec3,
    angle: f32,
) -> (ObjCore, SimMover) {
    prop_parts(gd, prop, SimKind::Monster, pos, angle)
}

/// The `(ObjCore, SimMover)` of an NPC built from `prop`, at the registry scale of its model (a `.dyo` NPC then
/// takes the scale of its record).
pub fn npc_parts(gd: &GameData, prop: &MoverProp, pos: Vec3, angle: f32) -> (ObjCore, SimMover) {
    prop_parts(gd, prop, SimKind::Npc, pos, angle)
}

/// The initial `MoverCore` of a prop-based mover (`InitProp` + `Init` defaults, `Mover.cpp:230-568`,
/// `1475-1596`; 16 §3.5): stats and level from the prop, `adj = 0`, `chg = NULL`, `HP = max`, `MP = add_mp`
/// (**not** the maximum), `FP = max`, the virtual item `prop.atk1`, the flying type flag, the random count.
fn prop_core(prop: &MoverProp, rng: &mut dyn GameRng) -> MoverCore {
    let level = i32::try_from(prop.level).unwrap_or(i32::MAX).max(1);
    let base = BaseStats {
        str: i32::try_from(prop.str).unwrap_or(i32::MAX),
        sta: i32::try_from(prop.sta).unwrap_or(i32::MAX),
        dex: i32::try_from(prop.dex).unwrap_or(i32::MAX),
        int: i32::try_from(prop.int).unwrap_or(i32::MAX),
    };
    let mut core = MoverCore {
        level,
        job: -1,
        base,
        hp: 0,
        mp: i32::try_from(prop.add_mp).unwrap_or(i32::MAX),
        fp: 0,
        dst: Default::default(),
        equip_cache: EquipCache {
            // `UpdateParam()` of an NPC: the defense is the natural armor
            defense_min: i32::try_from(prop.natural_armor).unwrap_or(i32::MAX),
            defense_max: i32::try_from(prop.natural_armor).unwrap_or(i32::MAX),
            adj_hit_rate: 0,
            adj_parry: 0,
        },
        equip_info: [EquipInfo::EMPTY; MAX_HUMAN_PARTS],
        virt_item: prop.atk1,
        mode: 0,
        state_mode: 0,
        type_flags: if prop.flying == 1 { OBJTYPE_FLYING } else { 0 },
        count: rng.random_below(COUNT_START),
        disguise: None,
        rates: MonsterRates::default(),
    };
    core.hp = npc_max_hp(&core, prop);
    core.fp = npc_max_fp(&core);
    core
}

/// `GetMaxHitPoint()` of an NPC with an empty DST table.
fn npc_max_hp(core: &MoverCore, prop: &MoverProp) -> i32 {
    let origin = npc_max_hp_origin(prop.add_hp, 1.0, core.rates.hp);
    final_max(&core.dst.table, DST_HP_MAX, DST_HP_MAX_RATE, origin)
}

/// `GetMaxFatiguePoint()` of an NPC with an empty DST table.
fn npc_max_fp(core: &MoverCore) -> i32 {
    let origin = npc_max_fp_origin(core.level, core.base.str, core.base.sta, core.base.dex);
    final_max(&core.dst.table, DST_FP_MAX, DST_FP_MAX_RATE, origin)
}

impl Mover {
    /// A monster, guard, event monster or pet (`InitProp`, 16 §3.5): stats and level from `prop`,
    /// `hp = max_hp`, `mp = add_mp`, `fp = max_fp`, `job = -1`, `count = rng.below(128)`, the flying flag and
    /// the virtual item. The AI kind and the rank come from the prop; the AI itself is doc 18's.
    pub fn new_monster(
        core: ObjCore,
        sim: SimMover,
        prop: &MoverProp,
        rng: &mut dyn GameRng,
    ) -> Mover {
        let mv = prop_core(prop, rng);
        Mover {
            core,
            sim,
            mv,
            combat: CombatState::default(),
            buffs: BuffSet::default(),
            status: StatusEffects::default(),
            sync: Default::default(),
            kind: MoverKind::Monster(Box::new(MonsterExt {
                ai: AiSlot::new(prop.ai, AI_STATE_DEFAULT),
                owner: None,
                class: prop.class,
                belligerence: prop.belligerence,
                active_attack: false,
                gold: 0,
                summon: None,
            })),
        }
    }

    /// A `character.inc` NPC (`InitCharacter`, `Mover.cpp:1001-1060`): the figure, the key, the menus and the
    /// worn items (`equip_info[part]` with `part` from the item prop, only for humanoid NPCs). `def` is
    /// `None` for a mover without a character definition (a plain mover from a `.dyo`).
    pub fn new_npc(
        core: ObjCore,
        sim: SimMover,
        prop: &MoverProp,
        def: Option<&NpcDef>,
        gd: &GameData,
        rng: &mut dyn GameRng,
    ) -> Mover {
        let mut mv = prop_core(prop, rng);
        let mut ext = NpcExt {
            belligerence: prop.belligerence,
            ..NpcExt::default()
        };
        if let Some(d) = def {
            ext.character_key = d.key.clone();
            ext.menus = d.menus.clone().into_boxed_slice();
            if let Some(fig) = d.figure {
                ext.hair_mesh = fig.hair_mesh;
                ext.hair_color = fig.hair_color;
                ext.head_mesh = fig.head_mesh;
            }
            for &item in &d.equip {
                if let Some(part) = gd.item(item).map(|p| p.parts as usize)
                    && let Some(slot) = mv.equip_info.get_mut(part)
                {
                    *slot = EquipInfo {
                        item_id: item,
                        option: 0,
                        flag: 0,
                    };
                }
            }
        }
        Mover {
            core,
            sim,
            mv,
            combat: CombatState::default(),
            buffs: BuffSet::default(),
            status: StatusEffects::default(),
            sync: Default::default(),
            kind: MoverKind::Npc(Box::new(ext)),
        }
    }
}

/// What the load normalisation had to repair (16 §3.5 step 2, 13 §17.4).
#[derive(Clone, Debug, PartialEq)]
pub enum LoadRepair {
    /// `job` outside `0..32`: set to 0 (vagrant).
    JobOutOfRange {
        /// The stored job.
        found: u8,
    },
    /// `level` outside `1..=129`: clamped.
    LevelClamped {
        /// The stored level.
        found: u16,
        /// The level in use.
        to: i32,
    },
    /// `exp` outside `0..next_exp`: clamped to `next_exp - 1`.
    ExpClamped {
        /// The stored EXP.
        found: i64,
        /// The EXP in use.
        to: i64,
    },
    /// A pool below zero: set to 0.
    PoolNegative {
        /// `"hp"`, `"mp"` or `"fp"`.
        pool: &'static str,
        /// The stored value.
        found: i32,
    },
    /// `mover_index` is neither `MI_MALE` nor `MI_FEMALE`: derived from the sex.
    MoverIndex {
        /// The stored index.
        found: u32,
    },
    /// `scale` is not a positive finite number: set to 1.
    Scale,
    /// `gold` outside `0..=i32::MAX`: clamped.
    Gold {
        /// The stored gold.
        found: i64,
    },
    /// Items that found no slot after the container repair (never silently dropped: logged here and left to
    /// the mailbox fallback of the load).
    ItemsOrphaned {
        /// How many.
        count: usize,
    },
}

/// A freshly built player and the repairs applied to the load.
#[derive(Clone, Debug)]
pub struct NewPlayer {
    /// The player mover (not yet in a world).
    pub mover: Mover,
    /// What the normalisation changed (to be logged by the join).
    pub repairs: Vec<LoadRepair>,
}

/// `GetAttrOption()` of an item: `ability | element << 16 | element_level << 24`.
fn attr_option(it: &ItemRecord) -> i32 {
    it.upgrade_level | (i32::from(it.legacy_element()) << 16) | (it.element_level << 24)
}

/// The worn items of an inventory as `m_aEquipInfo` (slot `bag + part`; 16 §13.6 `redo_equip(send = false)`
/// replaces this derivation with the full equip pass).
fn equip_info_from_inventory(inv: &InventoryDoc) -> [EquipInfo; MAX_HUMAN_PARTS] {
    let c: &ContainerRecord = &inv.container;
    let bag = usize::from(InventoryDoc::bag_slots(c.capacity));
    std::array::from_fn(|part| {
        c.item_at_slot(bag + part)
            .map_or(EquipInfo::EMPTY, |it| EquipInfo {
                item_id: it.item_id,
                option: attr_option(it),
                flag: it.flags,
            })
    })
}

/// The `(ObjCore, SimMover)` of a loaded character (`row.world_id`, `row.layer`, `row.pos`, angle 0, the
/// persisted scale, fuel and action state). The caller may move it with the spawn API afterwards.
pub fn player_parts(load: &CharacterLoad, gd: &GameData) -> (ObjCore, SimMover) {
    let row = &load.row;
    let mover_index = player_mover_index(row.sex, row.mover_index).0;
    let pos = Vec3::new(row.pos[0], row.pos[1], row.pos[2]);
    let mut core = ObjCore::new(
        ObjId::NULL,
        ObjKind::Mover,
        mover_index,
        WorldId(row.world_id),
        LayerId(row.layer),
    );
    core.pos = pos;
    core.angle = 0.0;
    core.scale = sane_scale(row.scale).0;
    let mut params = SimMoverParams::default_for(SimKind::Player);
    params.mover_index = mover_index;
    params.model_key = ModelKey {
        ot: OT_MOVER,
        index: mover_index,
    };
    params.level = u32::from(row.level.max(1));
    params.mode = row.mode_flags;
    params.hp_positive = row.hp > 0;
    params.scale = core.scale;
    if let Some(p) = gd.mover(mover_index) {
        // `m_pActMover->m_fSpeed = prop.fSpeed` (MI_MALE / MI_FEMALE: 0.1)
        params.base_speed = p.speed;
        params.frame_factor = p.frame;
    }
    let mut sim = SimMover::new(ObjId::NULL, pos, 0.0, params);
    sim.fuel = row.fuel;
    sim.tm_acc_fuel = i32::try_from(row.fuel_time_acc).unwrap_or(i32::MAX);
    sim.act.state = ObjState(row.act_state);
    sim.act.flags = StateFlags(row.act_state_flags);
    sim.motion.id = row.motion;
    (core, sim)
}

/// `(mover_index, repaired)`: `MI_MALE` for sex 0, `MI_FEMALE` for sex 1 when the stored index is neither.
fn player_mover_index(sex: u8, stored: u32) -> (u32, bool) {
    if stored == MI_MALE || stored == MI_FEMALE {
        (stored, false)
    } else if sex == 0 {
        (MI_MALE, true)
    } else {
        (MI_FEMALE, true)
    }
}

/// `(scale, repaired)`: a non-finite or non-positive scale becomes 1.
fn sane_scale(s: f32) -> (f32, bool) {
    if s.is_finite() && s > 0.0 {
        (s, false)
    } else {
        (1.0, true)
    }
}

impl Mover {
    /// A player from the enter-world load (`Mover::new_player`, 16 §3.5): `mv`, `ident`, `Progression`,
    /// `Appearance` and `SmState` from the row and documents; the load normalisation of step 2 (`job` outside
    /// `0..32` becomes 0, `level` is clamped to `1..=129`, `exp` to `0..next_exp - 1`, the pools to `>= 0`; the
    /// maxima are enforced by the getters, KEEP) with one [`LoadRepair`] per change; the worn items as
    /// `equip_info`; the live buffs from the buff document; `regen` as of a join at `now`. The inventory,
    /// pockets and banks are carried unchanged for doc 19.
    pub fn new_player(
        load: &CharacterLoad,
        authority: Authority,
        gd: &GameData,
        now: GameMs,
        rng: &mut dyn GameRng,
    ) -> NewPlayer {
        let (core, sim) = player_parts(load, gd);
        let row = &load.row;
        let docs = &load.docs;
        let mut repairs = Vec::new();

        if player_mover_index(row.sex, row.mover_index).1 {
            repairs.push(LoadRepair::MoverIndex {
                found: row.mover_index,
            });
        }
        if sane_scale(row.scale).1 {
            repairs.push(LoadRepair::Scale);
        }
        let job = if usize::from(row.job) < flyff_rules::consts::MAX_JOB {
            i32::from(row.job)
        } else {
            repairs.push(LoadRepair::JobOutOfRange { found: row.job });
            0
        };
        let level = i32::from(row.level).clamp(1, MAX_LEGEND_LEVEL);
        if i32::from(row.level) != level {
            repairs.push(LoadRepair::LevelClamped {
                found: row.level,
                to: level,
            });
        }
        // `exp` lies in `0 .. next_exp - 1`: the EXP of the next level is row `level + 1` (16 §3.5)
        let mut exp = row.exp.max(0);
        if let Some(next) = usize::try_from(level + 1)
            .ok()
            .and_then(|i| gd.exp.character.get(i))
            .map(|r| r.exp1)
            .filter(|&n| n > 0)
        {
            exp = exp.min(next - 1);
        }
        if exp != row.exp {
            repairs.push(LoadRepair::ExpClamped {
                found: row.exp,
                to: exp,
            });
        }
        let mut pool = |name: &'static str, v: i32| {
            if v < 0 {
                repairs.push(LoadRepair::PoolNegative {
                    pool: name,
                    found: v,
                });
                0
            } else {
                v
            }
        };
        let (hp, mp, fp) = (pool("hp", row.hp), pool("mp", row.mp), pool("fp", row.fp));
        let gold = row.gold.clamp(0, i64::from(i32::MAX));
        if gold != row.gold {
            repairs.push(LoadRepair::Gold { found: row.gold });
        }

        let mut progression = Progression {
            exp,
            max_level: row.max_level.max(row.level),
            death_exp: row.death_exp,
            death_level: row.death_level,
            stat_points: row.stat_points,
            skill_points: row.skill_points,
            skill_level_total: row.skill_level_total,
            job_skills: Progression::job_skills_from_doc(&docs.skills),
            flight_exp: row.flight_exp,
            tutorial_state: row.tutorial_state,
            fame: row.fame,
            angel_exp: row.angel_exp,
            angel_level: row.angel_level,
        };
        progression.max_level = progression.max_level.max(u16::try_from(level).unwrap_or(1));

        let mut equip_info = equip_info_from_inventory(&docs.inventory);
        // fallback for loads without an inventory (fixtures): the precomputed preview of the character list
        if docs.inventory.container.capacity == 0 {
            for (slot, &id) in equip_info.iter_mut().zip(row.equipment_preview.iter()) {
                if id != 0 {
                    slot.item_id = id;
                }
            }
        }

        let mv = MoverCore {
            level,
            job,
            base: BaseStats {
                str: row.stat_str,
                sta: row.stat_sta,
                dex: row.stat_dex,
                int: row.stat_int,
            },
            hp,
            mp,
            fp,
            dst: Default::default(),
            equip_cache: EquipCache::default(),
            equip_info,
            virt_item: II_WEA_HAN_HAND,
            mode: row.mode_flags,
            state_mode: 0,
            type_flags: 0,
            count: rng.random_below(COUNT_START),
            disguise: None,
            rates: MonsterRates::default(),
        };

        let ident = PlayerIdent {
            character_id: load.identity.id,
            account_id: load.identity.account_id,
            slot: load.identity.slot,
            name: load.identity.name.clone(),
            authority,
        };
        let appearance = Appearance {
            sex: row.sex,
            skin_set: row.skin_set,
            hair_mesh: row.hair_mesh,
            hair_color: row.hair_color,
            head_mesh: row.head_mesh,
            hide_coat: Appearance::hide_coat_from_mask(row.hide_coat_mask),
        };
        let (items, orphans) =
            PlayerItems::from_load(load, i32::try_from(gold).unwrap_or(i32::MAX));
        if !orphans.is_empty() {
            tracing::error!(
                character = load.identity.id.0,
                items = orphans.len(),
                "load: items without a free slot after the container repair"
            );
            repairs.push(LoadRepair::ItemsOrphaned {
                count: orphans.len(),
            });
        }
        let social = PlayerSocial {
            honor: row.honor_title_id.map(i32::from),
            honor_doc: docs.honor.clone(),
            ..PlayerSocial::default()
        };
        let misc = PlayerMisc {
            return_world: WorldId(row.return_world_id),
            return_pos: Vec3::new(row.return_pos[0], row.return_pos[1], row.return_pos[2]),
            return_village: None,
            marking_world: WorldId(row.marking_world_id),
            marking_pos: Vec3::new(row.marking_pos[0], row.marking_pos[1], row.marking_pos[2]),
            mute_remaining_s: row.mute_remaining_s,
            messenger_state: row.messenger_state,
            play_time_s: row.play_time_s,
            attendance_bits: row.attendance_bits,
            attendance_minutes: row.attendance_minutes,
            attendance_elapsed: row.attendance_elapsed,
            coupon_count: row.coupon_count,
            rest_points: row.rest_points,
            last_logout_at: row.last_logout_at,
            settings: docs.settings.clone(),
            // the Event.lua ticks start at the join (the `RegenTimers::at_join(now)` value)
            coupon_tick: now,
            keep_connect_tick: now,
            rainbow_entrant: false,
            // the guild-house rest tick starts at the join, the gauge counts as unsent (doc 22 §12.7)
            rest_tick: now,
            rest_sent: 0,
        };
        let ext = PlayerExt {
            ident,
            appearance,
            progression,
            sm: SmState::from_doc(&docs.sm_timers),
            regen: RegenTimers::at_join(now),
            death_flags: DeathFlags::default(),
            pk: PkFields {
                value: row.pk_value,
                propensity: row.pk_propensity,
                pk_exp: row.pk_exp,
            },
            view: PlayerView::default(),
            region_attr: 0,
            old_region_attr: 0,
            ctrl_ready: None,
            items,
            econ: crate::econ::EconState::default(),
            skills_rt: SkillRuntime {
                action_points: docs.taskbar.action_points,
                ..SkillRuntime::default()
            },
            quests: QuestBook::load(docs.quests.clone(), &gd.quests, now),
            npc: crate::npc::PlayerNpc::default(),
            social,
            taskbar: docs.taskbar.clone(),
            chat: crate::chat::PlayerChat::default(),
            misc,
        };

        // the buffs of the document: effects are applied by the first process (17 §7.9); the unknown rows are
        // dropped and the common ones capped at 60 minutes here, the KQ-17 strip is the join's
        let (buffs, _buff_report) = crate::buff::persist::load(&docs.buffs, gd);

        NewPlayer {
            mover: Mover {
                core,
                sim,
                mv,
                combat: CombatState::default(),
                buffs,
                status: StatusEffects::default(),
                sync: Default::default(),
                kind: MoverKind::Player(Box::new(ext)),
            },
            repairs,
        }
    }
}
