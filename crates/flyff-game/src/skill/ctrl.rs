//! The ground DoT and the Psychic Wall (Plan/17 §6.12; WP-SKILL-06; F-092;
//! `src:_Common/CtrlObjs.cpp:16-80`, `src:_Common/CommonCtrl.cpp:100-310`, `Ctrl.cpp:1544-1625`).
//!
//! Both are ctrls that a skill leaves behind. [`process`] runs them once per tick in the service context, right
//! after the world phase ([`crate::combat::tick::finish`]): their damage goes through the arbiter and the death
//! pipeline, which need the services.
//!
//! **Ground DoT** (`CContDamageCtrl`, a *virtual* object: no id, not in the link map, never sent to a client): an
//! area skill whose row has `continuous_pain` (Burning Field: 2 s) leaves a [`GroundDot`] at the target's
//! position. It strikes every `prop.continuous_pain` ms (strictly more than the period since the last strike,
//! counted in game time and checked once per tick, so about every 2.05 s) with
//! [`apply_skill_region`] around its position for the caster, `only_dmg = true`:
//! the **full** skill damage again and no buff. It ends `add.skill_time` ms after its creation (strictly) or when
//! its caster is gone; a dot created while the caster and the target dueled stops striking once the caster's
//! duel is over (it is kept, not deleted).
//!
//! **Psychic Wall** (`CI_PSYCHICWALL`, a visible ctrl): a player owns at most two; casting with both standing
//! fails (the skill answers 0). Each tick, the wall rolls its damage (`ability_min/max + level + int / 10 *
//! skill_lvl`, then the PvP row's, two draws in this order whether or not anybody is touched), and
//!
//! 1. every non-peaceful live mover (a guard only when the caster is chaotic or not a player) within
//!    [`in_range_point`]`(wall, 1.0)` loses HP **directly** (KEEP-QUIRK: no defense,
//!    no hate, no PK check, no arbiter): `Damage{AF_GENERIC, hp_after}` when it survives, else the death pipeline
//!    with the caster as the killer;
//! 2. every other live player within 1 m takes `send_damage(AF_FORCE, caster, dmg, side target)` when the caster
//!    is a player and the victim is its duel target (the PvP roll), or the caster has `control` or the pair are
//!    guild-combat / arena targets (the plain roll);
//! 3. each contact (a monster hit, a player that really lost HP) raises the wall's life by one; at `level / 2`
//!    the wall is destroyed (the rest of that tick's contacts are still processed) and the contact is pushed
//!    `0.825` along its own heading plus 180 degrees (not a mid-boss);
//! 4. a wall whose caster vanished, or older than `add.skill_time` ms, is destroyed.

use crate::combat::pvp::hit::{is_arena_target, is_guild_combat_target, is_pvp_target};
use crate::combat::state::{is_chaotic, is_live, is_peaceful};
use crate::combat::{DamageRequest, KillCtx, send_damage};
use crate::ctx::{GameCtx, Services};
use crate::item::fx::Fx;
use crate::movement::intents;
use crate::mover::Mover;
use crate::skill::apply::Applied;
use crate::skill::area::{ApplyType, apply_skill_region, in_range_point};
use crate::stats::env::{bare_inputs, point_param, prop_of};
use crate::world::ctrl::{CI_PSYCHICWALL, Ctrl, CtrlSpecial, PsychicWallState};
use crate::world::grid::LinkSet;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, LayerId, ObjId, WorldId};
use flyff_data::worlds::CtrlElemDef;
use flyff_proto::pb;
use flyff_rules::combat::{AtkFlags, DamageParam, x_random_range};
use flyff_rules::consts::dst::{DST_HP, RANK_GUARD, RANK_MIDBOSS};
use flyff_rules::stats::get_int;
use flyff_sim::num::set_angle_quantized;
use std::sync::Arc;

/// `CContDamageCtrl` (see the module documentation).
#[derive(Clone, Debug, PartialEq)]
pub struct GroundDot {
    /// `m_idSrc`: the caster.
    pub caster: ObjId,
    /// `m_idTarget`: the primary target the skill was cast on (the centre of the players' `is_target`).
    pub centre: ObjId,
    /// The target's position at the cast (`SetPos(pTarget->GetPos())`).
    pub pos: Vec3,
    /// The world and layer of the caster at the cast (`ADDOBJ(.., GetLayer())`).
    pub world: WorldId,
    /// See [`GroundDot::world`].
    pub layer: LayerId,
    /// The skill (`m_pSkillProp`) and the level of its row (`m_pAddSkillProp`).
    pub skill: u32,
    /// See [`GroundDot::skill`].
    pub level: u32,
    /// `m_bControl`.
    pub control: bool,
    /// `m_bDuelTarget`: the caster and the target were duel partners at the cast.
    pub duel_target: bool,
    /// `m_tmStart`.
    pub start: GameMs,
    /// `m_tmUpdate`: the last strike.
    pub update: GameMs,
}

