//! Cheering (Plan/21 §14; WP-SOCIAL-08; `src:WORLDSERVER/DPSrvr.cpp:7061-7120`, `src:_Common/Mover.cpp:9030-9053`).
//!
//! A player has up to three cheer points. `Cheering{target_obj}` spends one on another player in its observer set
//! (FIX VR-051: the original took any object id): the target hears who cheered, gets the `II_CHEERUP` buff and an
//! effect, the cheerer turns towards it and plays the cheer motion. A point comes back every 60 minutes while fewer
//! than three are held; nothing is persisted - a login starts with no points and the first one due in 60 minutes.

use super::party::wire::to_one;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::movement::{FrameLookup, intents};
use crate::outbox::Audience;
use flyff_common::defs::item::II_CHEERUP;
use flyff_common::defs::neuz::{MTI_CHEEROTHER, MTI_CHEERSAME};
use flyff_common::defs::obj::{XI_CHEERRECEIVEEFFECT, XI_CHEERSENDEFFECT};
use flyff_common::defs::text::{
    TID_CHEER_MESSAGE3, TID_CHEER_MESSAGE4, TID_CHEER_NO1, TID_CHEER_NO2,
};
use flyff_common::{GameMs, ObjId};
use flyff_proto::pb;
use flyff_sim::ActMsg;
use flyff_sim::step::{EventMode, NoRules, SimEnv, SimRole};
use flyff_sim::types::{ANILOOP_1PLAY, OBJMSG_MOTION};

/// `MAX_CHEERPOINT`.
pub const MAX_CHEER_POINTS: i32 = 3;
/// `TICK_CHEERPOINT`: 60 minutes per point.
pub const CHEER_POINT_MS: u64 = 60 * 60_000;

/// A login: no points, the first one in 60 minutes (`DPDatabaseClient.cpp:725`).
pub fn on_join(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let due = GameMs(ctx.clock.game_ms.0.saturating_add(CHEER_POINT_MS));
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.social.cheer_points = 0;
        p.social.tick_cheer = due;
    }
}

/// `CMover::SetCheerParam( points, now, rest )`: stores the points and the due time and tells the owner.
fn set_cheer(ctx: &mut GameCtx<'_>, who: ObjId, points: i32, rest_ms: u64) {
    let now = ctx.clock.game_ms;
    let Some(p) = ctx.worlds.player_mut(who).and_then(|m| m.player_mut()) else {
        return;
    };
    let add = p.social.cheer_points < points;
    p.social.cheer_points = points;
    p.social.tick_cheer = GameMs(now.0.saturating_add(rest_ms));
    to_one(
        ctx,
        who,
        pb::SetCheerParam {
            points,
            rest_ms: u32::try_from(rest_ms).unwrap_or(u32::MAX),
            add,
        },
    );
}

/// `CheckTickCheer` for every player, once per second (the party worker's slot): a player with fewer than three
/// points whose due time has passed gets one more and 60 minutes to the next.
pub fn process(_svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let due: Vec<ObjId> = ctx
        .worlds
        .store()
        .movers
        .iter()
        .filter_map(|(_, m)| {
            let p = m.player()?;
            // a due time of 0 is a clock that was never started (no join hook ran: a test avatar)
            (!m.core.deleted
                && p.social.cheer_points < MAX_CHEER_POINTS
                && p.social.tick_cheer.0 != 0
                && now > p.social.tick_cheer)
                .then(|| m.id())
        })
        .collect();
    for o in due {
        let points = ctx
            .worlds
            .player(o)
            .and_then(|m| m.player())
            .map_or(0, |p| p.social.cheer_points);
        set_cheer(ctx, o, points + 1, CHEER_POINT_MS);
    }
}

