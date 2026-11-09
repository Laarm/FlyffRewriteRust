//! The line-up (Plan/21 §8.4; WP-SOCIAL-25; `GuildCombat1to1.cpp:1485-1589`, `DPSrvr.cpp:10655-10688`): the window the
//! master or a kingpin of a tendering guild opens during the line-up time and the submission of the fighters in
//! fight order.
//!
//! A submission is validated as a whole and REPLACES the stored line-up (`ReplaceGc1to1Lineup`, the order is the fight
//! order). FIX (T22, as the siege's VR-049): every id must be a CURRENT member of the sender's guild and no id may
//! appear twice (the database refuses a character twice), both silently as the original had no such text.

use super::{Gc1to1State, character_of, defined, enabled, send_one, submit_logged};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::guild::GuildRank;
use crate::social::siege::fight::live_avatar;
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT1TO1_CANNOT_MAKEUP, TID_GAME_GUILDCOMBAT1TO1_HAVENOT_MASTER,
    TID_GAME_GUILDCOMBAT1TO1_LIMIT_LEVEL_NOTICE, TID_GAME_GUILDCOMBAT1TO1_LINEUPCOMPLETE,
    TID_GAME_GUILDCOMBAT1TO1_LOWMINPLAYER, TID_GAME_GUILDCOMBAT1TO1_NOLINEUP,
    TID_GAME_GUILDCOMBAT1TO1_NOLINEUPAUTH,
};
use flyff_common::{CharacterId, GuildId};
use flyff_db::{AtomicBatch, AtomicKind, DbOp};
use flyff_proto::pb;
use serde_json::json;
use std::collections::BTreeSet;

/// The rank of `c` when it is Master or Kingpin (`true`) of its guild.
fn may_line_up(svc: &Services, c: CharacterId) -> bool {
    svc.social
        .guild
        .guild_record_of(c)
        .and_then(|g| g.member(c))
        .is_some_and(|m| matches!(m.rank, GuildRank::Master | GuildRank::Kingpin))
}

/// The guild of `c` when it is in the tender list (`GetTenderGuildIndexByUser`).
fn tendering_guild(svc: &Services, c: CharacterId) -> Option<GuildId> {
    svc.social
        .guild
        .guild_of(c)
        .filter(|g| svc.social.gc1to1.tender_of(*g).is_some())
}

/// `Gc1to1MemberLineUpOpenWndReq` (`SendMemberLineUpOpenWnd`, `1485-1516`), in this order: state not Open (`NOLINEUP`);
/// the guild is no tenderer (`CANNOT_MAKEUP`); neither Master nor Kingpin (`NOLINEUPAUTH`); else
/// `Gc1to1MemberLineUpOpenWndNtf{min_level, min_players (after the override), max_players, ids (stored)}`.
pub fn on_open_wnd_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Open {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_NOLINEUP, Vec::new());
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let Some(guild) = tendering_guild(svc, c) else {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_CANNOT_MAKEUP,
            Vec::new(),
        );
        return;
    };
    if !may_line_up(svc, c) {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_NOLINEUPAUTH,
            Vec::new(),
        );
        return;
    }
    let cfg = &svc.social.gc1to1.config;
    let msg = pb::Gc1to1MemberLineUpOpenWndNtf {
        min_level: cfg.min_join_player_level,
        min_players: cfg.min_join_player,
        max_players: cfg.max_join_player,
        ids: svc
            .social
            .gc1to1
            .tender_of(guild)
            .map(|t| t.lineup.iter().map(|m| m.0).collect())
            .unwrap_or_default(),
    };
    send_one(ctx, who.0, msg);
}

