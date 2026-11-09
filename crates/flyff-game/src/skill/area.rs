//! Area resolution of a skill (Plan/17 §6.7; WP-SKILL-06; `src:_Common/Ctrl.cpp:225-780, 1415-1678`).
//!
//! [`do_apply_skill_ex`] is `CCtrl::DoApplySkillEx`: it sorts the targets of a skill by its `spell_region`
//! and leaves the effect on each of them to [`apply_skill`]:
//!
//! | `spell_region` | resolution |
//! |---|---|
//! | `SRO_AROUND` / `SRO_REGION`, not a troupe skill | [`apply_skill_around`]: the movers (and ctrls) around the target; `REGION` includes the centre, `AROUND` does not |
//! | the same with `EXT_TROUPEWITH` | a player rolls the add-skill row's probability, then [`apply_skill_around_troupe`]; nobody affected: the caster itself |
//! | the same with `EXT_TROUPE` | [`apply_skill_around_troupe`]; nobody affected: the skill fails (0) |
//! | the same with `continuous_pain` | additionally a [ground DoT](super::ctrl) at the target's position |
//! | `SRO_LINE` | [`apply_skill_line`]: a 2.5 x 2.5 m box, `skill_range` deep, in front of the caster |
//! | `SRO_DIRECT` and the rest | the target alone (the Psychic Wall is a wall, not an application) |
//!
//! **The candidates** are the objects of the legacy link-map window (`FOR_LINKMAP`, [`skill_region_n_range`]:
//! 4 / 8 / 16 / 32 link units for a radius up to 4 / 8 / 16 / more), players first (`linkPlayer`), then the
//! other objects (`linkDynamic`), in the order of the grid ([`Grid::for_each_legacy`](crate::world::grid::Grid)).
//! The list is taken once; every candidate is then looked at in turn, so an application that changes the state
//! of the world (a death, the attacker turning pink) is seen by the next one like in the original loop.
//!
//! **The filters** of a player candidate (a player caster against a player): `!control` and the hit type is `Pk`
//! (an AoE never starts a PK), or both sides in a solo duel but the candidate is not the caster's duel target. A
//! monster or ctrl candidate must be a non-peaceful mover (a guard is skipped unless the caster is chaotic) or,
//! for a mover caster, a ctrl. The caster itself is never a candidate. The range test is `IsRangeObj` of
//! `flyff_sim::bounds` (`R = 0.8 r_self + 0.8 r_other + range`, strictly below; `range == 0` counts as 1).
//!
//! The per-target flags follow the original exactly: a player candidate is the primary target
//! (`is_target`) when it is the centre; every monster or ctrl candidate is applied with the default
//! `bTarget = TRUE` (`ApplySkill`'s default argument), also in the line; the line applies players with
//! `is_target = false` and ignores `only_dmg` for monsters.

use crate::combat::arbiter::damage_env;
use crate::combat::hit_type::hit_type;
use crate::combat::state::{is_chaotic, is_die, is_live, is_peaceful};
use crate::ctx::{GameCtx, Services};
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::skill::apply::{Applied, ApplyArgs, apply_skill};
use crate::skill::ctrl;
use crate::stats::env::prop_of;
use crate::world::grid::{LinkSet, skill_region_n_range};
use crate::world::object::ObjRef;
use flyff_common::ObjId;
use flyff_common::defs::attribute::{RANK_GUARD, SRO_AROUND, SRO_LINE, SRO_REGION};
use flyff_common::defs::define::{EXT_MAGICATKSHOT, EXT_TROUPE, EXT_TROUPEWITH};
use flyff_common::defs::skill::{
    SI_ACR_SUP_DARKILLUSION, SI_JST_SUP_ABSORB, SI_JST_SUP_BLEEDING, SI_JST_SUP_POISON,
    SI_PSY_PSY_PSYCHICWALL,
};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};
use flyff_proto::pb;
use flyff_rules::combat::{HitType, x_random};
use flyff_rules::consts::NULL_ID;
use flyff_sim::bounds::{is_range_obj, is_range_point, radius};
use flyff_sim::num::{cosf, sinf, to_radian};

