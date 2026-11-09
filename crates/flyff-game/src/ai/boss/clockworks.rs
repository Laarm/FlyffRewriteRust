//! Everything ClockWorks-specific (Plan/18 §10.2-§10.4; `AIClockWorks.cpp:297-310`, `MoverActEvent.cpp:921-1018`,
//! `1776-1832`; task boss-ai-core, WP-AI-18): the defence mode of its pre-step, the four hit handlers and the
//! laser continuation. The bands and the arrival mapping are [`super::tables`]; the process is [`super::rage`].
//!
//! | swing | `OBJMSG` / attack item | reach | hit frame | continuation |
//! |---|---|---|---|---|
//! | Slash | `ATK1` / `dwAtk1` | `R` | area around event point 0 | - |
//! | Shoot | `ATK2` / `dwAtk3` | 30 m | area around the target's position | - |
//! | Laser | `ATK3` / `dwAtk2` | `R` | nothing | every 4th `m_nCount` while `MVRF_HITCONT`: the terrain point of the ray from event point 2 along bone 6 |
//! | Quake | `ATK4` / `dwAtk4` | 0 | area around the boss, 15 % knock-up | - |
//!
//! `m_nMotionHitCount` is NOT incremented after a ClockWorks hit (the dispatcher does it for the other bosses).

use super::BossAi;
use super::hits::{act_item, bone_point, event_pos, on_damage_at};
use crate::ai::ctx::AiCx;
use crate::ai::monster::helpers::hp_percent;
use crate::ai::rng::x_random;
use crate::combat::ActionCx;
use crate::combat::action::target_valid;
use crate::mover::Mover;
use crate::stats::dst::{DstCtx, DstEmit, DstSource};
use crate::stats::out::Out;
use flyff_common::ObjId;
use flyff_common::math::Vec3;
use flyff_proto::pb;
use flyff_rules::combat::AtkFlags;
use flyff_rules::consts::NULL_CHGPARAM;
use flyff_rules::consts::dst::DST_ADJDEF_RATE;
use flyff_rules::dst::OriginMax;
use flyff_sim::types::{OBJSTA_ATK1, OBJSTA_ATK2, OBJSTA_ATK3, OBJSTA_ATK4};

/// The ledger tag of the defence mode (`DstSource::Script`): one entry per ClockWorks.
pub const BOSS_DEFENSE_TAG: u32 = 0xB055_0001;
/// `SetDestParam(DST_ADJDEF_RATE, 800)` (Q20): a percentage, so x9 defence; the source comment's "+30 %" is stale.
pub const DEFENSE_RATE: i32 = 800;
/// The knock-up chance of the Quake in percent (`xRandom(100) < 15`).
pub const QUAKE_FLY_PERCENT: u32 = 15;
/// The bone the laser runs along (`GetMatrixBone(6)`) and the offset it points to in model space.
pub const LASER_BONE: usize = 6;
/// The laser's far point in the model space of [`LASER_BONE`] (`D3DXVECTOR3(0, 0, 40.0f)`).
pub const LASER_REACH: Vec3 = Vec3::new(0.0, 0.0, 40.0);
/// The event point the laser starts at.
pub const LASER_EVENT: usize = 2;
/// The event point the Slash is centred on.
pub const SLASH_EVENT: usize = 0;

/// The ClockWorks pre-step of `MoveProcessRage` (KQ-13 Q20): on the first tick `m_bDefenseMode` is set and
/// `SetDestParam(DST_ADJDEF_RATE, 800, NULL_CHGPARAM)` raises the defence factor `1 + 800 / 100` to x9 for every
/// `AII_CLOCKWORKS` mover (`MI_LOADCLOCKWORK` and `MI_EVENT04` included). The hit-point test of the original is
/// `GetHitPointPercent() <= 100`, always true. The DST message goes to the visibility range, as the original's.
pub fn pre_step(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    if b.defense_mode || hp_percent(m, cx) > 100 {
        return;
    }
    b.defense_mode = true;
    let mut emit = Vec::new();
    {
        let mut ctx = DstCtx {
            // a stored primitive does not read the composite origin
            origin: OriginMax::default(),
            heal_cnt: &mut m.status.heal_cnt,
            heal_ticks: 0,
            emit: &mut emit,
            send: true,
        };
        m.mv.dst.apply(
            DstSource::Script {
                tag: BOSS_DEFENSE_TAG,
            },
            DST_ADJDEF_RATE,
            DEFENSE_RATE,
            NULL_CHGPARAM,
            &mut ctx,
        );
    }
    let mut out = Out::new();
    for e in emit {
        if let DstEmit::Set { dst, adj, chg } = e {
            out.near(pb::SetDestParam {
                dst: dst as i32,
                adj,
                chg,
            });
        }
    }
    out.flush_sink(&mut *cx.sink, m);
}

/// `OnAttackMelee_ClockWorks(state, target)`: the hit frame of one of the four swings.
pub fn on_hit(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, target: ObjId) {
    let item = act_item(cx, boss);
    match state {
        // the Slash: the damage lands where the effect appears, event point 0
        OBJSTA_ATK1 => {
            let pos = event_pos(cx, boss, SLASH_EVENT);
            on_damage_at(cx, boss, item, pos, AtkFlags::GENERIC);
        }
        // the cannon: the area is centred on the target's position
        OBJSTA_ATK2 => {
            if target_valid(cx, boss, target)
                && let Some(pos) = cx.mover(target).map(|t| t.core.pos)
            {
                on_damage_at(cx, boss, item, pos, AtkFlags::GENERIC);
            }
        }
        // the laser has no hit-frame damage: see `on_continue`
        OBJSTA_ATK3 => {}
        // the stomp: around the boss, 15 % of the swings knock up
        OBJSTA_ATK4 => {
            let Some(pos) = cx.mover(boss).map(|m| m.sim.pos) else {
                return;
            };
            let mut flags = AtkFlags::GENERIC;
            if x_random(&mut *cx.arb.w.rng, 100) < QUAKE_FLY_PERCENT {
                flags |= AtkFlags::FLYING;
            }
            on_damage_at(cx, boss, item, pos, flags);
        }
        _ => {}
    }
}

/// `OnAttackMeleeContinue(state)` of the ClockWorks: while the laser swing plays (`MVRF_HITCONT`, every frame
/// after the hit) each frame whose `m_nCount & 3 == 0` (about 15 Hz; the client uses `& 1`) casts the ray from
/// event point 2 toward the point `(0, 0, 40)` of bone 6 against the terrain and damages the players around the
/// hit point. `count` is the free-running count of the frame, so the phase of the 15 Hz differs from swing to
/// swing (KEEP). It is a RAY, not the segment: the 40 m only gives the direction.
pub fn on_continue(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, count: i32, _hit_count: i32) {
    if state != OBJSTA_ATK3 || count & 3 != 0 {
        return;
    }
    let item = act_item(cx, boss);
    let v1 = event_pos(cx, boss, LASER_EVENT);
    let v2 = bone_point(cx, boss, LASER_BONE, LASER_REACH);
    let d = v2 - v1;
    let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
    // `D3DXVec3Normalize` of a zero vector is zero: no ray, no hit
    if !len.is_finite() || len <= 0.0 {
        return;
    }
    let dir = Vec3::new(d.x / len, d.y / len, d.z / len);
    let Some(t) = cx.sc.collision.intersect_ray_terrain2(v1, dir) else {
        return;
    };
    let hit = Vec3::new(v1.x + dir.x * t, v1.y + dir.y * t, v1.z + dir.z * t);
    on_damage_at(cx, boss, item, hit, AtkFlags::GENERIC);
}
