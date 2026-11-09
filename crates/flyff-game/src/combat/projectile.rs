//! The server projectile manager (Plan/17 §4; WP-COMBAT-06): every wand bolt, arrow and skill shot is simulated
//! on the server frame by frame with `flyff_sim::projectile` (the same step function the browser renders with),
//! and the damage lands when the projectile arrives.
//!
//! **Lifecycle.**
//!
//! 1. *Spawn* at the attacker's hit frame `F` ([`spawn_shot`]): a [`LiveProjectile`] with `server_frame = F + 1`,
//!    the origin `attacker.pos + origin_offset` (the kind's row of [`super::projectile_kind`]), and a
//!    `ProjectileSpawn` to the attacker's viewers and the attacker. Spawning never needs a projectile class for
//!    the shot to exist on the wire, but a shot without a class ([`super::projectile_kind::kind_for_sfx`] answers
//!    `None`) is not spawned at all: no projectile, no damage (KQ-35).
//! 2. *Step* once per frame at the barrier of the frame, after every mover has stepped it ([`step_layer`]): the
//!    positions the projectile homes on are the **end-of-frame** positions, exactly what the sim documents. A
//!    projectile spawned at frame `F` first moves at the barrier of `F + 1` (`server_frame`), so it is at its
//!    origin for one full frame, as `flyff_sim::projectile` specifies.
//! 3. *Contact*: the step reports `Hit` at frame `H`; the damage follows after the kind's damage delay `d`
//!    (the legacy classes that play their hit effect first): at the barrier of `H + d` the impact of §4.4 runs.
//! 4. *Impact* (`AttackBySFX`): `ProjectileHit{expired = false}` first (G6), then the damage through the arbiter
//!    (bow and monster shots roll `attack_result`, a wand never misses), then the push of a knock-back shot.
//!    A target that is gone, or an attacker that no longer exists, ends the projectile with
//!    `ProjectileHit{expired = true}` and no damage; a target that is dead gets the hit effect but no damage.
//! 5. *Expiry*: the class's own timeout (10 s generally), a target that vanishes under a class that deletes
//!    itself, or a projectile of another layer than the target's.
//!
//! The manager keeps every layer's projectiles in one list in id order; [`step_layer`] only touches those of the
//! layer being processed.

use super::action::roll_attack_result;
use super::arbiter::{DamageRequest, is_steal, process_damage};
use super::context::ActionCx;
use super::projectile_kind::{params as kind_row, speed as kind_speed};
use super::skillhit::{SkillHitHooks, SkillImpact};
use super::state::is_live;
use crate::movement::FrameLookup;
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, SimFrame, WorldId};
use flyff_proto::pb;
use flyff_rules::combat::{AtkFlags, DamageParam};
use flyff_rules::consts::NULL_ID;
use flyff_rules::consts::dst::{RANK_MIDBOSS, RANK_SUPER};
use flyff_sim::projectile::{Launch, ProjStep, Projectile, ProjectileKind, SpawnArgs};
use flyff_sim::step::MoverLookup;
use flyff_sim::types::OBJSTA_DMG_FLY_ALL;

/// The `SFXHIT_INFO` content of a shot (`OnAttackSFX`, `MoverAttack.cpp:2363-2381`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitPayload {
    /// `nMagicPower`: the wand / bow charge, or the skill level.
    pub magic_power: u32,
    /// The skill id; 0 = an autoattack (`NULL_ID` in the damage parameter).
    pub skill_id: u32,
    /// `nDmgCnt` (always 0 in this build).
    pub dmg_cnt: u32,
    /// `fDmgAngle`: the attacker's angle at the hit frame.
    pub dmg_angle: f32,
    /// `fDmgPower`: the knock-back (`dmg_shift * 0.2`; 0 = none).
    pub dmg_power: f32,
    /// `dwAtkFlags`.
    pub atk_flags: AtkFlags,
    /// `bControl`.
    pub control: bool,
}

impl HitPayload {
    /// The payload of an autoattack shot: no skill, no knock-back.
    pub fn autoattack(magic_power: u32, atk_flags: AtkFlags, dmg_angle: f32) -> HitPayload {
        HitPayload {
            magic_power,
            skill_id: 0,
            dmg_cnt: 0,
            dmg_angle,
            dmg_power: 0.0,
            atk_flags,
            control: false,
        }
    }
}

