//! The respawn system (Plan/15 §15, WP-WORLD-14; ref/06 §9.7, §12; F-088, F-097, KQ-14). Port of
//! `CWorld::ReadRespawn` (`src:_Common/WorldFile.cpp:555-783`), `CRespawnInfo::GenResPoint` / `GetPos` and
//! `CRespawner::Spawn` (`src:_Common/Respawn.cpp:94-161, 373-669`).
//!
//! * **Load** ([`RespawnTable::build`], [`Worlds::init_respawn`]): the raw `.rgn` lines of a world folder
//!   (`WorldStatic.respawn_defs`) become one [`RespawnProto`] per accepted line, in file order: the
//!   `__INCREASED_SPAWN` count (cap 80), the `short` cast of the time, the x0.5 / x0.7 factors, the fixed
//!   positions, the 100 precomputed [`RespawnProto::points`] (rejecting the unwalkable cells) and the halved
//!   base time. `gm_index` is the index among the accepted lines. The protos are shared by every layer.
//! * **State** ([`Respawner`], one per [`Layer`](super::Layer)): `alive`, `active_alive`, the countdown and the
//!   `half` flag of every proto. A fresh layer starts at zero, so its first respawn tick fills it.
//! * **Tick** ([`spawn_tick`], the P1 job [`process`], tick `n % 15 == 3`): per world ascending, per layer
//!   ascending (invalidated layers keep respawning until they are released), per proto in `gm_index` order:
//!   the day/hour window of the cached in-game clock pauses the countdown, `countdown -= 1`, and below zero the
//!   line rolls its next interval `base * r / 100` with `r` in `[50, 149]` (**FIX** PL-150: `i32`, the original
//!   wrapped a `short`; [`plan_refill`]), refills the missing objects (alternating "up to the maximum" and "up to
//!   half") and creates them at one of the 100 points.
//! * **Counters** ([`Respawner::on_destroyed`], reached from `Worlds::destroy` through
//!   `Worlds::respawn_on_destroy`): `alive -= 1`, and `active_alive -= 1` for an aggressive monster.
//!
//! Random draws of one created object, in this order: ctrl `random_below(360)` (the angle) or item
//! `random_below(item_max - item_min + 1)` (the count); then the position: `random_below(100)` (the point;
//! none for a fixed position), flyers `random_below(21)`; a monster's own construction draw
//! (`Mover::new_monster`, `m_nCount`) comes last. Per due line: `random_range(50, 150)` first.
//!
//! **Decisions** (15 §15.6): Q1 `i32` countdown (FIX, PL-150); Q2 the Madrigal "rate" branch for `gm_index == 0`
//! is kept and has no effect (both rates are 1.0); Q3 the Madrigal aggressive cap uses `rates.monster_aggressive`
//! (KEEP-QUIRK KQ-14, 20 % of the max count for **every** Madrigal line); Q4 the full/half alternation is kept;
//! the `RANK_GUARD` skip (non-KOR rule) is evaluated once at load from `events.pk` (KEEP); script and back-end
//! entries are dropped (Q6). Deviations: a spawn that the world refuses (outside the world, no layer) does not
//! count as alive (the original counted it); a line whose mover/item prop does not exist is dropped at load (the
//! original dereferenced a null prop); patrol index and cycle are loaded but not applied (all `.pat` files are
//! empty, 18 §9.13).
//!
//! **Giants** (owner decision of 2026-10-29, bugs #005 / #016; it deviates from the original data, which has
//! `cnt 4` / `cnt 2` giant lines, passive Madrigal giants and a free-running timer): a mover line whose prop is
//! `RANK_BOSS` ([`is_giant`], the "Giant X" monsters) has at most one monster per line and layer, whatever
//! `gameplay.increased_spawn` is ([`GIANT_MAX_COUNT`], [`effective_mover_count`]; a count-0 line stays 0). Its
//! monster is aggressive regardless of its level and of the aggro column ([`RespawnProto::giant`],
//! [`aggressive_cap`]), except in Madrigal with `rates.monster_aggressive = 0`, the operator's off switch that
//! silences Madrigal ([`giant_hostile`]). A destroy of the giant restarts the countdown of its line at `base_time`
//! with `half = false` ([`Respawner::restart`], `Worlds::respawn_on_destroy`), so the giant returns on the
//! `(base_time + 1)`-th respawn pass after its destroy and never within seconds. Normal lines are unchanged: the
//! KQ-14 share of red names, the level-7 gate, the count-1 / aggro-1 rule and the full / half alternation.
//!
//! `events.respawn == 0` stops everything. The DPS-meter marker of doc 17 §10 is the [`RespawnHooks`] seam.