/// `Gc1to1MemberLineUp{ids}` (`DPSrvr.cpp:10665-10688`, `SetMemberLineUp` `1519-1589`), in this order, each refusal
/// changes nothing: more ids than `MaxJoinPlayer` (silent, the handler's pre-check); state not Open (`NOLINEUP`); the
/// guild is no tenderer (`CANNOT_MAKEUP`); fewer ids than `MinJoinPlayer` (`LOWMINPLAYER` with the minimum); the sender is
/// neither Master nor Kingpin (`NOLINEUPAUTH`); FIX (silent): an id that is not a current member of the sender's guild
/// or an id twice; an ONLINE listed member below `MinJoinPlayerLevel` (`LIMIT_LEVEL_NOTICE`); no ONLINE listed Master or
/// Kingpin (`HAVENOT_MASTER`; offline listed members pass both checks, KEEP).
///
/// Then the line-up is replaced in submit order (= fight order) by ONE `AtomicBatch::of_ops(Other("Gc1to1Lineup"),
/// [ReplaceGc1to1Lineup])`, the sender gets `LINEUPCOMPLETE` and the audit line `gc1to1.lineup` is written.
pub fn on_member_line_up(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, ids: Vec<u32>) {
    if !enabled(ctx) {
        return;
    }
    let (min, max, min_level) = {
        let c = &svc.social.gc1to1.config;
        (
            c.min_join_player,
            c.max_join_player,
            c.min_join_player_level,
        )
    };
    let len = i32::try_from(ids.len()).unwrap_or(i32::MAX);
    if len > max {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Open {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_NOLINEUP, Vec::new());
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let Some(guild) = tendering_guild(svc, c) else {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_CANNOT_MAKEUP,
            Vec::new(),
        );
        return;
    };
    if len < min {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_LOWMINPLAYER,
            vec![min.to_string()],
        );
        return;
    }
    if !may_line_up(svc, c) {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_NOLINEUPAUTH,
            Vec::new(),
        );
        return;
    }
    // FIX (T22): current members, each once
    let members: Vec<CharacterId> = ids.iter().copied().map(CharacterId).collect();
    let mut seen = BTreeSet::new();
    let current = svc.social.guild.get(guild);
    if members
        .iter()
        .any(|m| !seen.insert(*m) || current.is_none_or(|g| g.member(*m).is_none()))
    {
        return;
    }
    // the level of the online listed members and the master or kingpin among them
    let mut leader = false;
    for m in &members {
        let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, *m) else {
            continue;
        };
        if ctx
            .worlds
            .mover(obj)
            .is_some_and(|mv| mv.mv.level < min_level)
        {
            defined(
                ctx,
                who.0,
                TID_GAME_GUILDCOMBAT1TO1_LIMIT_LEVEL_NOTICE,
                Vec::new(),
            );
            return;
        }
        leader |= current
            .and_then(|g| g.member(*m))
            .is_some_and(|s| matches!(s.rank, GuildRank::Master | GuildRank::Kingpin));
    }
    if !leader {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_HAVENOT_MASTER,
            Vec::new(),
        );
        return;
    }

    // ---- the commit block ----
    let round = {
        let Some(t) = svc
            .social
            .gc1to1
            .tenders
            .iter_mut()
            .find(|t| t.guild == guild)
        else {
            return; // unreachable: `tendering_guild` held above
        };
        t.lineup.clone_from(&members);
        t.round
    };
    submit_logged(
        ctx,
        "Gc1to1Lineup",
        AtomicBatch::of_ops(
            AtomicKind::Other("Gc1to1Lineup"),
            vec![DbOp::ReplaceGc1to1Lineup {
                round,
                guild_id: guild,
                members: members.clone(),
            }],
        ),
    );
    defined(
        ctx,
        who.0,
        TID_GAME_GUILDCOMBAT1TO1_LINEUPCOMPLETE,
        Vec::new(),
    );
    crate::audit::emit_for(
        ctx,
        who,
        "gc1to1.lineup",
        json!({
            "guild": guild.0,
            "round": round,
            "members": members.iter().map(|m| m.0).collect::<Vec<_>>(),
        }),
    );
}