/// One live projectile.
#[derive(Clone, Debug)]
pub struct LiveProjectile {
    /// The sim state (position, velocity, age, outcome).
    pub p: Projectile,
    /// The world the projectile flies in.
    pub world: WorldId,
    /// The layer.
    pub layer: LayerId,
    /// The shooter.
    pub attacker: ObjId,
    /// The hit information.
    pub payload: HitPayload,
    /// `ProjectileSpawn.sfx_id`.
    pub sfx_id: u32,
    /// `ProjectileSpawn.hit_sfx_id`.
    pub hit_sfx_id: u32,
    /// The frame it stands at its origin.
    pub server_frame: SimFrame,
    /// The multi-shot index.
    pub shot_index: u32,
    /// The frame the contact happened in (`None` while flying).
    pub contact_frame: Option<SimFrame>,
    /// Where the contact happened.
    pub contact_pos: Vec3,
    /// The frame the damage is applied in (`contact + delay`).
    pub impact_frame: Option<SimFrame>,
}

/// The live projectiles of every layer and the id counter (see the module documentation).
#[derive(Clone, Debug)]
pub struct ProjectileManager {
    live: Vec<LiveProjectile>,
    next_id: u32,
}

impl Default for ProjectileManager {
    fn default() -> Self {
        ProjectileManager {
            live: Vec::new(),
            next_id: 1,
        }
    }
}

impl ProjectileManager {
    /// Allocates the next projectile id (a per-process counter, never reused; 0 is never handed out).
    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Number of live projectiles.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// `true` when nothing flies.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The live projectiles in id order.
    pub fn iter(&self) -> impl Iterator<Item = &LiveProjectile> {
        self.live.iter()
    }

    /// The projectile `id`.
    pub fn get(&self, id: u32) -> Option<&LiveProjectile> {
        self.live.iter().find(|p| p.p.id == id)
    }

    /// Drops everything (a test helper).
    pub fn clear(&mut self) {
        self.live.clear();
    }
}

/// What a spawn site asks for ([`spawn_shot`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotRequest {
    /// The shooter.
    pub attacker: ObjId,
    /// The homing target.
    pub target: ObjId,
    /// The projectile class.
    pub kind: ProjectileKind,
    /// `ProjectileSpawn.sfx_id` (the flying effect).
    pub sfx_id: u32,
    /// `ProjectileSpawn.hit_sfx_id` (the effect on impact; 0 = none).
    pub hit_sfx_id: u32,
    /// The id announced by the attack notification, or `None` to allocate one now.
    pub projectile_id: Option<u32>,
    /// The hit information.
    pub payload: HitPayload,
    /// The multi-shot index.
    pub shot_index: u32,
}

/// Spawns a projectile at the hit frame (the barrier frame is `cx.sc.frame - 1`): the live entry and the
/// `ProjectileSpawn` message. Returns the projectile id, or `None` when the attacker or the target does not
/// exist.
pub fn spawn_shot(cx: &mut ActionCx<'_>, req: ShotRequest) -> Option<u32> {
    let (a, t) = (cx.mover(req.attacker)?, cx.mover(req.target)?);
    if a.core.world != t.core.world || a.core.layer != t.core.layer {
        return None;
    }
    let row = kind_row(req.kind);
    let origin = Vec3::new(
        a.sim.pos.x + row.origin_offset.x,
        a.sim.pos.y + row.origin_offset.y,
        a.sim.pos.z + row.origin_offset.z,
    );
    // the destination at spawn: the box centre for the wand bolt, the kind's offset otherwise
    let dest_y = if req.kind == ProjectileKind::WandAtk1 {
        t.sim.params.bounds.max.y.abs() / 2.0 * t.sim.params.scale
    } else {
        req.kind.info().dest_offset_y
    };
    let dest = Vec3::new(t.sim.pos.x, t.sim.pos.y + dest_y, t.sim.pos.z);
    let faster = cx.act.faster_projectiles;
    let spd = kind_speed(req.kind, faster);
    let launch = match req.kind {
        ProjectileKind::WandAtkAir | ProjectileKind::AtkStraight => Some(Launch {
            yaw: a.sim.angle,
            pitch: a.sim.angle_x,
        }),
        _ => None,
    };
    let (world, layer) = (a.core.world, a.core.layer);
    let attacker_pos = a.sim.pos;
    let id = req
        .projectile_id
        .unwrap_or_else(|| cx.q.projectiles.alloc_id());
    let mut args = SpawnArgs::new(req.kind, origin, dest, req.target);
    args.id = id;
    args.source = req.attacker;
    args.speed = spd;
    args.launch = launch;
    let mut p = Projectile::spawn(&args);
    if req.kind == ProjectileKind::YoyoAtk {
        p.make_yoyo_path(attacker_pos);
    }
    let server_frame = cx.sc.frame;
    cx.arb.fx.push(
        req.attacker,
        crate::stats::out::Aud::NearInclSelf,
        pb::ProjectileSpawn {
            projectile_id: id,
            kind: wire_kind(req.kind) as i32,
            sfx_id: req.sfx_id,
            hit_sfx_id: req.hit_sfx_id,
            target_obj_id: req.target.0,
            origin: Some(pb_vec(origin)),
            dest: Some(pb_vec(dest)),
            server_frame: server_frame.0,
            speed: spd,
            skill_id: req.payload.skill_id,
            // the impact sound comes from the client's IMPACT_SOUND table (Plan/17 §4.6)
            hit_sound_id: 0,
            shot_index: req.shot_index,
        },
    );
    let live = LiveProjectile {
        p,
        world,
        layer,
        attacker: req.attacker,
        payload: req.payload,
        sfx_id: req.sfx_id,
        hit_sfx_id: req.hit_sfx_id,
        server_frame,
        shot_index: req.shot_index,
        contact_frame: None,
        contact_pos: origin,
        impact_frame: None,
    };
    cx.q.projectiles.live.push(live);
    cx.q.stats.projectiles_spawned += 1;
    Some(id)
}