use super::Worlds;
use super::catalog::WorldStatic;
use super::ctrl::Ctrl;
use super::item_drop::ItemDrop;
use super::object::{ObjKind, RespawnOrigin};
use super::terrain::HeightAttr;
use crate::ctx::{GameCtx, Services};
use crate::item::GroundItemMeta;
use crate::item::container::SerialSource;
use crate::mover::{Mover, monster_parts};
use crate::rng::RngSet;
use flyff_common::config::{EventId, RulesConfig};
use flyff_common::defs::attribute::{RANK_BOSS, RANK_GUARD, RANK_MATERIAL};
use flyff_common::defs::itemkind::CK1_DOOR;
use flyff_common::defs::obj::{
    MI_AIBATT1, MI_AIBATT2, MI_AIBATT3, MI_BURUDENG1, MI_BURUDENG2, MI_BURUDENG3, MI_MUSHPANG1,
    MI_MUSHPANG2, MI_MUSHPANG3,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::rng::{GameRng, named_stream};
use flyff_common::{GameMs, LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::props::mover::MoverProp;
use flyff_data::worlds::{CtrlElemDef, RespawnDef};
use flyff_db::ItemRecord;
use flyff_formats::objects::{OT_CTRL, OT_ITEM, OT_MOVER};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Cap of a mover line's maximum count after `__INCREASED_SPAWN` (`if (nMobSpawn > 80) nMobSpawn = 80`).
pub const MAX_SPAWN_COUNT: i32 = 80;
/// `MAX_RESPOINT_PER_REGION`: the precomputed spawn points of a line.
pub const RESPAWN_POINTS: usize = 100;
/// `MAX_FAIL_TO_RESPAWN`: rejected candidate points after which a line is dropped.
pub const MAX_FAIL_TO_RESPAWN: u32 = 10_000;
/// `prj.m_fMonsterRebirthRate`: 1.0 (the original value came from the dropped DB back-end).
pub const MONSTER_REBIRTH_RATE: f32 = 1.0;
/// `prj.m_fMonsterRespawnRate`: 1.0.
pub const MONSTER_RESPAWN_RATE: f32 = 1.0;
/// `m_dwAiState` values (`STAND`, `PATROL`) that pin a single-monster line to its file position and angle.
const AI_STATE_STAND: u32 = 8;
const AI_STATE_PATROL: u32 = 9;
/// Giants per respawn line and layer (owner rule, bugs #005 / #016): one at a time.
pub const GIANT_MAX_COUNT: i32 = 1;
/// Lowest monster level that can be aggressive (`prop.level >= 7`, `Respawn.cpp:641`, 15 §15.4). Giants ignore it.
pub const AGGRESSIVE_MIN_LEVEL: u32 = 7;

/// A giant: a mover with the `RANK_BOSS` class (the "Giant X" monsters; `RANK_SUPER` and `RANK_MIDBOSS` are not).
pub fn is_giant(prop: &MoverProp) -> bool {
    prop.class == RANK_BOSS
}

/// The count of a mover line after `__INCREASED_SPAWN` (`long` truncation of `count * increased_spawn`, cap 80).
pub fn scaled_mover_count(file_count: i32, increased_spawn: f32) -> i32 {
    let n = (f64::from(file_count) * f64::from(increased_spawn)) as i64;
    n.min(i64::from(MAX_SPAWN_COUNT))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The population a mover line refills to (`m_nMaxcb`): a giant line has [`GIANT_MAX_COUNT`] monster when the file
/// has any (`increased_spawn` does not touch it; a count-0 line stays 0), every other mover line the
/// [`scaled_mover_count`]. The admin map uses the same rule for its "xN" labels.
pub fn effective_mover_count(file_count: i32, prop: &MoverProp, increased_spawn: f32) -> i32 {
    if is_giant(prop) {
        if file_count > 0 { GIANT_MAX_COUNT } else { 0 }
    } else {
        scaled_mover_count(file_count, increased_spawn)
    }
}

/// Whether the giants of `world` are hostile at the aggressive rate `rate` (`rates.monster_aggressive`): always,
/// except that a rate of 0 silences Madrigal, giants included (the operator's switch; the quiet test servers and
/// the load mixes rely on it).
pub fn giant_hostile(world: WorldId, rate: f32) -> bool {
    world.0 != WI_WORLD_MADRIGAL || rate > 0.0
}

/// The line can make aggressive monsters now: the cap is not zero, and the level is at least
/// [`AGGRESSIVE_MIN_LEVEL`] or the line is a giant line. One rule for the spawn and for the admin spawn table.
pub fn line_is_aggressive(
    p: &RespawnProto,
    st: &RespawnState,
    world: WorldId,
    rate: f32,
    level: u32,
) -> bool {
    aggressive_cap(p, st, world, rate) > 0 && (p.giant || level >= AGGRESSIVE_MIN_LEVEL)
}

/// What the load of the respawn lines reads from the configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RespawnConfig {
    /// `gameplay.increased_spawn` (`__INCREASED_SPAWN`): multiplies a mover line's count.
    pub increased_spawn: f32,
    /// `events.pk` at load: without it the `RANK_GUARD` lines are skipped (non-KOR rule, F-097).
    pub pk_server: bool,
}

impl Default for RespawnConfig {
    fn default() -> Self {
        RespawnConfig {
            increased_spawn: 1.0,
            pk_server: false,
        }
    }
}

impl RespawnConfig {
    /// The configuration of a starting game (`pk_server` is the startup state of `events.pk`).
    pub fn from_rules(rules: &RulesConfig, pk_server: bool) -> RespawnConfig {
        RespawnConfig {
            increased_spawn: rules.gameplay.increased_spawn,
            pk_server,
        }
    }
}

/// One accepted respawn line (`CRespawnInfo` after `Add`, 15 §15.1).
#[derive(Clone, Debug, PartialEq)]
pub struct RespawnProto {
    /// The index among the accepted lines of the world (`m_nGMIndex`).
    pub gm_index: u16,
    /// What the line creates.
    pub kind: ObjKind,
    /// The mover / ctrl / item prop id.
    pub index: u32,
    /// Seconds after the x0.5 / x0.7 factor and the halving (`m_uTime`).
    pub base_time: u16,
    /// `m_nMaxcb` (movers: after `increased_spawn`, capped at 80; giant lines: 1).
    pub max_count: i32,
    /// A mover line whose prop is `RANK_BOSS` (a giant; owner rule, bugs #005 / #016): one monster at a time,
    /// always aggressive, a wait of `base_time` after its destroy.
    pub giant: bool,
    /// `m_nMaxcb == 1 && (ai_state == STAND || PATROL)` before the giant rule (file version 4 or later): the
    /// line has exactly one monster, at `vpos`, facing `angle`.
    pub fixed: bool,
    /// `m_nMaxAttackNum`: the file's "aggro" column.
    pub max_attack: i32,
    /// The spawn rectangle `[left, top, right, bottom]` in world units.
    pub rect: [i32; 4],
    /// `m_vPos`: the file position for flyers and for fixed single-monster lines, else the origin.
    pub vpos: Vec3,
    /// `m_fY`: the file's y in every case.
    pub y_base: f32,
    /// Spawn window in game days.
    pub day: (i32, i32),
    /// Spawn window in game hours.
    pub hour: (i32, i32),
    /// Item stack size range (item lines).
    pub item_count: (i32, i32),
    /// `m_dwAiState`.
    pub ai_state: u32,
    /// `m_fAngle` (used for fixed single-monster lines).
    pub angle: f32,
    /// `m_dwPatrolIndex` (`None` = the file's -1).
    pub patrol: Option<u32>,
    /// `m_bPatrolCycle`.
    pub patrol_cycle: bool,
    /// The ctrl block of a ctrl line.
    pub ctrl: Option<Box<CtrlElemDef>>,
    /// The mover prop has `flying != 0`.
    pub flying: bool,
    /// The 100 precomputed spawn points `(x, z)` (`m_aResPoint`).
    pub points: Box<[(i32, i32); RESPAWN_POINTS]>,
}

impl RespawnProto {
    /// The line has exactly one monster, at `vpos`, facing `angle` ([`RespawnProto::fixed`]).
    pub fn is_fixed(&self) -> bool {
        self.fixed
    }
}

/// What the load of one world folder's respawn lines did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RespawnLoadReport {
    /// Lines in the `.rgn`.
    pub lines: u32,
    /// Lines accepted.
    pub accepted: u32,
    /// `RANK_GUARD` lines skipped because `events.pk` is off.
    pub skipped_guard: u32,
    /// `RANK_MATERIAL` lines skipped.
    pub skipped_material: u32,
    /// Lines whose mover or item prop does not exist (dropped; the original crashed).
    pub unknown_prop: u32,
    /// Lines of an object type the server never creates.
    pub unknown_kind: u32,
    /// Lines dropped by `GenResPoint` (more than 10 000 rejected points).
    pub no_points: u32,
}

impl RespawnLoadReport {
    fn add(&mut self, o: &RespawnLoadReport) {
        self.lines += o.lines;
        self.accepted += o.accepted;
        self.skipped_guard += o.skipped_guard;
        self.skipped_material += o.skipped_material;
        self.unknown_prop += o.unknown_prop;
        self.unknown_kind += o.unknown_kind;
        self.no_points += o.no_points;
    }
}

/// The accepted respawn lines of a world folder, shared by every world id of the folder and every layer.
#[derive(Debug, Default, PartialEq)]
pub struct RespawnTable {
    /// The lines in `gm_index` order.
    pub protos: Vec<RespawnProto>,
    /// What the load did.
    pub report: RespawnLoadReport,
}

/// `(time as f64 * f) as u16`: IEEE double multiply, truncation, saturating at the `u16` range.
fn scale_time(time: u16, f: f64) -> u16 {
    (f64::from(time) * f) as u16
}

/// The x0.5 mobs of `ReadRespawn`.
fn is_fast_respawn_mover(id: u32) -> bool {
    matches!(
        id,
        MI_AIBATT1
            | MI_AIBATT2
            | MI_AIBATT3
            | MI_MUSHPANG1
            | MI_MUSHPANG2
            | MI_MUSHPANG3
            | MI_BURUDENG1
            | MI_BURUDENG2
            | MI_BURUDENG3
    )
}

impl RespawnTable {
    /// Builds the table of a folder from its raw lines (15 §15.1-§15.2). `rng` draws the spawn points; a world
    /// loaded twice with the same seed gets the same points.
    pub fn build(
        statics: &WorldStatic,
        data: &GameData,
        cfg: &RespawnConfig,
        rng: &mut dyn GameRng,
    ) -> RespawnTable {
        let mut protos: Vec<RespawnProto> = Vec::with_capacity(statics.respawn_defs.len());
        let mut report = RespawnLoadReport::default();
        for def in &statics.respawn_defs {
            report.lines += 1;
            match build_proto(def, statics, data, cfg, rng, protos.len()) {
                Ok(p) => {
                    report.accepted += 1;
                    protos.push(p);
                }
                Err(Reject::Guard) => report.skipped_guard += 1,
                Err(Reject::Material) => report.skipped_material += 1,
                Err(Reject::UnknownProp) => report.unknown_prop += 1,
                Err(Reject::UnknownKind) => report.unknown_kind += 1,
                Err(Reject::NoPoints) => report.no_points += 1,
            }
        }
        RespawnTable { protos, report }
    }

    /// Number of accepted lines.
    pub fn len(&self) -> usize {
        self.protos.len()
    }

    /// `true` without any accepted line.
    pub fn is_empty(&self) -> bool {
        self.protos.is_empty()
    }
}

/// Why a line did not become a proto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reject {
    Guard,
    Material,
    UnknownProp,
    UnknownKind,
    NoPoints,
}

