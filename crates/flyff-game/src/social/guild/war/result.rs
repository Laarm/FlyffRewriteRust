//! The end of a war (Plan/21 §6.4; `CGuildWarMng::Result`, `_Common/guildwar.cpp:165-330`): the win points, the
//! clean-up of both guilds and of every member, the announcement and ONE end batch.

use super::model::{GuildWar, WarResult, WarRole};
use super::{PK_LIMIT_S, on_pk_limit, wire};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::guild::commit::{Origin, avatar_of, emit, now, submit};
use flyff_common::{CharacterId, GuildId, GuildWarId};
use flyff_db::audit::social as audit;
use flyff_db::{AtomicBatch, AtomicKind, DbOp};
use flyff_rules::social::war_points;

/// Ends the running war `id` with `result` (a no-op for a war that is not running), in ONE commit block and this
/// order:
///
/// 0. the war leaves the service first (nothing can reach it again);
/// 1. a decided result (`< Truce`) applies [`war_points`] to the win points of both guilds and counts the win and the
///    loss (`Guild.war_surrenders` is never touched, KEEP); a truce and a draw change nothing;
/// 2. both guilds lose `war` / `enemy`, every member his `war_id`; every online member (the declarer's guild first,
///    `CharacterId` order) gets `PlayerSocial.war = 0`, `SetWar{0}` near him and the 10 s PK limit
///    ([`on_pk_limit`], a seam);
/// 3. `WarEnd{id_war, points_decl, points_acpt, result}` to every player in a world (the current points for a truce
///    and a draw);
/// 4. ONE `GuildWarEnd` batch: the final row with the EXACT `WR_*` code (FIX: the original stored the timeout results
///    as `*_GN`), both guild rows and `SetGuildWarOfMembers(.., 0)`;
/// 5. the audit line `guild.war_end`.
pub fn end_war(svc: &mut Services, ctx: &mut GameCtx<'_>, id: GuildWarId, result: WarResult) {
    let Some(war) = svc.social.guild_war.remove(id) else {
        return;
    };
    finish(svc, ctx, war, result);
}

/// [`end_war`] over a war that is already out of the service (the orphans of the load): a guild that no longer
/// exists, or fights another war, is left alone in every step.
pub(super) fn finish(svc: &mut Services, ctx: &mut GameCtx<'_>, war: GuildWar, result: WarResult) {
    let id = war.id;
    let fights =
        |svc: &Services, g: GuildId| svc.social.guild.get(g).is_some_and(|g| g.war == Some(id));
    let mut fighting = [false; 2];
    for (i, role) in WarRole::BOTH.into_iter().enumerate() {
        fighting[i] = fights(svc, war.guild(role));
    }
    let [decl_fights, acpt_fights] = fighting;

    // ---- 1. the points ----
    if let Some(winner) = result.winner() {
        let (wg, lg) = (war.guild(winner), war.guild(winner.other()));
        if decl_fights && acpt_fights {
            let pts =
                |svc: &Services, g: GuildId| svc.social.guild.get(g).map_or(0, |g| g.war_points);
            let p = war_points(pts(svc, wg), pts(svc, lg));
            if let Some(g) = svc.social.guild.get_mut(wg) {
                g.war_points = p.winner;
                g.war_wins = g.war_wins.saturating_add(1);
            }
            if let Some(g) = svc.social.guild.get_mut(lg) {
                g.war_points = p.loser;
                g.war_losses = g.war_losses.saturating_add(1);
            }
        }
    }

    // ---- 2. the clean-up, the declarer's guild first ----
    for (role, fights) in WarRole::BOTH.into_iter().zip(fighting) {
        if !fights {
            continue;
        }
        let gid = war.guild(role);
        let members: Vec<CharacterId> = match svc.social.guild.get_mut(gid) {
            Some(g) => {
                g.war = None;
                g.enemy = None;
                for m in g.members.values_mut() {
                    m.war_id = None;
                }
                g.members.keys().copied().collect()
            }
            None => continue,
        };
        for c in members {
            let Some(obj) = avatar_of(svc, c) else {
                continue;
            };
            svc.social.guild.sync_member(&svc.presence, ctx, c);
            wire::set_war_near(svc, ctx, obj, 0);
            on_pk_limit(ctx, PlayerRef(obj), PK_LIMIT_S);
        }
    }

    // an orphan (a guild is gone or fights another war): avatars of the vanished roster may still carry the flag
    if !(decl_fights && acpt_fights) {
        clear_stray_flags(svc, ctx, id);
    }

    // ---- 3. the announcement ----
    let points =
        |svc: &Services, role: WarRole| svc.social.guild.get(war.guild(role)).map(|g| g.war_points);
    let (pd, pa) = (points(svc, WarRole::Decl), points(svc, WarRole::Acpt));
    wire::ended(
        svc,
        ctx,
        id,
        pd.unwrap_or_default(),
        pa.unwrap_or_default(),
        result.code(),
    );

    // ---- 4. the batch ----
    let row = war.ended_record(result, now(ctx), pd, pa);
    let guild_row = |svc: &Services, g: GuildId| svc.social.guild.get(g).map(|g| g.to_record());
    let batch = match (
        decl_fights
            .then(|| guild_row(svc, war.decl.guild))
            .flatten(),
        acpt_fights
            .then(|| guild_row(svc, war.acpt.guild))
            .flatten(),
    ) {
        (Some(d), Some(a)) => AtomicBatch::guild_war_end(row, d, a),
        (d, a) => {
            // a guild is gone (or fights another war): only what still belongs to this war is written
            let reset: Vec<GuildId> = [(d.as_ref(), war.decl.guild), (a.as_ref(), war.acpt.guild)]
                .into_iter()
                .filter_map(|(r, g)| r.map(|_| g))
                .collect();
            let mut ops = vec![DbOp::UpsertGuildWar(row)];
            ops.extend(d.into_iter().chain(a).map(DbOp::UpsertGuild));
            if let Some(first) = reset.first().copied() {
                ops.push(DbOp::SetGuildWarOfMembers {
                    guilds: [first, reset.get(1).copied().unwrap_or(first)],
                    war_id: 0,
                });
            }
            AtomicBatch::of_ops(AtomicKind::Other("GuildWarEnd"), ops)
        }
    };
    submit(svc, ctx, "GuildWarEnd", batch, &[], None);

    // ---- 5. the audit ----
    emit(
        ctx,
        Origin::System,
        audit::guild_war_end(
            id,
            war.decl.guild,
            war.acpt.guild,
            result.code(),
            pd.unwrap_or_default(),
            pa.unwrap_or_default(),
        ),
    );
}

/// Clears the war flag of every online avatar that still carries `id` (only the abnormal end of a war whose guild
/// vanished needs it: the roster that would name them is gone), with `SetWar{0}` near each and the PK limit.
fn clear_stray_flags(svc: &Services, ctx: &mut GameCtx<'_>, id: GuildWarId) {
    let strays: Vec<flyff_common::ObjId> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|o| {
            ctx.worlds
                .player(*o)
                .and_then(|m| m.player())
                .is_some_and(|p| p.social.war == id.0)
        })
        .collect();
    for obj in strays {
        if let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) {
            p.social.war = 0;
        }
        wire::set_war_near(svc, ctx, obj, 0);
        on_pk_limit(ctx, PlayerRef(obj), PK_LIMIT_S);
    }
}
