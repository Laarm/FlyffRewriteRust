//! Slot item 11 (Plan/10 §5.4, 21 §6.3; `CGuildWarMng::Process`, `guildwar.cpp` and `CDPCoreSrvr::OnWarTimeout`):
//! once per second while `events.guild_war` is on.

use super::model::{GuildWar, WarResult, WarRole};
use super::result::{end_war, finish};
use super::{COUNTER_PERSIST_MS, GuildWarService};
use crate::ctx::{GameCtx, Services};
use flyff_common::GuildWarId;
use flyff_db::DbOp;

/// One pass over the running wars in id order:
///
/// 1. the orphans (stored wars that could not be loaded) and the wars of a guild that no longer exists end as a
///    draw, points untouched, so that no running row and no lock outlives a guild (an extension of the plan);
/// 2. a war whose two hours are over (`now_s >= started_at + 7200`, `now_s = clock.unix_ms / 1000`) ends by the timeout
///    rule: the master who was absent longer loses, equal: the side with more deaths loses, equal: a draw. In the
///    second of the timeout nobody gets an increment;
/// 3. otherwise every side whose master is not online in the directory gets `absent_s + 1` (once per SECOND and war,
///    FIX of the per-world counting, ref/14 B18);
/// 4. every war with changed counters and at least [`COUNTER_PERSIST_MS`] since its last write gets one
///    `UpsertGuildWar` (a crash loses at most that, 13 §8).
///
/// A war older than two hours ends at the first pass after the event was switched on again (m26).
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    sweep_orphans(svc, ctx);
    let now_s = ctx.clock.unix_ms.0.div_euclid(1000);
    for id in svc.social.guild_war.ids() {
        let Some(war) = svc.social.guild_war.get(id) else {
            continue;
        };
        if WarRole::BOTH
            .into_iter()
            .any(|r| svc.social.guild.get(war.guild(r)).is_none())
        {
            end_war(svc, ctx, id, WarResult::Draw);
            continue;
        }
        if war.timed_out(now_s) {
            let result = war.timeout_result();
            end_war(svc, ctx, id, result);
            continue;
        }
        count_absence(svc, id);
    }
    persist_counters(svc, ctx);
}

/// Closes the stored wars that could not be loaded.
fn sweep_orphans(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if svc.social.guild_war.orphans().is_empty() {
        return;
    }
    for r in svc.social.guild_war.take_orphans() {
        finish(svc, ctx, GuildWar::from_record(&r), WarResult::Draw);
    }
}

/// `absent_s + 1` for every side whose master is not online (a guild without a master counts as absent).
fn count_absence(svc: &mut Services, id: GuildWarId) {
    let absent: Vec<WarRole> = {
        let Some(war) = svc.social.guild_war.get(id) else {
            return;
        };
        WarRole::BOTH
            .into_iter()
            .filter(|r| {
                !svc.social
                    .guild
                    .get(war.guild(*r))
                    .and_then(|g| g.master())
                    .is_some_and(|m| svc.presence.get(m).is_some_and(|e| e.online.is_some()))
            })
            .collect()
    };
    if absent.is_empty() {
        return;
    }
    if let Some(war) = svc.social.guild_war.get_mut(id) {
        for role in absent {
            let side = war.side_mut(role);
            side.absent_s = side.absent_s.saturating_add(1);
        }
        war.dirty = true;
    }
}

/// One `UpsertGuildWar` for every dirty war that was not written for [`COUNTER_PERSIST_MS`].
fn persist_counters(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let due =
        |w: &GuildWar| w.dirty && now.0.saturating_sub(w.last_persist.0) >= COUNTER_PERSIST_MS;
    let ids: Vec<GuildWarId> = svc
        .social
        .guild_war
        .running()
        .filter(|w| due(w))
        .map(|w| w.id)
        .collect();
    for id in ids {
        if let Some(war) = svc.social.guild_war.get_mut(id) {
            ctx.persist.op(DbOp::UpsertGuildWar(war.record()));
            war.dirty = false;
            war.last_persist = now;
        }
    }
}

impl GuildWarService {
    /// `true` when some war has counters that were not written yet (tests, the shutdown view).
    pub fn has_dirty(&self) -> bool {
        self.running().any(|w| w.dirty)
    }
}