// ---- the kinds of target ---------------------------------------------------------------------------------------------

/// `nApplyType`: which kinds of objects an area skill reaches (`OBJTYPE_PLAYER | OBJTYPE_MONSTER | OBJTYPE_CTRL`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyType(u8);

impl ApplyType {
    /// Nothing.
    pub const NONE: ApplyType = ApplyType(0);
    /// `OBJTYPE_PLAYER`.
    pub const PLAYER: ApplyType = ApplyType(1);
    /// `OBJTYPE_MONSTER`.
    pub const MONSTER: ApplyType = ApplyType(2);
    /// `OBJTYPE_CTRL`.
    pub const CTRL: ApplyType = ApplyType(4);

    /// `true` when any bit of `other` is set (the original's `nApplyType & (A | B)`).
    pub const fn has(self, other: ApplyType) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for ApplyType {
    type Output = ApplyType;
    fn bitor(self, rhs: ApplyType) -> ApplyType {
        ApplyType(self.0 | rhs.0)
    }
}

/// `CCtrl::TargetSelecter(target)` (`Ctrl.cpp:1415-1445`, `src` is the caster): a mover target reaches players
/// and monsters; a ctrl target reaches players, ctrls and monsters when a player casts, nothing when another
/// mover casts, players only when a ctrl casts.
pub fn target_selecter(ctx: &GameCtx<'_>, src: ObjId, target: ObjId) -> ApplyType {
    let store = ctx.worlds.store();
    if matches!(store.lookup(target), Some(ObjRef::Mover(_))) {
        return ApplyType::PLAYER | ApplyType::MONSTER;
    }
    match store.lookup(src) {
        Some(ObjRef::Mover(_)) if store.is_player(src) => {
            ApplyType::PLAYER | ApplyType::CTRL | ApplyType::MONSTER
        }
        Some(ObjRef::Ctrl(_)) => ApplyType::PLAYER,
        _ => ApplyType::NONE,
    }
}

// ---- IsRangeObj over the objects of the store ------------------------------------------------------------------

/// `GetRadius()` of a mover: the model box times the scale ([`flyff_sim::bounds::radius`]).
pub fn radius_of(m: &Mover) -> f32 {
    radius(&m.sim.params.bounds, m.sim.params.scale)
}

/// The position and `GetRadius()` of an object that has a model: a mover, or a ctrl whose model box is known
/// (`IsRangeObj` is `FALSE` without a model). A ground item has none.
fn body(ctx: &GameCtx<'_>, id: ObjId) -> Option<(Vec3, f32)> {
    let store = ctx.worlds.store();
    match store.lookup(id)? {
        ObjRef::Mover(_) => store.mover(id).map(|m| (m.core.pos, radius_of(m))),
        ObjRef::Ctrl(_) => {
            let c = store.ctrl(id)?;
            let model = ctx.worlds.ctrl_models().models.get(&c.core.index)?;
            Some((c.core.pos, radius(&model.aabb, c.core.scale)))
        }
        ObjRef::Item(_) => None,
    }
}

/// `this->IsRangeObj(other, range)`.
pub fn in_range_obj(ctx: &GameCtx<'_>, this: ObjId, other: ObjId, range: f32) -> bool {
    match (body(ctx, this), body(ctx, other)) {
        (Some((tp, tr)), Some((op, or))) => is_range_obj(tp, tr, op, or, range),
        _ => false,
    }
}

/// `this->IsRangeObj(point, range)`.
pub fn in_range_point(ctx: &GameCtx<'_>, this: ObjId, point: Vec3, range: f32) -> bool {
    body(ctx, this).is_some_and(|(tp, tr)| is_range_point(tp, tr, point, range))
}

/// The radius of the skill: the level row's `skill_range`, or the skill row's for an item
/// (`(float)dwSkillRange`, a missing value is the huge float of `NULL_ID`).
pub fn skill_range(a: &Applied<'_>) -> f32 {
    match &a.add {
        Some(add) => add.skill_range as f32,
        None => a.prop.skill_range as f32,
    }
}

// ---- the sweep over the link map -------------------------------------------------------------------------------

/// How the range test of a candidate is made.
#[derive(Clone, Copy, Debug)]
enum Reach {
    /// `cand.IsRangeObj(centre, range)`: the centre is an object (`ApplySkillAround`).
    Object(ObjId),
    /// `cand.IsRangeObj(point, range)` (`ApplySkillRegion`).
    Point(Vec3),
}

/// The common part of `ApplySkillAround` and `ApplySkillRegion`.
#[derive(Clone, Copy, Debug)]
struct Sweep {
    /// The caster (`pSrc`).
    src: ObjId,
    /// The world and layer the link map is read in.
    world: WorldId,
    layer: LayerId,
    /// The centre of the window.
    pos: Vec3,
    /// `nRange` of `FOR_LINKMAP` (link units, 4 / 8 / 16 / 32).
    n_range: i32,
    /// `fRange` in metres.
    range: f32,
    /// Which kinds of object are reached.
    mask: ApplyType,
    /// The range test.
    reach: Reach,
    /// The primary target of a player candidate (`bTarget = (centre == candidate)`).
    centre: Option<ObjId>,
    /// The mover spared because `fTargetRatio == 0` (the centre of an `AROUND` skill).
    spare: Option<ObjId>,
    /// `bControl`.
    control: bool,
}

/// The ids of one link set of the window, in link-map order.
fn candidates(ctx: &GameCtx<'_>, s: &Sweep, set: LinkSet) -> Vec<ObjId> {
    let mut out = Vec::new();
    if let Some(l) = ctx.worlds.layer(s.world, s.layer) {
        l.grid
            .for_each_legacy(s.pos, s.n_range, set, |id| out.push(id));
    }
    out
}

/// The candidate passes the range test of the sweep.
fn reaches(ctx: &GameCtx<'_>, s: &Sweep, cand: ObjId) -> bool {
    match s.reach {
        Reach::Object(c) => in_range_obj(ctx, cand, c, s.range),
        Reach::Point(p) => in_range_point(ctx, cand, p, s.range),
    }
}

/// `IsValidObj(m) && m->IsLive()`.
fn live(ctx: &GameCtx<'_>, id: ObjId) -> bool {
    ctx.worlds
        .mover(id)
        .is_some_and(|m| !m.core.deleted && is_live(m))
}

/// The player filter of the sweeps (a player candidate): `None` skips it, `Some(is_target)` applies the skill.
///
/// A player caster against a player candidate: `!control` and `GetHitType(..) == HITTYPE_PK` skips it (an AoE never
/// starts a PK), and two players in a solo duel only hit each other (`m_nDuel == 1` on both sides and the type is
/// not `Pvp`). Everything else is applied.
fn player_candidate(
    svc: &Services,
    ctx: &GameCtx<'_>,
    src: ObjId,
    centre: Option<ObjId>,
    cand: ObjId,
    control: bool,
) -> Option<bool> {
    let def = ctx.worlds.mover(cand)?;
    let is_target = centre == Some(cand);
    if let Some(att) = ctx.worlds.mover(src)
        && att.is_player()
        && def.is_player()
    {
        let env = damage_env(svc, ctx);
        let ty = hit_type(
            att,
            def,
            &env,
            &*ctx.worlds.movement.combat.hooks,
            is_target,
            0,
        );
        if !control && ty == HitType::Pk {
            return None;
        }
        if att.combat.duel.duel == 1 && def.combat.duel.duel == 1 && ty != HitType::Pvp {
            return None;
        }
    }
    Some(is_target)
}

/// The monster / ctrl filter: a non-peaceful mover (a guard is spared by a non-chaotic player), or any ctrl when a
/// mover casts.
fn other_candidate(svc: &Services, ctx: &GameCtx<'_>, src: ObjId, cand: ObjId) -> bool {
    match ctx.worlds.store().lookup(cand) {
        Some(ObjRef::Mover(_)) => {
            let Some(t) = ctx.worlds.mover(cand) else {
                return false;
            };
            if is_peaceful(t) {
                return false;
            }
            let core = svc.stats.core();
            let guard = prop_of(t, core).class == RANK_GUARD;
            let spared = ctx
                .worlds
                .mover(src)
                .is_some_and(|a| a.is_player() && !is_chaotic(a));
            !(guard && spared)
        }
        Some(ObjRef::Ctrl(_)) => ctx.worlds.mover(src).is_some(),
        _ => false,
    }
}

/// The ctrl branch of `CCtrl::ApplySkill`: nothing is damaged or buffed, but the application still rolls the
/// skill's probability (a draw unless it is `NULL_ID`) and an `AROUND` / `REGION` skill that is not a shot shows its
/// `sfx_obj[1]` on the ctrl.
fn apply_to_ctrl(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    ctrl: ObjId,
    a: &Applied<'_>,
    args: &ApplyArgs,
) {
    if args.only_dmg {
        return;
    }
    let p = &*a.prop;
    let prob = if args.ignore_prob {
        NULL_ID
    } else {
        match &a.add {
            Some(add) => add.probability,
            None => p.probability as u32,
        }
    };
    let world = ctx.worlds.core(ctrl).map(|c| c.world);
    let passes = prob == NULL_ID
        || world.is_some_and(|w| (x_random(ctx.rng.world(w), 100) as i32) < prob as i32);
    if passes
        && matches!(p.spell_region, SRO_AROUND | SRO_REGION)
        && p.exe_target != EXT_MAGICATKSHOT
        && p.sfx_obj[1] != NULL_ID
    {
        let mut fx = Fx::new();
        fx.out.near(pb::CreateSfxObj {
            sfx_id: p.sfx_obj[1],
            x: 0.0,
            y: 0.0,
            z: 0.0,
            flag: false,
        });
        fx.flush_for(svc, ctx, ctrl);
    }
}

/// Walks the window of `s` and applies the skill to the candidates (see the module documentation).
fn sweep(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    s: &Sweep,
    a: &Applied<'_>,
    ignore_prob: bool,
    only_dmg: bool,
) {
    if s.mask.has(ApplyType::PLAYER) {
        for cand in candidates(ctx, s, LinkSet::Players) {
            if !reaches(ctx, s, cand) || s.spare == Some(cand) || cand == s.src {
                continue;
            }
            let Some(is_target) = player_candidate(svc, ctx, s.src, s.centre, cand, s.control)
            else {
                continue;
            };
            if !live(ctx, cand) {
                continue;
            }
            let args = ApplyArgs {
                src: s.src,
                ignore_prob,
                param: 0,
                only_dmg,
                is_target,
            };
            apply_skill(svc, ctx, cand, a, &args);
        }
    }
    if s.mask.has(ApplyType::MONSTER | ApplyType::CTRL) {
        for cand in candidates(ctx, s, LinkSet::Others) {
            if !other_candidate(svc, ctx, s.src, cand)
                || !reaches(ctx, s, cand)
                || s.spare == Some(cand)
                || cand == s.src
            {
                continue;
            }
            // `ApplySkill(pSrc, prop, add, ignore, 0, only_dmg)`: the default `bTarget = TRUE`
            let args = ApplyArgs {
                src: s.src,
                ignore_prob,
                param: 0,
                only_dmg,
                is_target: true,
            };
            if ctx.worlds.mover(cand).is_some() {
                if live(ctx, cand) {
                    apply_skill(svc, ctx, cand, a, &args);
                }
            } else {
                apply_to_ctrl(svc, ctx, cand, a, &args);
            }
        }
    }
}

/// `CCtrl::ApplySkillAround(pSrc, apply, ..., range_custom, control)` on `centre` (`Ctrl.cpp:416-572`): the skill
/// is applied around the centre object. `range_custom != 0` replaces the skill's own range (the link window is
/// still chosen by the skill's range).
#[allow(clippy::too_many_arguments)]
pub fn apply_skill_around(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    centre: ObjId,
    src: ObjId,
    mask: ApplyType,
    a: &Applied<'_>,
    ignore_prob: bool,
    only_dmg: bool,
    range_custom: f32,
    control: bool,
) {
    let Some(c) = ctx.worlds.core(centre) else {
        return;
    };
    let (world, layer, pos) = (c.world, c.layer, c.pos);
    let skill = skill_range(a);
    let n_range = skill_region_n_range(skill);
    let range = if range_custom != 0.0 {
        range_custom
    } else {
        skill
    };
    let ratio_zero = a.prop.spell_region != SRO_REGION;
    let s = Sweep {
        src,
        world,
        layer,
        pos,
        n_range,
        range,
        mask,
        reach: Reach::Object(centre),
        centre: Some(centre),
        spare: ratio_zero.then_some(centre),
        control,
    };
    sweep(svc, ctx, &s, a, ignore_prob, only_dmg);
}

/// `CCtrl::ApplySkillRegion(pos, apply, ..., centre, control)` of the caster `src` (`Ctrl.cpp:225-405`): the
/// skill is applied around a point (the ground DoT); `centre` is the primary target of the players.
#[allow(clippy::too_many_arguments)]
pub fn apply_skill_region(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    pos: Vec3,
    mask: ApplyType,
    a: &Applied<'_>,
    ignore_prob: bool,
    only_dmg: bool,
    centre: Option<ObjId>,
    control: bool,
) {
    let Some(c) = ctx.worlds.core(src) else {
        return;
    };
    let (world, layer) = (c.world, c.layer);
    let range = skill_range(a);
    let s = Sweep {
        src,
        world,
        layer,
        pos,
        n_range: skill_region_n_range(range),
        range,
        mask,
        reach: Reach::Point(pos),
        centre,
        spare: None,
        control,
    };
    sweep(svc, ctx, &s, a, ignore_prob, only_dmg);
}

// ---- the line ------------------------------------------------------------------------------------------------------

/// The depth and the width / height of the line (`ApplySkillLine(.., 2.5, 2.5, skill_range)`).
pub const LINE_WIDTH: f32 = 2.5;

/// `::IsTouchAABB(min1, max1, min2, max2)`: the boxes overlap (touching counts).
fn touches(min1: Vec3, max1: Vec3, min2: Vec3, max2: Vec3) -> bool {
    !(max1.x < min2.x
        || min1.x > max2.x
        || max1.y < min2.y
        || min1.y > max2.y
        || max1.z < min2.z
        || min1.z > max2.z)
}

/// The position of `target` in the space of a caster at `origin` facing `angle` degrees (the inverse of
/// `rotY(-angle) * translate(origin)`; D3D row vectors, so `rotY(+angle)` after the translation).
fn to_caster_space(origin: Vec3, angle: f32, target: Vec3) -> Vec3 {
    let a = to_radian(angle);
    let (s, c) = (sinf(a), cosf(a));
    let d = target - origin;
    Vec3::new(d.x * c + d.z * s, d.y, -d.x * s + d.z * c)
}

/// `CCtrl::ApplySkillLine(apply, w, h, depth, ignore, control)` of the caster `src` (`Ctrl.cpp:580-740`): the box
/// `x in [-w/2, w/2]`, `y in [-h/2, h/2]`, `z in [-depth, 0]` of the caster's space meets the model box of each
/// candidate (translated to its position, neither scaled nor turned). Players are applied with
/// `is_target = false`, monsters and ctrls with the defaults.
#[allow(clippy::too_many_arguments)]
pub fn apply_skill_line(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    mask: ApplyType,
    a: &Applied<'_>,
    width: f32,
    height: f32,
    depth: f32,
    ignore_prob: bool,
    control: bool,
) {
    let Some(caster) = ctx.worlds.mover(src) else {
        return;
    };
    let (world, layer, origin, angle) = (
        caster.core.world,
        caster.core.layer,
        caster.core.pos,
        caster.core.angle,
    );
    let min1 = Vec3::new(-width / 2.0, -height / 2.0, -depth);
    let max1 = Vec3::new(width / 2.0, height / 2.0, 0.0);
    let n_range = match depth {
        d if d <= 4.0 => 4,
        d if d <= 8.0 => 8,
        d if d <= 16.0 => 16,
        _ => 32,
    };
    let s = Sweep {
        src,
        world,
        layer,
        pos: origin,
        n_range,
        range: depth,
        mask,
        reach: Reach::Point(origin),
        centre: None,
        spare: None,
        control,
    };
    let in_box = |ctx: &GameCtx<'_>, id: ObjId| -> bool {
        let Some(t) = ctx.worlds.mover(id) else {
            return false;
        };
        let local = to_caster_space(origin, angle, t.core.pos);
        let b = &t.sim.params.bounds;
        touches(min1, max1, local + b.min, local + b.max)
    };
    if mask.has(ApplyType::PLAYER) {
        for cand in candidates(ctx, &s, LinkSet::Players) {
            if cand == src {
                continue;
            }
            // the line has no target: `bTarget` stays FALSE for the hit-type test and for the application
            let Some(is_target) = player_candidate(svc, ctx, src, None, cand, control) else {
                continue;
            };
            if !live(ctx, cand) || !in_box(ctx, cand) {
                continue;
            }
            let args = ApplyArgs {
                src,
                ignore_prob,
                param: 0,
                only_dmg: false,
                is_target,
            };
            apply_skill(svc, ctx, cand, a, &args);
        }
    }
    if mask.has(ApplyType::MONSTER | ApplyType::CTRL) {
        for cand in candidates(ctx, &s, LinkSet::Others) {
            if !other_candidate(svc, ctx, src, cand) {
                continue;
            }
            let args = ApplyArgs {
                src,
                ignore_prob,
                param: 0,
                only_dmg: false,
                is_target: true,
            };
            if ctx.worlds.mover(cand).is_some() {
                if live(ctx, cand) && in_box(ctx, cand) {
                    apply_skill(svc, ctx, cand, a, &args);
                }
            } else {
                apply_to_ctrl(svc, ctx, cand, a, &args);
            }
        }
    }
}

