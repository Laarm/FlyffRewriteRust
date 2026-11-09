//! The death EXP penalty and the revival/logout interface (Plan/16 §10, WP-MOVER-10; F-125, KQ-09).
//!
//! **Death changes no EXP.** The penalty is applied when the character is revived (doc 17's handlers call
//! [`apply`]) or, for a character that logs out dead, by [`logout_while_dead`]. The loss is a fraction of the EXP
//! needed for the next level (`DiePenalty.inc`: 0 up to level 20, 6 % up to 29, 5 % up to 59, ...), taken from the
//! current EXP, never below 0; the level never goes down (the level-down flag of the table is forced off) and a
//! level-1 character loses nothing. The first penalty after a clean state records `(death_exp, death_level)`; a
//! level lost this way and regained pays no stat or skill points again (see `progression`).
//!
//! Exemptions (the function returns the HP rate and leaves the EXP alone): killed by a player (`last_pk`), killed
//! in a guild war (`guild_combat`), a lost duel (`last_duel_party`, cleared by the call). `SM_REVIVAL` (the
//! blessing scroll) zeroes the loss and is consumed unless the call is a resurrection; a chaotic player pays 0.9 of
//! the loss instead.

use super::env::{StatsCore, point_param, sm_active};
use super::out::Out;
use super::pool::{set_fp_silent, set_hp_silent, set_mp_silent};
use super::progression::emit_experience;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::world::logout::{LogoutHooks, LogoutStep};
use flyff_common::WorldId;
use flyff_common::math::Vec3;
use flyff_proto::pb;
use flyff_rules::consts::SM_REVIVAL;
use flyff_rules::consts::dst::{DST_FP, DST_MP};
use flyff_rules::progression::{
    death_exp_loss, die_penalty_rates, exp_after_loss, is_after_death as rules_is_after_death,
    next_exp, recovery_exp_adjust,
};
use flyff_rules::stats::c_int;

/// `CMover::IsAfterDeath()`: below the level or EXP of the last death penalty.
pub fn is_after_death(m: &Mover) -> bool {
    m.player().is_some_and(|p| {
        rules_is_after_death(
            m.mv.level,
            p.progression.exp,
            p.progression.death_level,
            p.progression.death_exp,
        )
    })
}

/// `CMover::SubDieDecExp(bTransfer, dwDestParam, bResurrection)` (`Mover.cpp:7265-7340`, 16 §10.2).
///
/// * `transfer`: send `SetExperience` to self when the loss was computed (a revival; the logout does not);
/// * `recovery_exp`: the `DST_RECOVERY_EXP` value of a resurrection skill in percent (0 = none): the loss shrinks to
///   `dec - dec * ((100 - v) / 100)`;
/// * `resurrection`: a resurrection scroll or skill (does not consume `SM_REVIVAL`).
///
/// Returns the **HP rate** of the table (`REVIVAL_PENALTY`: the fraction of the maximum HP the character revives
/// with, 0.8 / 0.6 / 0.5 / 0.4 / 0.3), also when an exemption leaves the EXP alone. A non-player gets 0.
pub fn apply(
    m: &mut Mover,
    c: &StatsCore,
    transfer: bool,
    recovery_exp: u32,
    resurrection: bool,
    out: &mut Out,
) -> f32 {
    if !m.is_player() {
        return 0.0;
    }
    let level = m.mv.level;
    let (rate, mut dec, _level_down) = die_penalty_rates(&c.rules, level);
    let flags = m.player().map(|p| p.death_flags).unwrap_or_default();
    if flags.last_pk || flags.guild_combat {
        return rate;
    }
    if flags.last_duel_party {
        if let Some(p) = m.player_mut() {
            p.death_flags.last_duel_party = false;
        }
        return rate;
    }
    // KQ-09: the level never goes down, whatever the table says
    if dec == 0.0 {
        return rate;
    }
    if !is_after_death(m) {
        let (exp, lvl) = (m.player().map_or(0, |p| p.progression.exp), m.mv.level);
        if let Some(p) = m.player_mut() {
            p.progression.death_exp = exp;
            p.progression.death_level = lvl;
        }
    }
    // `GetDieDecExpRate`
    if recovery_exp != 0 {
        dec = recovery_exp_adjust(dec, recovery_exp);
    }
    let revival = sm_active(m, SM_REVIVAL);
    let chaotic = m.player().is_some_and(|p| p.pk.propensity > 0);
    if chaotic && (revival || resurrection) {
        dec *= 0.9;
    } else if revival {
        dec = 0.0;
    }
    if !resurrection && revival {
        consume_revival(m, out);
    }
    // `DecExperiencePercent`: nothing for a zero fraction
    if dec != 0.0 {
        let loss = death_exp_loss(next_exp(&c.rules, level), dec);
        if let Some(p) = m.player_mut() {
            p.progression.exp = exp_after_loss(level, p.progression.exp, loss);
        }
    }
    if transfer {
        emit_experience(m, out);
    }
    rate
}