fn build_proto(
    d: &RespawnDef,
    statics: &WorldStatic,
    data: &GameData,
    cfg: &RespawnConfig,
    rng: &mut dyn GameRng,
    accepted_before: usize,
) -> Result<RespawnProto, Reject> {
    let kind = match d.obj_type {
        OT_MOVER => ObjKind::Mover,
        OT_CTRL => ObjKind::Ctrl,
        OT_ITEM => ObjKind::Item,
        _ => return Err(Reject::UnknownKind),
    };
    if d.index == 0 {
        return Err(Reject::UnknownProp);
    }
    // step 1/2: the time reads through `short` (65535 stays 65535); movers with a prop get the factors, and their
    // maximum count (long truncation, cap 80 for movers only; a giant line has one monster). `pre_count` is the
    // count that decides the fixed position: before the giant rule (a giant ignores `increased_spawn`, so it is
    // the file count), so that a `cnt 2` STAND giant line keeps its 100 random points
    let mut time = d.time_s as i16 as u16;
    let mut flying = false;
    let mut giant = false;
    let mut pre_count = d.max_count;
    let mut max_count = d.max_count;
    match kind {
        ObjKind::Mover => {
            let prop = data.mover(d.index).ok_or(Reject::UnknownProp)?;
            if prop.class == RANK_GUARD && !cfg.pk_server {
                return Err(Reject::Guard);
            }
            if prop.class == RANK_MATERIAL {
                return Err(Reject::Material);
            }
            if prop.class != RANK_BOSS {
                time = if is_fast_respawn_mover(prop.id) {
                    scale_time(time, 0.5)
                } else {
                    scale_time(time, 0.7)
                };
            }
            flying = prop.flying != 0;
            giant = is_giant(prop);
            max_count = effective_mover_count(d.max_count, prop, cfg.increased_spawn);
            if !giant {
                pre_count = max_count;
            }
        }
        ObjKind::Item => {
            data.item(d.index).ok_or(Reject::UnknownProp)?;
        }
        ObjKind::Ctrl => {}
    }
    // step 3: fixed position
    let file_pos = Vec3::new(d.pos[0], d.pos[1], d.pos[2]);
    let fixed =
        d.version >= 4 && pre_count == 1 && matches!(d.ai_state, AI_STATE_STAND | AI_STATE_PATROL);
    let vpos = if flying || fixed {
        file_pos
    } else {
        Vec3::ZERO
    };
    let mut proto = RespawnProto {
        // step 6 (the index is the count of the lines accepted so far)
        gm_index: u16::try_from(accepted_before).unwrap_or(u16::MAX),
        kind,
        index: d.index,
        base_time: time / 2,
        max_count,
        giant,
        fixed,
        max_attack: d.max_attack_num,
        rect: d.rect,
        vpos,
        y_base: d.pos[1],
        day: (clamp_i32(d.day.0), clamp_i32(d.day.1)),
        hour: (clamp_i32(d.hour.0), clamp_i32(d.hour.1)),
        item_count: (clamp_i32(d.item.0), clamp_i32(d.item.1)),
        ai_state: d.ai_state,
        angle: d.angle,
        patrol: (d.patrol_index != u32::MAX).then_some(d.patrol_index),
        patrol_cycle: d.patrol_cycle,
        ctrl: d.ctrl.clone(),
        flying,
        points: Box::new([(0, 0); RESPAWN_POINTS]),
    };
    // step 5: the 100 spawn points
    gen_points(&mut proto, statics, rng).ok_or(Reject::NoPoints)?;
    Ok(proto)
}