/// `Cheering{target_obj}` (`OnCheering`).
pub fn on_cheering(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::Cheering) {
    let target = ObjId(m.target_obj);
    if target == who.0 {
        return;
    }
    // VR-051: a player in the observer set of the cheerer
    let seen = ctx
        .worlds
        .core(who.0)
        .is_some_and(|c| c.viewers.contains(&target));
    if !seen || ctx.worlds.player(target).is_none() {
        notice::defined(ctx, who, TID_CHEER_NO2, Vec::new());
        return;
    }
    let now = ctx.clock.game_ms;
    let Some((points, due)) = ctx
        .worlds
        .player(who.0)
        .and_then(|mv| mv.player())
        .map(|p| (p.social.cheer_points, p.social.tick_cheer))
    else {
        return;
    };
    if points <= 0 {
        let minutes = due.0.saturating_sub(now.0) / 60_000;
        notice::defined(ctx, who, TID_CHEER_NO1, vec![minutes.to_string()]);
        return;
    }
    // the cheer is paid: a full set restarts the 60 minutes, otherwise the running ones go on
    let rest = if points >= MAX_CHEER_POINTS {
        CHEER_POINT_MS
    } else {
        due.0.saturating_sub(now.0)
    };
    set_cheer(ctx, who.0, points - 1, rest);
    // the cheerer turns towards the target and plays the motion that fits the sexes
    let (tpos, same_sex, my_name) = {
        let t = ctx.worlds.player(target);
        let me = ctx.worlds.player(who.0);
        let (Some(t), Some(me)) = (t, me) else { return };
        (
            t.core.pos,
            t.player().map(|p| p.appearance.sex) == me.player().map(|p| p.appearance.sex),
            me.player()
                .map(|p| p.ident.name.clone())
                .unwrap_or_default(),
        )
    };
    let (tid, mti) = if same_sex {
        (TID_CHEER_MESSAGE3, MTI_CHEERSAME)
    } else {
        (TID_CHEER_MESSAGE4, MTI_CHEEROTHER)
    };
    notice::defined(ctx, PlayerRef(target), tid, vec![my_name]);
    play_motion(ctx, who.0, tpos, mti);
    sfx(ctx, who.0, XI_CHEERSENDEFFECT);
    sfx(ctx, target, XI_CHEERRECEIVEEFFECT);
    if let Some(mv) = ctx.worlds.player_mut(who.0) {
        intents::force_pose_broadcast(mv);
    }
    if ctx.data.item(II_CHEERUP).is_some() {
        let _ = crate::buff::apply_item(svc, ctx, target, II_CHEERUP, None);
    }
}

/// `SetAngle( GetDegree( target, me ) )` and `SendActMsg( OBJMSG_MOTION, mti, ANILOOP_1PLAY )` on the server copy
/// of the cheerer.
pub(crate) fn play_motion(
    ctx: &mut GameCtx<'_>,
    who: ObjId,
    towards: flyff_common::math::Vec3,
    mti: u32,
) {
    let now = ctx.clock.frame0;
    let shared = ctx.worlds.movement.shared.clone();
    let Some(statics) = ctx.worlds.statics_of(who).cloned() else {
        return;
    };
    let (world, layer) = ctx
        .worlds
        .core(who)
        .map(|c| (c.world, c.layer))
        .unwrap_or_default();
    let dynamic = ctx.worlds.dynamic_of(world, layer);
    let lookup = FrameLookup::default();
    let rng = ctx.rng.world(world);
    let Some(m) = ctx.worlds.player_mut(who) else {
        return;
    };
    m.sim.angle =
        flyff_sim::num::set_angle_quantized(flyff_sim::num::get_degree(towards, m.sim.pos));
    let env = SimEnv {
        frame: now,
        role: SimRole::Server,
        flags: shared.flags,
        world: &statics.collision,
        dynamic: &dynamic,
        motions: &shared.motions,
        lookup: &lookup,
        mode: EventMode::Normal,
    };
    let _ = flyff_sim::send_act_msg(
        &mut m.sim,
        &env,
        &mut NoRules,
        rng,
        ActMsg::with(OBJMSG_MOTION, [mti as i32, ANILOOP_1PLAY as i32, 0, 0, 0]),
        &mut m.sync.events,
    );
}

/// `AddCreateSfxObj( mover, sfx )` at the mover's place, to everyone near it.
pub(crate) fn sfx(ctx: &mut GameCtx<'_>, target: ObjId, sfx_id: u32) {
    let Some(pos) = ctx.worlds.core(target).map(|c| c.pos) else {
        return;
    };
    ctx.broadcast(
        None,
        target,
        Audience::NearInclSelf,
        pb::CreateSfxObj {
            sfx_id,
            x: pos.x,
            y: pos.y,
            z: pos.z,
            flag: false,
        },
    );
}
