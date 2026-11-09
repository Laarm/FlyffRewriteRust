//! Guild buffs (Plan/21 §5.11; WP-SOCIAL-14; `features.guild_buffs`; KQ-39 d; `CGuildMng::AddBuffs`,
//! `_Common/guild.cpp:1162-1229`, `User.cpp:441-447`).
//!
//! **The pass.** [`process_all`] runs every tick (tick phase 5, after the abnormal-state pass) and costs one walk over
//! the sessions plus, per member that lacks the buff, one walk over its guild: for every online guild member
//! WITHOUT any `IK3_GUILD_BUFF` buff it applies, in file order, every row of `GameData.guild.buffs` (`guildbuff.inc`:
//! `II_BUFF_GUILD_BUFF_01..04` at guild levels 5 / 15 / 30 / 45) whose constraints hold: `min_level <= guild.level`,
//! `member_count <= members`, `member_online <= online members` (0 = no constraint), through
//! [`buff::api::apply_item`]. A running buff is never refreshed by a level-up (KEEP, KQ-39 d: the higher row applies
//! only after the old buff expired; the suggestion of ref/14 §5.7 to re-evaluate on a level change is NOT followed).
//! It is re-applied the tick after it expired.
//!
//! **Removal** on leave, kick, disband and character deletion is the detach path of `commit.rs`
//! (`remove_by_ik3(IK3_GUILD_BUFF)`); [`remove`] is the same one-liner. FIX (not in the plan): [`strip_stale`] runs at
//! the join and strips a persisted guild buff from a player without a guild (a member kicked while offline would keep
//! the buff for up to 11.6 days, 999 999 999 ms).

use crate::buff;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::defs::itemkind::IK3_GUILD_BUFF;
use flyff_common::{GuildId, ObjId};
use smallvec::SmallVec;
use std::collections::BTreeMap;
use std::sync::Arc;

/// `RemoveIk3Buffs(IK3_GUILD_BUFF)` of one avatar.
pub fn remove(svc: &mut Services, ctx: &mut GameCtx<'_>, obj: ObjId) {
    buff::api::remove_by_ik3(svc, ctx, obj, IK3_GUILD_BUFF);
}

/// The join FIX: a player without a guild loses a guild buff that it carried over from its last session.
pub fn strip_stale(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let has_guild = ctx
        .player(p)
        .and_then(|m| m.player())
        .is_some_and(|pl| pl.social.guild.is_some());
    if !has_guild && buff::api::has_ik3(ctx, p.0, IK3_GUILD_BUFF) {
        remove(svc, ctx, p.0);
    }
}

/// The per-tick pass of the tick loop (phase 5): see the module documentation.
pub fn process_all(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let _ = pass(svc, ctx);
}

/// What one pass looked at: the online guild members it examined and the applications it made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PassStats {
    /// Online guild members whose buffs were looked at (the cost of the pass).
    pub examined: usize,
    /// `apply_item` calls.
    pub applied: usize,
}

/// One pass; the counters are for the suites.
pub(crate) fn pass(svc: &mut Services, ctx: &mut GameCtx<'_>) -> PassStats {
    let mut stats = PassStats::default();
    if !ctx.cfg.rules.features.guild_buffs
        || ctx.data.guild.buffs.is_empty()
        || svc.social.guild.is_empty()
    {
        return stats;
    }
    let mut want: SmallVec<[(ObjId, GuildId); 8]> = SmallVec::new();
    for e in ctx.sessions.iter() {
        let Some(guild) = ctx
            .worlds
            .player(e.player)
            .and_then(|m| m.player())
            .and_then(|p| p.social.guild)
        else {
            continue;
        };
        stats.examined += 1;
        if !buff::api::has_ik3(ctx, e.player, IK3_GUILD_BUFF) {
            want.push((e.player, guild));
        }
    }
    if want.is_empty() {
        return stats;
    }
    let data = Arc::clone(ctx.data);
    // (members, online members) of a guild, counted once per pass
    let mut counts: BTreeMap<GuildId, (i64, i64)> = BTreeMap::new();
    for (obj, guild) in want {
        let Some(g) = svc.social.guild.get(guild) else {
            continue;
        };
        let level = i64::from(g.level);
        let (members, online) = *counts.entry(guild).or_insert_with(|| {
            (
                i64::try_from(g.members.len()).unwrap_or(i64::MAX),
                i64::try_from(svc.social.guild.online_objs(&svc.presence, guild).len())
                    .unwrap_or(i64::MAX),
            )
        });
        for row in &data.guild.buffs {
            if i64::from(row.min_level) <= level
                && i64::from(row.member_count) <= members
                && i64::from(row.member_online) <= online
            {
                stats.applied += 1;
                if let Err(e) = buff::api::apply_item(svc, ctx, obj, row.item, None) {
                    tracing::debug!(?e, item = row.item, "a guild buff was not applied");
                }
            }
        }
    }
    stats
}