/// `SetSMMode(SM_REVIVAL, 0)`: the blessing is used up; `SmMode{SM_REVIVAL, 0}` to self. WP-MOVER-16's `sm::set`
/// takes this over.
fn consume_revival(m: &mut Mover, out: &mut Out) {
    if let Some(p) = m.player_mut() {
        p.sm.timers[SM_REVIVAL] = 0;
    }
    out.to_self(pb::SmMode {
        sm_type: SM_REVIVAL as i32,
        value: 0,
    });
}

/// [`apply`] for a live player, flushing its messages (the call doc 17's revival handlers make). Returns the HP
/// rate; `0.0` when `who` is not a live player.
pub fn apply_for(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    transfer: bool,
    recovery_exp: u32,
    resurrection: bool,
) -> f32 {
    let core = svc.stats.handle();
    let mut out = Out::new();
    let rate = ctx.worlds.player_mut(who.0).map_or(0.0, |m| {
        apply(m, &core, transfer, recovery_exp, resurrection, &mut out)
    });
    out.flush(ctx, who.0);
    rate
}

/// The logout of a dead player (`CUser::DestroyPlayer`, `User.cpp:4079-4096`, 16 §10.3): the penalty without
/// messages, then HP = `(int)(max HP * rate)` and MP/FP raised to `(int)(max * rate)` when lower, stored for the
/// next login (the character logs in alive). The location of the revival point is the world's
/// (`world::logout::destroy_player`).
pub fn logout_while_dead(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let core = svc.stats.handle();
    let mut out = Out::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        let rate = apply(m, &core, false, 0, false, &mut out);
        let hp = c_int(super::env::max_hp(m, &core) as f32 * rate);
        set_hp_silent(m, &core, hp);
        for (dst, max) in [
            (DST_MP, super::env::max_mp(m, &core)),
            (DST_FP, super::env::max_fp(m, &core)),
        ] {
            let v = c_int(max as f32 * rate);
            if point_param(m, &core, dst) < v {
                if dst == DST_MP {
                    set_mp_silent(m, &core, v);
                } else {
                    set_fp_silent(m, &core, v);
                }
            }
        }
    }
    out.flush(ctx, who.0);
}

/// The logout hooks of this document: the `DeadLogout` step runs [`logout_while_dead`]; every other step and
/// predicate goes to the `inner` hooks (the other documents' cleanups), or does nothing without them. Install it
/// with `WorldRules::with_logout`.
#[derive(Default)]
pub struct StatsLogout {
    inner: Option<Box<dyn LogoutHooks>>,
}

impl StatsLogout {
    /// The hooks of this document alone.
    pub fn new() -> StatsLogout {
        StatsLogout { inner: None }
    }

    /// The same, forwarding everything else to `inner`.
    pub fn with_inner(inner: Box<dyn LogoutHooks>) -> StatsLogout {
        StatsLogout { inner: Some(inner) }
    }
}

impl std::fmt::Debug for StatsLogout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatsLogout")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl LogoutHooks for StatsLogout {
    fn step(&mut self, step: LogoutStep, svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
        if step == LogoutStep::DeadLogout {
            logout_while_dead(svc, ctx, p);
        }
        if let Some(inner) = self.inner.as_mut() {
            inner.step(step, svc, ctx, p);
        }
    }

    fn secret_room_position(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> Option<Vec3> {
        self.inner
            .as_ref()
            .and_then(|i| i.secret_room_position(ctx, player))
    }

    fn is_instance_world(&self, world: WorldId) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|i| i.is_instance_world(world))
    }

    fn is_chaotic(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> bool {
        match &self.inner {
            Some(i) => i.is_chaotic(ctx, player),
            None => ctx
                .player(player)
                .and_then(|m| m.player())
                .is_some_and(|p| p.pk.propensity > 0),
        }
    }
}