fn pb_vec(v: Vec3) -> pb::Vec3 {
    pb::Vec3 {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

/// The protobuf class of a sim kind (the numbers are the same).
fn wire_kind(k: ProjectileKind) -> pb::ProjectileKind {
    pb::ProjectileKind::try_from(i32::from(k.as_u8())).unwrap_or(pb::ProjectileKind::Unspecified)
}

/// One frame of the projectile phase for the layer `(world, layer)`: `frame` is the frame that just stepped
/// (the barrier's frame `F`); the impacts apply from `F + 1` (`cx.sc.frame`).
pub fn step_layer(
    cx: &mut ActionCx<'_>,
    skill: &mut dyn SkillHitHooks,
    world: WorldId,
    layer: LayerId,
    frame: SimFrame,
) {
    if cx.q.projectiles.live.is_empty() {
        return;
    }
    let all = std::mem::take(&mut cx.q.projectiles.live);
    let (mut mine, mut others): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|p| p.world == world && p.layer == layer);
    if mine.is_empty() {
        cx.q.projectiles.live = others;
        return;
    }
    // the end-of-frame snapshot of every target and shooter
    let mut ids: Vec<ObjId> = mine
        .iter()
        .flat_map(|p| [p.p.target, p.attacker])
        .filter(|id| {
            cx.mover(*id)
                .is_some_and(|m| m.core.world == world && m.core.layer == layer)
        })
        .collect();
    ids.sort();
    ids.dedup();
    let lookup = FrameLookup::for_objects(cx.arb.w.store, &ids, &cx.sc.shared.flags);
    let mut keep = Vec::with_capacity(mine.len());
    for mut lp in mine.drain(..) {
        if lp.server_frame > frame {
            // spawned in this very barrier: it starts moving at its own frame
            keep.push(lp);
            continue;
        }
        let done = advance(cx, skill, &mut lp, &lookup, frame);
        if !done {
            keep.push(lp);
        }
    }
    // whatever the handlers spawned meanwhile is in `live`; merge by id
    let spawned = std::mem::take(&mut cx.q.projectiles.live);
    keep.extend(spawned);
    keep.append(&mut others);
    keep.sort_by_key(|p| p.p.id);
    cx.q.projectiles.live = keep;
}

/// Advances one projectile by the frame `frame`; `true` when it is finished and must be dropped.
fn advance(
    cx: &mut ActionCx<'_>,
    skill: &mut dyn SkillHitHooks,
    lp: &mut LiveProjectile,
    lookup: &dyn MoverLookup,
    frame: SimFrame,
) -> bool {
    // the damage after the contact delay
    if let Some(at) = lp.impact_frame {
        if frame >= at {
            impact(cx, skill, lp, frame);
            return true;
        }
        return false;
    }
    match lp.p.step_in(lookup) {
        ProjStep::Flying => false,
        ProjStep::Hit { pos } => {
            lp.contact_frame = Some(frame);
            lp.contact_pos = pos;
            let delay = u64::from(kind_row(lp.p.kind).damage_delay_frames);
            let at = SimFrame(frame.0 + delay);
            lp.impact_frame = Some(at);
            if delay == 0 {
                impact(cx, skill, lp, frame);
                true
            } else {
                false
            }
        }
        ProjStep::Expired => {
            end_expired(cx, lp, frame);
            true
        }
    }
}

/// `ProjectileHit{expired = true}`: the projectile ended without an impact.
fn end_expired(cx: &mut ActionCx<'_>, lp: &LiveProjectile, frame: SimFrame) {
    cx.q.stats.projectiles_expired += 1;
    cx.arb.fx.push(
        lp.attacker,
        crate::stats::out::Aud::NearInclSelf,
        pb::ProjectileHit {
            projectile_id: lp.p.id,
            target_obj_id: lp.p.target.0,
            pos: Some(pb_vec(lp.p.pos)),
            server_frame: frame.0,
            expired: true,
        },
    );
}

/// The impact of §4.4 (`AttackBySFX`), at the barrier of `frame`.
fn impact(
    cx: &mut ActionCx<'_>,
    skill: &mut dyn SkillHitHooks,
    lp: &LiveProjectile,
    frame: SimFrame,
) {
    let (att, tgt) = (lp.attacker, lp.p.target);
    let target_here = cx
        .mover(tgt)
        .is_some_and(|m| m.core.world == lp.world && m.core.layer == lp.layer);
    // an attacker that no longer exists has no stats to hit with
    if !target_here || cx.mover(att).is_none() {
        end_expired(cx, lp, frame);
        return;
    }
    cx.q.stats.projectiles_hit += 1;
    cx.arb.fx.push(
        att,
        crate::stats::out::Aud::NearInclSelf,
        pb::ProjectileHit {
            projectile_id: lp.p.id,
            target_obj_id: tgt.0,
            pos: Some(pb_vec(lp.contact_pos)),
            server_frame: frame.0,
            expired: false,
        },
    );
    // `AttackBySFX`: a target that is not alive takes no damage
    if !cx.mover(tgt).is_some_and(is_live) {
        return;
    }
    let pl = lp.payload;
    let mut flags = pl.atk_flags;
    if flags.intersects(AtkFlags::GENERIC) && !roll_attack_result(cx, att, tgt) {
        flags = AtkFlags::MISS;
    }
    if cx.arb.env.steal_protection && is_steal(&mut cx.arb, att, tgt) {
        return;
    }
    if flags.is_skill_attack() {
        let applied = skill.on_skill_impact(
            cx,
            &SkillImpact {
                attacker: att,
                target: tgt,
                skill: pl.skill_id,
                magic_power: pl.magic_power,
                atk_flags: flags.bits(),
                control: pl.control,
                dmg_angle: pl.dmg_angle,
                dmg_power: pl.dmg_power,
            },
        );
        if !applied {
            return;
        }
    } else {
        let skill_part = if pl.skill_id == 0 {
            NULL_ID
        } else {
            pl.skill_id
        };
        let param = DamageParam::make(pl.magic_power, skill_part, pl.dmg_cnt);
        process_damage(&mut cx.arb, DamageRequest::new(tgt, att, flags, param));
    }
    if pl.dmg_power != 0.0 {
        apply_push(cx, tgt, pl.dmg_angle, pl.dmg_power);
    }
}

/// The knock-back of a shot (Plan/17 §4.4 step 5): the external force `AngleToVectorXZ(angle, power)` on the
/// server copy and a `PushPower` to everybody who sees the target, itself included (a player victim owns its
/// movement). No push for `RANK_SUPER` / `RANK_MIDBOSS` and none while the target is knocked into the air.
pub fn apply_push(cx: &mut ActionCx<'_>, victim: ObjId, push_angle: f32, power: f32) {
    let core = cx.arb.env.core;
    let Some(m) = cx.arb.w.store.mover_mut(victim) else {
        return;
    };
    let rank = core.data.mover(m.prop_id()).map_or(0, |p| p.class);
    let power = if rank == RANK_SUPER || rank == RANK_MIDBOSS {
        0.0
    } else {
        power
    };
    if m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0 {
        return;
    }
    let angle = m.sim.angle;
    flyff_sim::sync::push_power(&mut m.sim, angle, push_angle, power);
    let msg = pb::PushPower {
        pos: Some(pb_vec(m.sim.pos)),
        angle,
        push_angle,
        power,
        server_frame: cx.sc.frame.0,
    };
    cx.arb
        .fx
        .push(victim, crate::stats::out::Aud::NearInclSelf, msg);
}