// ---- creation --------------------------------------------------------------------------------------------------------

/// Leaves a [`GroundDot`] at the position of `target` for the caster `src` (`DoApplySkillEx`, the
/// `tmContinuousPain` branch of the area skills).
pub fn spawn_dot(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    target: ObjId,
    a: &Applied<'_>,
    control: bool,
) {
    let (Some(caster), Some(t)) = (ctx.worlds.mover(src), ctx.worlds.mover(target)) else {
        return;
    };
    let now = ctx.clock.game_ms;
    let dot = GroundDot {
        caster: src,
        centre: target,
        pos: t.core.pos,
        world: caster.core.world,
        layer: caster.core.layer,
        skill: a.prop.id,
        level: a.level(),
        control,
        duel_target: is_pvp_target(caster, t),
        start: now,
        update: now,
    };
    svc.skill.dots.push(dot);
}

/// The Psychic Wall of the player `caster` (`DoApplySkillEx`, `Ctrl.cpp:1590-1625`): the first free of its two
/// slots gets a new wall at its position with the angle `-angle`. `false` when it is no player, both slots are
/// taken or the wall cannot be placed (the skill then answers 0).
pub fn spawn_wall(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    caster: ObjId,
    a: &Applied<'_>,
    control: bool,
) -> bool {
    let Some(m) = ctx.worlds.player(caster) else {
        return false;
    };
    let (world, layer, pos, angle) = (m.core.world, m.core.layer, m.core.pos, m.core.angle);
    let walls = m.player().map_or([None; 2], |p| p.skills_rt.walls);
    let standing = |id: Option<ObjId>| {
        id.is_some_and(|id| ctx.worlds.store().ctrl(id).is_some_and(|c| !c.core.deleted))
    };
    let Some(slot) = (0..2).find(|&i| !standing(walls[i])) else {
        return false;
    };
    let mut wall = Ctrl::new(
        CI_PSYCHICWALL,
        pos,
        set_angle_quantized(-angle),
        CtrlElemDef::default(),
        false,
    );
    wall.special = CtrlSpecial::PsychicWall(Box::new(PsychicWallState {
        caster: Some(caster),
        skill: a.prop.id,
        level: a.level(),
        control,
        born: ctx.clock.game_ms,
        life: 0,
    }));
    let Ok(id) = ctx.worlds.spawn_ctrl(world, layer, wall) else {
        return false;
    };
    if let Some(p) = ctx.worlds.player_mut(caster).and_then(Mover::player_mut) {
        p.skills_rt.walls[slot] = Some(id);
    }
    true
}

// ---- the per-tick pass -----------------------------------------------------------------------------------------------

/// One tick of every ground DoT and every Psychic Wall (called by [`crate::combat::tick::finish`]).
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !svc.skill.dots.is_empty() {
        process_dots(svc, ctx);
    }
    let walls: Vec<ObjId> = ctx
        .worlds
        .store()
        .ctrls
        .values()
        .filter(|c| !c.core.deleted && matches!(c.special, CtrlSpecial::PsychicWall(_)))
        .map(|c| c.core.id)
        .collect();
    for id in walls {
        process_wall(svc, ctx, id);
    }
}

fn process_dots(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let dots = std::mem::take(&mut svc.skill.dots);
    let mut keep = Vec::with_capacity(dots.len());
    for mut d in dots {
        if tick_dot(svc, ctx, &mut d) {
            keep.push(d);
        }
    }
    // a strike may have left a dot of its own (a secondary skill): they come after the survivors
    keep.append(&mut svc.skill.dots);
    svc.skill.dots = keep;
}

/// `CContDamageCtrl::Process`; `false` when the dot is deleted.
fn tick_dot(svc: &mut Services, ctx: &mut GameCtx<'_>, d: &mut GroundDot) -> bool {
    if ctx.worlds.layer(d.world, d.layer).is_none() {
        return false;
    }
    let data = Arc::clone(ctx.data);
    let Some(a) = Applied::skill(&data, d.skill, d.level) else {
        return false;
    };
    let Some(add) = a.add.as_deref() else {
        return false;
    };
    let now = ctx.clock.game_ms;
    let since = |t: GameMs| now.0.wrapping_sub(t.0) as u32 as i32;
    let mut alive = true;
    if since(d.update) > a.prop.continuous_pain as i32 {
        d.update = now;
        alive = strike(svc, ctx, d, &a);
    }
    if since(d.start) > add.skill_time as i32 {
        alive = false;
    }
    alive
}

