//! The two service-context steps around the world phase of a tick (Plan/17 §3, §9; WP-COMBAT-05, -08).
//!
//! The hit frames of the barrier run without the services, so the services' state they read is copied once per
//! tick right before the phase ([`prepare`]), and the deaths they caused wait for the service-context pipeline
//! right after it ([`finish`]). `Game::phase_worlds` calls both.

use super::death::process_pending_at;
use super::world::{ActionConfig, CombatEnv, FixedFactors};
use crate::ctx::{GameCtx, Services};
use crate::event::EventFactors;
use flyff_common::config::EventId;
use flyff_rules::combat::CombatConfig;

/// Refreshes the combat environment snapshot of the world phase: the rule tables, the switches, the element
/// table, the Event.lua ATK / DEF bonuses, kill-steal protection and the monster hit rate.
pub fn prepare(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let events = s.event.factors();
    let factors = FixedFactors {
        attack: events.lua_attack(),
        defense: events.lua_defense(),
    };
    let env = CombatEnv {
        core: s.stats.handle(),
        cfg: CombatConfig::from_config(&ctx.cfg.rules),
        act: ActionConfig::from_config(&ctx.cfg.rules).with_events(ctx.live),
        attr: std::sync::Arc::clone(s.combat.attr.handle(ctx.data)),
        factors,
        steal_protection: ctx.live.event_state(EventId::Steal) == 0,
        monster_hit_rate: ctx.live.rates.monster_hit.unwrap_or(1.0),
        pvp: super::pvp::PvpEnv::from_services(s, ctx),
    };
    ctx.worlds.movement.combat.env = Some(env);
}

/// Runs the skill hits and the deaths the world phase queued: their sim effects start at the first frame of the next tick.
pub fn finish(s: &mut Services, ctx: &mut GameCtx<'_>) {
    // the skill hits of the phase (WP-SKILL-05) first: they may kill
    if !ctx.worlds.movement.combat.queues.skill_jobs.is_empty() {
        crate::skill::cast::process_pending(s, ctx);
    }
    // the ground DoTs and Psychic Walls (WP-SKILL-06) strike in the same step
    crate::skill::ctrl::process(s, ctx);
    if !ctx.worlds.movement.combat.queues.deaths.is_empty() {
        let frame = ctx.clock.tick.frames_completed_after();
        process_pending_at(s, ctx, frame);
    }
    // the duels (WP-COMBAT-13): countdown, idle timeout, region rule, vanished opponent
    super::duel::process(s, ctx);
}