// ---- the troupe ----------------------------------------------------------------------------------------------------

/// `CCtrl::ApplySkillAroundTroupe(party, prop, add, ignore)` of the player `src` (`Ctrl.cpp:742-781`): every
/// member of the caster's party in the caster's world and layer, alive, strictly closer than the level row's
/// `skill_range`, takes the skill (default arguments: not `only_dmg`, primary target). The caster is a member
/// like the rest only for a `REGION` skill. Returns `true` when at least one member took it.
///
/// The members are the players of the link window around the caster that carry the caster's party id (every
/// member in range is in the window); the order is the grid's, not the party list's.
pub fn apply_skill_around_troupe(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    a: &Applied<'_>,
    ignore_prob: bool,
) -> bool {
    let Some(add) = a.add.as_deref() else {
        return false;
    };
    let Some(caster) = ctx.worlds.player(src) else {
        return false;
    };
    let Some(party) = caster.player().and_then(|p| p.social.party) else {
        return false;
    };
    let (world, layer, pos) = (caster.core.world, caster.core.layer, caster.core.pos);
    let max = add.skill_range as f32;
    let max_sq = max * max;
    let ratio_zero = a.prop.spell_region != SRO_REGION;
    let s = Sweep {
        src,
        world,
        layer,
        pos,
        n_range: skill_region_n_range(max),
        range: max,
        mask: ApplyType::PLAYER,
        reach: Reach::Point(pos),
        centre: None,
        spare: None,
        control: false,
    };
    let mut any = false;
    for member in candidates(ctx, &s, LinkSet::Players) {
        let Some(m) = ctx.worlds.mover(member) else {
            continue;
        };
        if m.core.deleted
            || is_die(m)
            || m.player().and_then(|p| p.social.party) != Some(party)
            || (member == src && ratio_zero)
        {
            continue;
        }
        let d = m.core.pos - pos;
        if d.x * d.x + d.y * d.y + d.z * d.z < max_sq {
            let args = ApplyArgs {
                ignore_prob,
                ..ApplyArgs::by(src)
            };
            apply_skill(svc, ctx, member, a, &args);
            any = true;
        }
    }
    any
}