/// `CContDamageCtrl::ApplyDamage`; `false` when the caster is gone (the dot is deleted).
fn strike(svc: &mut Services, ctx: &mut GameCtx<'_>, d: &GroundDot, a: &Applied<'_>) -> bool {
    let Some(caster) = ctx.worlds.mover(d.caster).filter(|m| !m.core.deleted) else {
        return false;
    };
    if caster.combat.duel.duel == 0 && d.duel_target {
        return true;
    }
    let centre = ctx.worlds.core(d.centre).map(|c| c.id);
    apply_skill_region(
        svc,
        ctx,
        d.caster,
        d.pos,
        ApplyType::PLAYER | ApplyType::MONSTER,
        a,
        false,
        true,
        centre,
        d.control,
    );
    true
}

// ---- the Psychic Wall ------------------------------------------------------------------------------------------------

/// The reach of a wall in link units (`fDepth = 3` gives `nRange = 4`).
const WALL_N_RANGE: i32 = 4;
/// The heading push of a contact.
const WALL_PUSH: f32 = 0.825;

/// `CCommonCtrl::DestroyWall`: frees the caster's slot and deletes the ctrl.
fn destroy_wall(ctx: &mut GameCtx<'_>, wall: ObjId, caster: Option<ObjId>) {
    if let Some(c) = caster
        && let Some(p) = ctx.worlds.player_mut(c).and_then(Mover::player_mut)
    {
        for slot in &mut p.skills_rt.walls {
            if *slot == Some(wall) {
                *slot = None;
            }
        }
    }
    ctx.worlds.delete(wall);
}