fn clamp_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// `CRespawnInfo::GenResPoint` (`__RES0807`, 15 §15.2): 100 random integer points inside the rectangle whose
/// height attribute is neither outside, `NOMOVE` nor (`NOWALK` on the ground / `NOFLY` for flyers). More than
/// [`MAX_FAIL_TO_RESPAWN`] rejections: `None` (the line is dropped). A fixed single-monster line probes its
/// file position (no draws).
fn gen_points(p: &mut RespawnProto, statics: &WorldStatic, rng: &mut dyn GameRng) -> Option<()> {
    let check = if p.flying {
        HeightAttr::NoFly
    } else {
        HeightAttr::NoWalk
    };
    let mut fails = 0_u32;
    let mut n = 0;
    while n < RESPAWN_POINTS {
        let (x, z) = if p.is_fixed() {
            (p.vpos.x, p.vpos.z)
        } else {
            let w = (p.rect[2] - p.rect[0]).max(1) as u32;
            let h = (p.rect[3] - p.rect[1]).max(1) as u32;
            (
                p.rect[0].wrapping_add(rng.random_below(w) as i32) as f32,
                p.rect[1].wrapping_add(rng.random_below(h) as i32) as f32,
            )
        };
        let a = statics.height_attr(x, z);
        if a == HeightAttr::Outside || a == HeightAttr::NoMove || a == check {
            fails += 1;
            if fails > MAX_FAIL_TO_RESPAWN {
                return None;
            }
            continue;
        }
        if let Some(slot) = p.points.get_mut(n) {
            *slot = (x as i32, z as i32);
        }
        n += 1;
    }
    Some(())
}

