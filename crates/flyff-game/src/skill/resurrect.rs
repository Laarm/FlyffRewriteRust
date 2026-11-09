//! The Resurrection skill and the buffs around a revival (Plan/17 §9.4, §9.5; WP-SKILL-10; PL-082;
//! `src:_Common/Ctrl.cpp:790-820`, `src:WORLDSERVER/DPSrvr.cpp:6861-6905`).
//!
//! `SI_ASS_HEAL_RESURRECTION` on a dead player does not revive anybody: the hard-coded skill step stores a
//! [`ResurrectionOffer`] on the dead player ([`offer`]) and sends it `ResurrectionMessage`; the client shows a
//! confirmation. `ResurrectionOk` ([`on_resurrection_ok`]) revives it:
//!
//! 1. an offer must exist, the player must still be dead and the caster online, else the offer is cleared and
//!    nothing happens;
//! 2. `CreateSfxObj(XI_SKILL_ASS_HEAL_RESURRECTION01)` and `Resurrection` to the viewers, the sim plays the
//!    resurrection state (`OBJMSG_RESURRECTION`);
//! 3. the skill's DST slots are applied with the caster as the source (`ApplyParam(src, prop, add, TRUE)`: the
//!    heal `adj_param_val[0]` plus the `RT_HEAL` refer);
//! 4. with `dest_param[1] == DST_RECOVERY_EXP` the death penalty is paid, reduced to `adj_param_val[1]` percent
//!    (level 1: 100 % ... level 20: 60 %), `death_penalty::apply(p, true, adj_param_val[1], false)`.
//!
//! `ResurrectionCancel` just clears the offer. There is no time limit and no range check (original).
//!
//! [`after_revival`] is `DeathHooks::after_revival` (PL-082): the death sent `RemoveAllSkillInfluence`, but the
//! charged item buffs and the pet and equip buffs survive on the server, so after **every** revival the server
//! sends `SetSkillState` for each surviving buff, in key order, to the viewers and the player.

use crate::buff::core::BuffCx;
use crate::buff::set::BuffKind;
use crate::combat::state::{ResurrectionOffer, is_die};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::skill::apply::{CasterFacts, apply_param};
use crate::skill::use_skill::props;
use crate::stats::dst::DstSource;
use flyff_common::ObjId;
use flyff_common::defs::attribute::DST_RECOVERY_EXP;
use flyff_common::defs::obj::XI_SKILL_ASS_HEAL_RESURRECTION01;
use flyff_proto::pb;
use flyff_sim::types::OBJSTA_RESURRECTION;

/// What [`offer`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offered {
    /// The offer was stored and `ResurrectionMessage` sent.
    Made,
    /// An offer is already pending on the player: nothing changes (the skill still counts as applied).
    AlreadyPending,
    /// The player is already in the resurrection state: the application is cancelled (`return FALSE`).
    Resurrecting,
    /// The target is not a dead player: nothing to offer.
    NotApplicable,
}

/// The hard-coded step of `SI_ASS_HEAL_RESURRECTION` (`ApplySkillHardCoding`, `Ctrl.cpp:796-815`): `caster` offers
/// to revive `target`. `Resurrecting` stops the application, `Made` / `AlreadyPending` end it with the offer pending.
pub fn offer(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    caster: ObjId,
    target: ObjId,
    skill: u32,
    level: u32,
) -> Offered {
    let Some(t) = ctx.worlds.mover(target) else {
        return Offered::NotApplicable;
    };
    if !t.is_player() || !is_die(t) {
        return Offered::NotApplicable;
    }
    if t.sim.act.state.bits() & flyff_sim::types::OBJSTA_DMG_ALL == OBJSTA_RESURRECTION {
        return Offered::Resurrecting;
    }
    if t.combat.resurrection.is_some() {
        return Offered::AlreadyPending;
    }
    if let Some(t) = ctx.worlds.mover_mut(target) {
        t.combat.resurrection = Some(ResurrectionOffer {
            caster,
            skill,
            level,
        });
    }
    let mut fx = Fx::new();
    fx.out.to_self(pb::ResurrectionMessage {});
    fx.flush_for(svc, ctx, target);
    Offered::Made
}