// ---- DoApplySkillEx ------------------------------------------------------------------------------------------------

/// `CCtrl::DoApplySkillEx(target, prop, add, ignore, param, only_dmg, control)` (`Ctrl.cpp:1488-1678`): resolves
/// the targets by the skill's region (see the module documentation), then, for a mover caster, removes its Dark
/// Illusion (any other skill) and the Jester buffs that exclude the one just cast, and rolls the secondary
/// skill of the skill row. `1` is the application, `0` the failure of a troupe skill without a party or of a
/// Psychic Wall that found no free slot.
#[allow(clippy::too_many_arguments)]
pub fn do_apply_skill_ex(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    target: ObjId,
    a: &Applied<'_>,
    ignore_prob: bool,
    param: u32,
    only_dmg: bool,
    control: bool,
) -> i32 {
    let p = &*a.prop;
    let mut result = 1;
    match p.spell_region {
        SRO_AROUND | SRO_REGION => {
            let troupe = p.exe_target == EXT_TROUPE;
            let troupe_with = p.exe_target == EXT_TROUPEWITH;
            if !troupe && !troupe_with {
                let mask = target_selecter(ctx, src, target);
                apply_skill_around(
                    svc,
                    ctx,
                    target,
                    src,
                    mask,
                    a,
                    ignore_prob,
                    only_dmg,
                    0.0,
                    control,
                );
            } else if troupe_with {
                // a player rolls the row's probability, the troupe takes it without rolling again; nobody
                // there: the caster itself
                if ctx.worlds.player(src).is_some()
                    && let Some(add) = a.add.as_deref()
                {
                    let world = ctx.worlds.core(src).map(|c| c.world);
                    let hit = world.is_some_and(|w| {
                        (x_random(ctx.rng.world(w), 100) as i32) < add.probability as i32
                    });
                    if hit && !apply_skill_around_troupe(svc, ctx, src, a, true) {
                        apply_skill(
                            svc,
                            ctx,
                            src,
                            a,
                            &ApplyArgs {
                                ignore_prob: true,
                                ..ApplyArgs::by(src)
                            },
                        );
                    }
                }
            } else if ctx.worlds.player(src).is_some()
                && !apply_skill_around_troupe(svc, ctx, src, a, ignore_prob)
            {
                return 0;
            }
            // a ground DoT, unless the target is a player in its invulnerability window (a just ended duel)
            if p.continuous_pain != NULL_ID {
                let blocked = ctx
                    .worlds
                    .mover(target)
                    .is_some_and(|t| t.is_player() && t.combat.dead_ticks > 0);
                if !blocked {
                    ctrl::spawn_dot(svc, ctx, src, target, a, control);
                }
            }
        }
        SRO_LINE => {
            let mask = target_selecter(ctx, src, target);
            let depth = a.add.as_deref().map_or(0.0, |add| add.skill_range as f32);
            apply_skill_line(
                svc,
                ctx,
                src,
                mask,
                a,
                LINE_WIDTH,
                LINE_WIDTH,
                depth,
                ignore_prob,
                control,
            );
        }
        _ => {
            // the direct hit: the Psychic Wall is no application; `bOnlyDmg` is not passed on
            if p.id != SI_PSY_PSY_PSYCHICWALL {
                let args = ApplyArgs {
                    src,
                    ignore_prob,
                    param,
                    only_dmg: false,
                    is_target: true,
                };
                apply_skill(svc, ctx, target, a, &args);
            }
        }
    }
    if p.id == SI_PSY_PSY_PSYCHICWALL && !ctrl::spawn_wall(svc, ctx, src, a, control) {
        result = 0;
    }
    caster_bookkeeping(svc, ctx, src, a);
    // the secondary skill (level 1): `rate == NULL` or a roll `<= rate` (sic)
    if p.active_skill != NULL_ID {
        let fires = p.active_skill_rate == NULL_ID
            || ctx
                .worlds
                .core(target)
                .is_some_and(|c| x_random(ctx.rng.world(c.world), 100) <= p.active_skill_rate);
        if fires {
            super::apply::do_active_skill_ex(
                svc,
                ctx,
                src,
                p.active_skill,
                1,
                target,
                false,
                control,
            );
        }
    }
    result
}