/// The state of one proto in one layer (`m_cb`, `m_nActiveAttackNum`, `m_cbTime`, `m_bHalf`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RespawnState {
    /// Objects of the line that exist (`m_cb`, counted up at spawn and down at destroy).
    pub alive: i32,
    /// Aggressive monsters counted against the cap (`m_nActiveAttackNum`).
    pub active_alive: i32,
    /// Seconds until the next refill check (`m_cbTime`); below zero fires the check.
    pub countdown: i32,
    /// The next check refills only up to half of the maximum (`m_bHalf`).
    pub half: bool,
}

/// The respawn state of one layer, parallel to [`RespawnTable::protos`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Respawner {
    /// One state per proto.
    pub states: Vec<RespawnState>,
}

impl Respawner {
    /// A fresh respawner for `n` lines: every counter zero, the first tick spawns the full population.
    pub fn fresh(n: usize) -> Respawner {
        Respawner {
            states: vec![RespawnState::default(); n],
        }
    }

    /// `CRespawnInfo::Increment`: an object of line `origin` was destroyed (15 §15.5).
    pub fn on_destroyed(&mut self, origin: RespawnOrigin, aggressive: bool) {
        if let Some(s) = self.states.get_mut(usize::from(origin.gm_index)) {
            s.alive = s.alive.saturating_sub(1);
            if aggressive {
                s.active_alive = s.active_alive.saturating_sub(1);
            }
        }
    }

    /// The owner's giant rule (bugs #005 / #016): the next check of line `gm_index` comes `base_time` seconds
    /// after now, as a full check. `alive` and `active_alive` stay as they are. Resetting `half` is required: a
    /// half check after the wait would compute `max / 2 - (max - 1) = 0` for a one-monster line and spawn nothing.
    pub fn restart(&mut self, gm_index: u16, base_time: u16) {
        if let Some(s) = self.states.get_mut(usize::from(gm_index)) {
            s.countdown = i32::from(base_time);
            s.half = false;
        }
    }

    /// Total alive objects of all lines.
    pub fn alive_total(&self) -> i64 {
        self.states.iter().map(|s| i64::from(s.alive)).sum()
    }
}

/// The inclusive game-calendar window of a line (`min < max`: a plain range; otherwise the wrap test
/// `!(v < min && v > max)`, which never excludes anything when `min == max`).
pub fn in_window(v: i32, (lo, hi): (i32, i32)) -> bool {
    if lo < hi {
        lo <= v && v <= hi
    } else {
        !(v < lo && v > hi)
    }
}

/// Seams of the spawn tick that other documents fill in.
pub trait RespawnHooks: Send {
    /// `combat::dps::mark_if_tracked` (doc 17 §10, `__DPS_METER`): mark a freshly created monster as tracked
    /// when `GameData.dps_monsters` lists its prop or its class is `RANK_BOSS` / `RANK_SUPER`. A no-op until
    /// WP-COMBAT-14.
    fn mark_if_tracked(&mut self, _m: &mut Mover) {}
}

/// The default hooks: nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRespawnHooks;

impl RespawnHooks for NoRespawnHooks {}

/// The holder of the installed [`RespawnHooks`] (`Worlds` is `Debug`, trait objects are not).
pub(crate) struct RespawnHookSlot(pub Option<Box<dyn RespawnHooks>>);

impl Default for RespawnHookSlot {
    fn default() -> Self {
        RespawnHookSlot(Some(Box::new(NoRespawnHooks)))
    }
}

impl std::fmt::Debug for RespawnHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RespawnHookSlot(..)")
    }
}

/// What the spawn tick reads besides the worlds.
#[derive(Clone, Copy, Debug)]
pub struct SpawnEnv<'a> {
    /// The loaded game data (movers, items, ctrls).
    pub data: &'a GameData,
    /// The game clock of the tick (`drop_time` of respawned items).
    pub now: GameMs,
    /// The in-game day `1..=30` (the cached once-per-second sample, 15 §17.1).
    pub day: i32,
    /// The in-game hour `1..=24`.
    pub hour: i32,
    /// `events.respawn != 0`.
    pub enabled: bool,
    /// `rates.monster_aggressive` (Madrigal's aggressive cap, KQ-14).
    pub aggressive_rate: f32,
}

/// What one spawn tick created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnReport {
    /// Lines whose countdown fired.
    pub checks: u32,
    /// Monsters created.
    pub movers: u32,
    /// Ctrls created.
    pub ctrls: u32,
    /// Items created.
    pub items: u32,
    /// Objects the world refused (outside the world, layer gone, ids exhausted).
    pub refused: u32,
}