/// `ResurrectionCancel` (`OnResurrectionCancel`): the offer is dropped.
pub fn on_resurrection_cancel(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        m.combat.resurrection = None;
    }
}

/// `ResurrectionOk` (`OnResurrectionOK`): see the module documentation.
pub fn on_resurrection_ok(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let me = who.0;
    let Some(m) = ctx.worlds.player(me) else {
        return;
    };
    let Some(offer) = m.combat.resurrection else {
        return;
    };
    // revived meanwhile (a scroll, the lodestar): the offer is void
    if !is_die(m) {
        on_resurrection_cancel(ctx, who);
        return;
    }
    // the caster must still be online
    if !ctx
        .worlds
        .mover(offer.caster)
        .is_some_and(|c| c.is_player())
    {
        on_resurrection_cancel(ctx, who);
        return;
    }
    on_resurrection_cancel(ctx, who);
    let core = s.stats.handle();
    let data = std::sync::Arc::clone(ctx.data);
    let Some((prop, add)) = props(&core, offer.skill, offer.level) else {
        return;
    };
    let frame = ctx.clock.frame0;
    // the effects and the resurrection state
    crate::combat::context::with_action_at(s, ctx, me, frame, |cx| {
        cx.arb.fx.near(
            me,
            pb::CreateSfxObj {
                sfx_id: XI_SKILL_ASS_HEAL_RESURRECTION01,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                flag: false,
            },
        );
        cx.arb.fx.near(me, pb::Resurrection {});
        cx.arb.fx.sim.push((me, crate::combat::SimFx::Resurrection));
    });
    // `ApplyParam(caster, prop, add, TRUE, 0)` on the revived player
    let item_core = s.items.handle();
    let env = item_core.env(ctx.clock.unix_ms.to_unix_time());
    let bcx = BuffCx::new(env, ctx.clock.game_ms);
    let caster = ctx
        .worlds
        .mover(offer.caster)
        .map(|c| CasterFacts::of(c, &bcx));
    let mut fx = Fx::new();
    if let (Some(caster), Some(t)) = (caster, ctx.worlds.mover_mut(me)) {
        apply_param(
            t,
            &bcx,
            DstSource::Script { tag: prop.id },
            &caster,
            prop,
            Some(add),
            true,
            0,
            &mut fx,
        );
    }
    fx.flush_for(s, ctx, me);
    // the penalty, softened by the skill level
    if add.dest_param[1] == DST_RECOVERY_EXP {
        let _ =
            crate::stats::death_penalty::apply_for(s, ctx, who, true, add.adj_param_val[1], false);
    }
    let _ = data;
    after_revival(s, ctx, who);
}

/// PL-082 (`DeathHooks::after_revival`): `SetSkillState` for every surviving buff of the player, in key order, to the
/// viewers and the player. `total_ms` is the time the buff has left (the client counts down from the message).
pub fn after_revival(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let now = ctx.clock.game_ms;
    let unix = ctx.clock.unix_ms.to_unix_time().0;
    let Some(m) = ctx.worlds.player(who.0) else {
        return;
    };
    let mut fx = Fx::new();
    for (key, e) in m.buffs.iter().filter(|(_, e)| !e.removed) {
        let item2 = key.kind == BuffKind::Item2;
        fx.out.near(pb::SetSkillState {
            target: who.0.0,
            buff_type: u32::from(key.kind.as_u8()),
            buff_id: u32::from(key.id),
            // the client reads the seconds left of an absolute-time item buff
            level: if item2 {
                i32::try_from(i64::from(e.level) - unix).unwrap_or(0)
            } else {
                e.level as i32
            },
            total_ms: e.snapshot_ms(now),
        });
    }
    fx.flush_for(s, ctx, who.0);
}