/// The caster side of `DoApplySkillEx` (`Ctrl.cpp:1630-1672`): any skill but Dark Illusion ends the caster's Dark
/// Illusion; casting one of the exclusive Jester buffs removes the other two.
fn caster_bookkeeping(svc: &mut Services, ctx: &mut GameCtx<'_>, src: ObjId, a: &Applied<'_>) {
    use crate::buff::core::{BuffCx, remove_fake};
    use crate::buff::set::{BuffKey, BuffKind};
    use crate::buff::status;
    if ctx.worlds.mover(src).is_none() {
        return;
    }
    let p = &*a.prop;
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let cx = BuffCx::new(env, ctx.clock.game_ms);
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.mover_mut(src) {
        let illusion = BuffKey::new(BuffKind::Skill, SI_ACR_SUP_DARKILLUSION);
        if p.id != SI_ACR_SUP_DARKILLUSION && m.buffs.contains(illusion) {
            status::clear_dark_cover(m, &cx, &mut fx);
            remove_fake(m, &cx, illusion, &mut fx);
        }
        let exclusive: &[u32] = match p.id {
            SI_JST_SUP_POISON => &[SI_JST_SUP_BLEEDING, SI_JST_SUP_ABSORB],
            SI_JST_SUP_BLEEDING => &[SI_JST_SUP_POISON, SI_JST_SUP_ABSORB],
            SI_JST_SUP_ABSORB => &[SI_JST_SUP_BLEEDING, SI_JST_SUP_POISON],
            _ => &[],
        };
        for &s in exclusive {
            remove_fake(m, &cx, BuffKey::new(BuffKind::Skill, s), &mut fx);
        }
    }
    fx.flush_for(svc, ctx, src);
}

/// `CCtrl::DoApplySkill` (`Ctrl.cpp:1450-1470`): [`do_apply_skill_ex`], then a hostile skill that reached a
/// different mover counts as an attack on it (aggro, PK bookkeeping): `OnAttacked(caster, 0, TRUE, 0)`.
#[allow(clippy::too_many_arguments)]
pub fn do_apply_skill(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    target: ObjId,
    a: &Applied<'_>,
    ignore_prob: bool,
    param: u32,
    only_dmg: bool,
    control: bool,
) -> i32 {
    let result = do_apply_skill_ex(
        svc,
        ctx,
        src,
        target,
        a,
        ignore_prob,
        param,
        only_dmg,
        control,
    );
    if result == 1
        && src != target
        && a.prop.evildoing < 0
        && ctx.worlds.mover(src).is_some()
        && ctx.worlds.mover(target).is_some()
    {
        crate::combat::skill_attacked(svc, ctx, target, src);
    }
    result
}