impl Worlds {
    /// Builds the respawn tables of every loaded world from the raw `.rgn` lines (15 §3.5 step 3, §15.1) and
    /// gives every existing layer a fresh [`Respawner`]. World ids that share a folder share one table, whose
    /// points come from the stream `respawn:<folder>` of `seed` (PL-013). Returns the summed load report.
    pub fn init_respawn(
        &mut self,
        data: &GameData,
        cfg: &RespawnConfig,
        seed: u64,
    ) -> RespawnLoadReport {
        let mut total = RespawnLoadReport::default();
        let mut cache: BTreeMap<String, Arc<RespawnTable>> = BTreeMap::new();
        let ids: Vec<WorldId> = self.world_ids().collect();
        for id in ids {
            let Some(w) = self.worlds.get_mut(&id) else {
                continue;
            };
            let folder = w.statics.folder.clone();
            let table = match cache.get(&folder) {
                Some(t) => Arc::clone(t),
                None => {
                    let mut rng = named_stream(seed, &format!("respawn:{folder}"));
                    let t = Arc::new(RespawnTable::build(&w.statics, data, cfg, &mut rng));
                    total.add(&t.report);
                    cache.insert(folder, Arc::clone(&t));
                    t
                }
            };
            for l in w.layers.values_mut() {
                l.respawner = Respawner::fresh(table.len());
            }
            w.respawn = table;
        }
        total
    }

    /// Installs the respawn hooks (doc 17's DPS marker).
    pub fn set_respawn_hooks(&mut self, h: Box<dyn RespawnHooks>) {
        self.respawn_hooks.0 = Some(h);
    }

    /// An object of line `origin` was destroyed in `(world, layer)` (step 5 of `Worlds::destroy`): the layer's
    /// counters ([`Respawner::on_destroyed`]) and, for a giant line, the restart of the line's countdown at its
    /// `base_time` ([`Respawner::restart`]). Every destroy of a giant counts: a kill (at the removal of the
    /// corpse), a GM or admin despawn, the release of a layer. Each layer has its own respawner, so an instance or
    /// a ticket layer waits on its own.
    pub(crate) fn respawn_on_destroy(
        &mut self,
        world: WorldId,
        layer: LayerId,
        origin: RespawnOrigin,
        aggressive: bool,
    ) {
        // copied out of the table before the mutable borrow of the layer
        let giant_wait = self
            .world(world)
            .and_then(|w| w.respawn.protos.get(usize::from(origin.gm_index)))
            .filter(|p| p.giant)
            .map(|p| p.base_time);
        if let Some(l) = self.layer_mut(world, layer) {
            l.respawner.on_destroyed(origin, aggressive);
            if let Some(base_time) = giant_wait {
                l.respawner.restart(origin.gm_index, base_time);
            }
        }
    }
}

/// The P1 job `world::respawn::process` (10 §5.3, tick `n % 15 == 3`): [`spawn_tick`] over the worlds with the
/// cached in-game time, `events.respawn` and the game's serial source.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let env = SpawnEnv {
        data: ctx.data,
        now: ctx.clock.game_ms,
        day: i32::from(ctx.worlds.game_time.day),
        hour: i32::from(ctx.worlds.game_time.hour),
        enabled: ctx.live.event_state(EventId::Respawn) != 0,
        aggressive_rate: ctx.live.rates.monster_aggressive,
    };
    let mut hooks: Box<dyn RespawnHooks> = ctx
        .worlds
        .respawn_hooks
        .0
        .take()
        .unwrap_or_else(|| Box::new(NoRespawnHooks));
    let mut serials = crate::item::ops::CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
    let r = spawn_tick(ctx.worlds, &env, ctx.rng, &mut serials, &mut *hooks);
    ctx.worlds.respawn_hooks.0 = Some(hooks);
    if r.refused > 0 {
        tracing::warn!(refused = r.refused, "respawn: objects refused by the world");
    }
}

/// What a due check decided (the arithmetic of `CRespawner::Spawn`, 15 §15.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refill {
    /// The countdown to the next check, in seconds (`m_cbTime`).
    pub countdown: i32,
    /// How many objects to create now (may be negative after the half rule: nothing is created then).
    pub dead: i32,
    /// The `half` flag after this check.
    pub half: bool,
}

/// The countdown of a due line: `base * roll / 100` with `roll` in `[50, 149]`. **FIX** (PL-150, 15 §15.6 Q1):
/// computed in `i32`; the original stored it in a `short`, so `base 32 767` lines wrapped negative for
/// `roll >= 101` and respawned at the next tick.
pub fn next_countdown(base_time: u16, roll: u32) -> i32 {
    i32::from(base_time).saturating_mul(roll as i32) / 100
}

/// The decision of one due check of `proto` in `world` (15 §15.3): the next countdown (the Madrigal "rate"
/// branch for `gm_index == 0`, Q2, with both rates 1.0), the number of objects missing and the alternation of
/// the refill: a check with `half` set refills only up to half of the maximum.
pub fn plan_refill(proto: &RespawnProto, st: &RespawnState, world: WorldId, roll: u32) -> Refill {
    let t = next_countdown(proto.base_time, roll);
    let (countdown, mut dead) = if proto.gm_index == 0 && world.0 == WI_WORLD_MADRIGAL {
        let f = proto.max_count as f32 * MONSTER_RESPAWN_RATE - st.alive as f32;
        (
            (t as f32 * MONSTER_REBIRTH_RATE) as i32,
            if f > 0.0 { f as i32 } else { 0 },
        )
    } else {
        (t, (proto.max_count - st.alive).max(0))
    };
    if st.half {
        dead = proto.max_count / 2 - (proto.max_count - dead);
    }
    Refill {
        countdown,
        dead,
        half: !st.half,
    }
}

