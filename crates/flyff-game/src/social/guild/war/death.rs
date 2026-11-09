//! Deaths in a war (Plan/21 §6.3; `CDPCoreSrvr::OnWarDead`, `DPCoreSrvr.cpp:1623-1676`): the `GuildWar` arm of
//! [`crate::social::hooks::on_player_killed`].

use super::end_war;
use super::model::WarResult;
use super::wire;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::guild::character_of;
use flyff_common::GuildWarId;
use flyff_proto::pb;

/// A member of one guild killed a member of the other in a war (`SubWar`, called by combat only while
/// `events.guild_war` is on and the guilds differ). The server re-validates everything: the victim fights a war
/// (`PlayerSocial.war` is not 0; a surrendered member is NOT a war death), the war runs, the killer fights the SAME
/// war and the two belong to the two guilds of it; anything else is ignored.
///
/// * The victim is the master of his guild: the war ends at once and the other side wins
///   ([`WarResult::DeclGn`] when the acceptor's master died, [`WarResult::AcptGn`] when the declarer's did).
/// * Otherwise his side counts one more death (persisted with the counters within 10 s) and the online members of
///   both guilds (the declarer's first) get `WarDead{id_war, player_name, decl}`.
///
/// A kill scores nothing by itself: the points are the win points of the result.
pub fn on_war_death(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    killer: PlayerRef,
    victim: PlayerRef,
) {
    let Some((v_char, v_name, v_war)) = ctx
        .player(victim)
        .and_then(|m| m.player())
        .map(|p| (p.ident.character_id, p.ident.name.clone(), p.social.war))
    else {
        return;
    };
    let k_war = ctx
        .player(killer)
        .and_then(|m| m.player())
        .map_or(0, |p| p.social.war);
    let k_char = character_of(ctx, killer);
    if v_war == 0 || k_war != v_war {
        return;
    }
    let id = GuildWarId(v_war);
    let guilds = &svc.social.guild;
    let (Some(v_guild), Some(k_guild)) = (
        guilds.guild_of(v_char),
        k_char.and_then(|c| guilds.guild_of(c)),
    ) else {
        return;
    };
    let Some(war) = svc.social.guild_war.get(id) else {
        return;
    };
    let Some(role) = war.role_of(v_guild) else {
        return;
    };
    if v_guild == k_guild || war.role_of(k_guild) != Some(role.other()) {
        return;
    }
    let (decl, acpt) = (war.decl.guild, war.acpt.guild);
    // the master of his guild: the war is decided on the spot
    if guilds.get(v_guild).and_then(|g| g.master()) == Some(v_char) {
        end_war(svc, ctx, id, WarResult::master_killed(role.other()));
        return;
    }
    if let Some(war) = svc.social.guild_war.get_mut(id) {
        let side = war.side_mut(role);
        side.deaths = side.deaths.saturating_add(1);
        war.dirty = true;
    }
    wire::to_both_guilds(
        svc,
        ctx,
        decl,
        acpt,
        pb::WarDead {
            id_war: id.0,
            player_name: v_name,
            decl: role.is_decl(),
        },
    );
}