/// `CCommonCtrl::_ProcessWall` for the wall `id`.
fn process_wall(svc: &mut Services, ctx: &mut GameCtx<'_>, id: ObjId) {
    let Some((st, pos)) = ctx.worlds.store().ctrl(id).and_then(|c| match &c.special {
        CtrlSpecial::PsychicWall(s) => Some(((**s).clone(), c.core.pos)),
        CtrlSpecial::Common => None,
    }) else {
        return;
    };
    let Some(caster_id) = st.caster else {
        destroy_wall(ctx, id, None);
        return;
    };
    let data = Arc::clone(ctx.data);
    let Some(a) = Applied::skill(&data, st.skill, st.level) else {
        destroy_wall(ctx, id, Some(caster_id));
        return;
    };
    let Some(add) = a.add.as_deref() else {
        destroy_wall(ctx, id, Some(caster_id));
        return;
    };
    // the attacker has to be around
    let core = svc.stats.handle();
    let Some((level, int, attacker_player, attacker_chaotic, world)) = ctx
        .worlds
        .mover(caster_id)
        .filter(|m| !m.core.deleted)
        .map(|m| {
            (
                m.mv.level,
                get_int(&bare_inputs(m, &core)),
                m.is_player(),
                is_chaotic(m),
                m.core.world,
            )
        })
    else {
        destroy_wall(ctx, id, Some(caster_id));
        return;
    };
    // the two rolls of the tick
    let lvl = add.skill_lvl as i32;
    let bonus = level.wrapping_add((int / 10).wrapping_mul(lvl));
    let roll = |min: u32, max: u32, ctx: &mut GameCtx<'_>| {
        x_random_range(
            ctx.rng.world(world),
            (min as i32).wrapping_add(bonus),
            (max as i32).wrapping_add(bonus),
        )
    };
    let dmg = roll(add.ability_min, add.ability_max, ctx);
    let dmg_pvp = roll(add.ability_min_pvp, add.ability_max_pvp, ctx);
    let max_life = (add.skill_lvl / 2) as i32;
    let mut life = st.life;
    let mut broken = false;

    // (a) the monsters (and pets, ...) of the dynamic link map: direct HP loss
    let Some((world, layer)) = ctx
        .worlds
        .store()
        .ctrl(id)
        .map(|c| (c.core.world, c.core.layer))
    else {
        return;
    };
    let cands = window(ctx, world, layer, pos, LinkSet::Others);
    for cand in cands {
        let Some(t) = ctx.worlds.mover(cand) else {
            continue;
        };
        let guard = prop_of(t, &core).class == RANK_GUARD;
        let apply = !is_peaceful(t) && !(attacker_player && !attacker_chaotic && guard);
        if !apply || t.core.deleted || !is_live(t) || !in_range_point(ctx, cand, pos, 1.0) {
            continue;
        }
        let hp = point_param(t, &core, DST_HP);
        let after = hp.wrapping_sub(dmg);
        let midboss = prop_of(t, &core).class == RANK_MIDBOSS;
        let heading = t.sim.angle;
        if after > 0 {
            if let Some(t) = ctx.worlds.mover_mut(cand) {
                t.mv.hp = after;
            }
            broadcast_damage(svc, ctx, caster_id, cand, dmg, after);
        } else {
            // dies: the death pipeline hands out the experience and the loot and runs `DoDie`
            if let Some(t) = ctx.worlds.mover_mut(cand) {
                t.mv.hp = 0;
            }
            ctx.worlds.movement.combat.queues.deaths.push(KillCtx {
                victim: cand,
                attacker: caster_id,
                msg: flyff_rules::combat::objmsg::DAMAGE,
                reflect: 0,
            });
        }
        life += 1;
        if life >= max_life {
            broken = true;
        }
        if !midboss && let Some(t) = ctx.worlds.mover_mut(cand) {
            intents::push(t, heading + 180.0, WALL_PUSH);
        }
    }
    crate::combat::death::process_pending(svc, ctx);

    // (b) the players: forced damage for duel, control, guild-combat and arena targets
    let cands = window(ctx, world, layer, pos, LinkSet::Players);
    for cand in cands {
        if cand == caster_id {
            continue;
        }
        let Some(t) = ctx.worlds.mover(cand) else {
            continue;
        };
        if t.core.deleted || !is_live(t) || !in_range_point(ctx, cand, pos, 1.0) {
            continue;
        }
        let chosen = match ctx.worlds.mover(caster_id) {
            Some(att) if attacker_player => {
                if is_pvp_target(att, t) {
                    Some(dmg_pvp)
                } else if st.control
                    || is_guild_combat_target(att, t, &*ctx.worlds.movement.combat.hooks)
                    || is_arena_target(att, t)
                {
                    Some(dmg)
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(n) = chosen else {
            continue;
        };
        let heading = t.sim.angle;
        let req = DamageRequest::new(
            cand,
            caster_id,
            AtkFlags::FORCE,
            DamageParam(n.max(0) as u32),
        )
        .side_target();
        if send_damage(svc, ctx, req) > 0 {
            life += 1;
            if life >= max_life {
                broken = true;
            }
            if let Some(t) = ctx.worlds.mover_mut(cand) {
                intents::push(t, heading + 180.0, WALL_PUSH);
            }
        }
    }

    // the book-keeping of the wall itself
    if let Some(CtrlSpecial::PsychicWall(s)) =
        ctx.worlds.store_mut().ctrl_mut(id).map(|c| &mut c.special)
    {
        s.life = life;
    }
    let age = ctx.clock.game_ms.0.wrapping_sub(st.born.0) as u32 as i32;
    if broken || age > add.skill_time as i32 {
        destroy_wall(ctx, id, Some(caster_id));
    }
}

/// The ids of one link set around `pos` in the legacy window of the wall.
fn window(
    ctx: &GameCtx<'_>,
    world: WorldId,
    layer: LayerId,
    pos: Vec3,
    set: LinkSet,
) -> Vec<ObjId> {
    let mut out = Vec::new();
    if let Some(l) = ctx.worlds.layer(world, layer) {
        l.grid
            .for_each_legacy(pos, WALL_N_RANGE, set, |id| out.push(id));
    }
    out
}

/// `AddDamage(victim, attacker, dmg, AF_GENERIC)` for a wall hit that did not kill (`hp_after` is the HP left).
fn broadcast_damage(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    attacker: ObjId,
    victim: ObjId,
    dmg: i32,
    hp_after: i32,
) {
    let attacker_angle = ctx.worlds.mover(attacker).map_or(0.0, |a| a.sim.angle);
    let mut fx = Fx::new();
    fx.out.near(pb::Damage {
        attacker: attacker.0,
        damage: u32::try_from(dmg).unwrap_or(0),
        atk_flags: AtkFlags::GENERIC.bits(),
        defender_pos: None,
        defender_angle: 0.0,
        hp_after,
        server_frame: ctx.clock.tick.frames_completed_after().0,
        attacker_angle,
    });
    fx.flush_for(svc, ctx, victim);
}