/// One respawn tick over every world and layer (`CRespawner::Spawn` per layer, 15 §15.3). The caller supplies
/// the clock sample, the random streams (`rng.world(w)` per world) and the serial source of respawned items
/// (a source without serials gives the item serial 0: it is numbered at pickup).
pub fn spawn_tick(
    worlds: &mut Worlds,
    env: &SpawnEnv<'_>,
    rng: &mut RngSet,
    serials: &mut dyn SerialSource,
    hooks: &mut dyn RespawnHooks,
) -> SpawnReport {
    let mut report = SpawnReport::default();
    if !env.enabled {
        return report;
    }
    let world_ids: Vec<WorldId> = worlds.world_ids().collect();
    for wid in world_ids {
        let Some(w) = worlds.world(wid) else { continue };
        let table = Arc::clone(&w.respawn);
        if table.is_empty() {
            continue;
        }
        let statics = Arc::clone(&w.statics);
        let layer_ids: Vec<LayerId> = w.layers.keys().copied().collect();
        for lid in layer_ids {
            if let Some(l) = worlds.layer_mut(wid, lid)
                && l.respawner.states.len() != table.len()
            {
                l.respawner
                    .states
                    .resize(table.len(), RespawnState::default());
            }
            for (i, proto) in table.protos.iter().enumerate() {
                if proto.index == 0
                    || !in_window(env.day, proto.day)
                    || !in_window(env.hour, proto.hour)
                {
                    continue;
                }
                let Some(st) = worlds
                    .layer_mut(wid, lid)
                    .and_then(|l| l.respawner.states.get_mut(i))
                else {
                    break;
                };
                st.countdown = st.countdown.saturating_sub(1);
                if st.countdown >= 0 {
                    continue;
                }
                report.checks += 1;
                let r = rng.world(wid);
                let plan = plan_refill(proto, st, wid, r.random_range(50, 150));
                st.countdown = plan.countdown;
                st.half = plan.half;
                for _ in 0..plan.dead.max(0) {
                    let mut cx = SpawnCx {
                        env,
                        statics: &statics,
                        world: wid,
                        layer: lid,
                        proto,
                        index: i,
                        rng: &mut *r,
                        serials: &mut *serials,
                        hooks: &mut *hooks,
                    };
                    spawn_one(worlds, &mut cx, &mut report);
                }
            }
        }
    }
    report
}

/// What `spawn_one` shares.
pub(crate) struct SpawnCx<'a> {
    pub(crate) env: &'a SpawnEnv<'a>,
    pub(crate) statics: &'a WorldStatic,
    pub(crate) world: WorldId,
    pub(crate) layer: LayerId,
    pub(crate) proto: &'a RespawnProto,
    pub(crate) index: usize,
    pub(crate) rng: &'a mut dyn GameRng,
    pub(crate) serials: &'a mut dyn SerialSource,
    pub(crate) hooks: &'a mut dyn RespawnHooks,
}

/// The position of a new object (15 §15.3 step 3).
fn pick_position(cx: &mut SpawnCx<'_>) -> Vec3 {
    let p = cx.proto;
    let (x, z, y0) = if p.is_fixed() {
        (p.vpos.x, p.vpos.z, p.vpos.y)
    } else {
        let r = cx.rng.random_below(RESPAWN_POINTS as u32) as usize;
        let (px, pz) = p.points.get(r).copied().unwrap_or((0, 0));
        (px as f32, pz as f32, p.vpos.y)
    };
    if p.flying {
        let mut y = y0 + cx.rng.random_below(21) as f32;
        let land = cx.statics.land_height(x, z);
        if y < land {
            y = land + 10.0;
        }
        Vec3::new(x, y, z)
    } else {
        let y = cx.statics.under_height(Vec3::new(x, p.y_base + 1.0, z));
        Vec3::new(x, y, z)
    }
}

/// Creates one object of the line (15 §15.3 `spawn_one`) and counts it.
pub(crate) fn spawn_one(worlds: &mut Worlds, cx: &mut SpawnCx<'_>, report: &mut SpawnReport) {
    let origin = RespawnOrigin {
        gm_index: cx.proto.gm_index,
    };
    let result = match cx.proto.kind {
        ObjKind::Mover => spawn_mover(worlds, cx, origin),
        ObjKind::Ctrl => spawn_ctrl(worlds, cx, origin),
        ObjKind::Item => spawn_item(worlds, cx),
    };
    match result {
        Some(counted_aggressive) => {
            if let Some(st) = worlds
                .layer_mut(cx.world, cx.layer)
                .and_then(|l| l.respawner.states.get_mut(cx.index))
            {
                st.alive += 1;
                if counted_aggressive {
                    st.active_alive += 1;
                }
            }
            match cx.proto.kind {
                ObjKind::Mover => report.movers += 1,
                ObjKind::Ctrl => report.ctrls += 1,
                ObjKind::Item => report.items += 1,
            }
        }
        None => report.refused += 1,
    }
}

/// A monster: `Some(counted against the aggressive cap)` on success.
fn spawn_mover(worlds: &mut Worlds, cx: &mut SpawnCx<'_>, origin: RespawnOrigin) -> Option<bool> {
    let p = cx.proto;
    let prop = cx.env.data.mover(p.index)?;
    let pos = pick_position(cx);
    let angle = if p.is_fixed() { p.angle } else { 0.0 };
    let (core, sim) = monster_parts(cx.env.data, prop, pos, angle);
    let mut m = Mover::new_monster(core, sim, prop, &mut *cx.rng);
    m.core.respawn = Some(origin);
    // `SetGold(level * 15)`; the state and the DPS marker
    let level = m.mv.level;
    if let Some(ext) = m.monster_mut() {
        ext.gold = level.wrapping_mul(15); // C `int` arithmetic
        ext.ai.state = p.ai_state;
    }
    cx.hooks.mark_if_tracked(&mut m);
    // the aggressive cap (15 §15.4, KQ-14)
    let st = worlds
        .layer(cx.world, cx.layer)
        .and_then(|l| l.respawner.states.get(cx.index))
        .copied()
        .unwrap_or_default();
    let cap = aggressive_cap(p, &st, cx.world, cx.env.aggressive_rate);
    let counted = st.active_alive < cap;
    // a giant is aggressive at any level (owner rule); no random draw is involved
    if counted
        && (p.giant || prop.level >= AGGRESSIVE_MIN_LEVEL)
        && let Some(ext) = m.monster_mut()
    {
        ext.active_attack = true;
    }
    worlds
        .spawn_mover(cx.world, cx.layer, m)
        .ok()
        .map(|_| counted)
}

/// The cap of aggressive monsters of a line (15 §15.4). A giant line is hostile whatever its aggro column says
/// ([`giant_hostile`]: only a rate of 0 in Madrigal silences it).
pub fn aggressive_cap(p: &RespawnProto, st: &RespawnState, world: WorldId, rate: f32) -> i32 {
    if p.giant {
        if giant_hostile(world, rate) {
            st.active_alive.saturating_add(1)
        } else {
            0
        }
    } else if p.max_attack == 1 && p.max_count == 1 {
        st.active_alive.saturating_add(1)
    } else if world.0 == WI_WORLD_MADRIGAL {
        ((p.max_count as f32 * MONSTER_RESPAWN_RATE) * rate) as i32
    } else {
        p.max_attack
    }
}

/// A ctrl: `Some(false)` on success.
fn spawn_ctrl(worlds: &mut Worlds, cx: &mut SpawnCx<'_>, origin: RespawnOrigin) -> Option<bool> {
    let p = cx.proto;
    let angle = cx.rng.random_below(360) as f32;
    let pos = pick_position(cx);
    let elem = p.ctrl.as_deref().cloned().unwrap_or_else(|| CtrlElemDef {
        min_item_num: 1,
        max_item_num: 1,
        ..CtrlElemDef::default()
    });
    let door = cx
        .env
        .data
        .ctrls
        .get(p.index)
        .is_some_and(|c| c.ctrl_kind1 == CK1_DOOR);
    let mut c = Ctrl::new(p.index, pos, angle, elem, door);
    c.core.respawn = Some(origin);
    worlds.spawn_ctrl(cx.world, cx.layer, c).ok().map(|_| false)
}

/// A ground item: `Some(false)` on success.
fn spawn_item(worlds: &mut Worlds, cx: &mut SpawnCx<'_>) -> Option<bool> {
    let p = cx.proto;
    let prop = cx.env.data.item(p.index)?;
    let span = (p.item_count.1 - p.item_count.0 + 1).max(0) as u32;
    let n = p
        .item_count
        .0
        .saturating_add(cx.rng.random_below(span) as i32);
    let n = n.clamp(1, i32::try_from(prop.pack_max.max(1)).unwrap_or(i32::MAX));
    let pos = pick_position(cx);
    let mut record = ItemRecord::new(0, p.index, u16::try_from(n).unwrap_or(u16::MAX), 0);
    record.endurance = i32::try_from(prop.endurance).unwrap_or(i32::MAX);
    // a fresh serial when the source has one; otherwise the pickup numbers the item (19 §2.7)
    record.serial = cx.serials.next_serial().unwrap_or(0);
    let meta = GroundItemMeta {
        owner: None,
        holder: None,
        drop_time: cx.env.now,
        from_monster: false,
        event_monster: None,
        gold: None,
        respawned: p.gm_index > 0,
    };
    let mut d = ItemDrop::new(record, pos, 0.0, meta);
    d.core.respawn = Some(RespawnOrigin {
        gm_index: p.gm_index,
    });
    worlds.spawn_item(cx.world, cx.layer, d).ok().map(|_| false)
}

/// The alive objects of a line in a layer (tests and the corpus checks).
pub fn alive_of(worlds: &Worlds, world: WorldId, layer: LayerId, gm_index: u16) -> i32 {
    worlds
        .layer(world, layer)
        .and_then(|l| l.respawner.states.get(usize::from(gm_index)))
        .map_or(0, |s| s.alive)
}

/// The ids of the objects a layer holds that came from respawn lines (members and pending adds).
pub fn respawned_objects(worlds: &Worlds, world: WorldId, layer: LayerId) -> Vec<ObjId> {
    let Some(l) = worlds.layer(world, layer) else {
        return Vec::new();
    };
    l.members
        .iter()
        .copied()
        .chain(l.queues.add.iter().copied())
        .filter(|&o| worlds.core(o).is_some_and(|c| c.respawn.is_some()))
        .collect()
}
