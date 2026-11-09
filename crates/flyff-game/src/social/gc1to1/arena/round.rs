//! The rounds of an arena (WP-SOCIAL-26; `GuildCombat1to1.cpp:125-229`, `390-613`): the wait before a round, the round, the
//! result check, the finish, the death hook and the logout rule.

use super::close::{Awards, close_wait, flush_awards, give_chips};
use super::{
    arena_clone, arena_mut, present, present_kth, present_members, send_state_all, to_wait_stage,
    to_war_stage,
};
use crate::combat::hooks::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::gc1to1::{
    Arena, ArenaState, PlayerResult, defined, enabled, send_one, small_caption, text_of, text_with,
};
use crate::social::siege::fight::{live_avatar, name_of};
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT_1TO1_WARDRAW, TID_GAME_GUILDCOMBAT_1TO1_WARLOST,
    TID_GAME_GUILDCOMBAT_1TO1_WARWIN, TID_GAME_GUILDCOMBAT1TO1_DRAWMSG,
    TID_GAME_GUILDCOMBAT1TO1_WARNOOTHER, TID_GAME_GUILDCOMBAT1TO1_WINMSG,
};
use flyff_common::{CharacterId, GameMs, ObjId};
use flyff_proto::pb;
use flyff_rules::social::gc1to1::win_chips;
use serde_json::json;

/// `SendWarResultAllPlayer` at the start of a round: `Gc1to1WarResult{201, own k-th id (0 beyond the line-up), 0, 0}`.
pub(super) fn send_war_start_all(svc: &Services, ctx: &mut GameCtx<'_>, a: &Arena) {
    for (side, _, obj) in present_members(svc, ctx, a) {
        send_one(
            ctx,
            obj,
            pb::Gc1to1WarResult {
                state: PlayerResult::War.code(),
                id_player: a.kth(side).map_or(0, |c| c.0),
                win_count0: 0,
                win_count1: 0,
            },
        );
    }
}

/// `SendWarResultAllPlayer` at the end of a round: per present line-up member the RECIPIENT's view
/// `Gc1to1WarResult{own side's last, own side's k-th id (0 beyond), OWN wins, OTHER wins}` and the small caption
/// `DRAWMSG [round]` or `WINMSG [round, winner guild, winner name]` (only while the winner's avatar exists).
pub(super) fn send_round_result_all(svc: &Services, ctx: &mut GameCtx<'_>, a: &Arena) {
    let round = (a.prog + 1).to_string();
    let winner_side = if a.side(0).last == PlayerResult::Win {
        Some(0)
    } else if a.side(1).last == PlayerResult::Win {
        Some(1)
    } else {
        None
    };
    // the winner of the round: his guild and name, while his avatar exists
    let winner = winner_side.and_then(|s| {
        let c = a.kth(s)?;
        live_avatar(&svc.presence, &*ctx.worlds, c)?;
        let guild = svc.social.guild.guild_record_of(c)?.name.clone();
        Some((guild, name_of(svc, c)?))
    });
    for (side, _, obj) in present_members(svc, ctx, a) {
        let other = 1 - side.min(1);
        send_one(
            ctx,
            obj,
            pb::Gc1to1WarResult {
                state: a.side(side).last.code(),
                id_player: a.kth(side).map_or(0, |c| c.0),
                win_count0: a.side(side).wins,
                win_count1: a.side(other).wins,
            },
        );
        match (&winner_side, &winner) {
            (None, _) => small_caption(
                ctx,
                obj,
                TID_GAME_GUILDCOMBAT1TO1_DRAWMSG,
                vec![round.clone()],
            ),
            (Some(_), Some((guild, name))) => small_caption(
                ctx,
                obj,
                TID_GAME_GUILDCOMBAT1TO1_WINMSG,
                vec![round.clone(), guild.clone(), name.clone()],
            ),
            (Some(_), None) => {}
        }
    }
}

/// `GuildCombat1to1WarWait` (`390-437`): all rounds done (`prog >= total` or `prog >= MaxJoinPlayer`) -> [`close_wait`];
/// both k-th fighters present -> state Wait for `WarWaitTime` and the state to all; else the result check decides at once
/// (an absent fighter loses, both absent draw).
pub(super) fn war_wait(svc: &mut Services, ctx: &mut GameCtx<'_>, idx: usize) {
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    let max = usize::try_from(svc.social.gc1to1.config.max_join_player).unwrap_or(0);
    if a.prog >= a.total || a.prog >= max {
        close_wait(svc, ctx, idx, false);
        return;
    }
    if present_kth(svc, ctx, &a, 0, None).is_some() && present_kth(svc, ctx, &a, 1, None).is_some()
    {
        let now = ctx.clock.game_ms;
        let wait = svc.social.gc1to1.config.war_wait_time_ms;
        if let Some(m) = arena_mut(svc, idx) {
            m.state = ArenaState::Wait;
            m.due = GameMs(now.0.saturating_add(wait));
        }
        if let Some(a) = arena_clone(svc, idx) {
            send_state_all(svc, ctx, &a);
        }
    } else {
        result_check(svc, ctx, idx, None);
    }
}

/// `GuildCombat1to1War` (`440-467`): both k-th fighters present -> the audit line `gc1to1.round`, both to their fight
/// spot with state 1, state War for `WarTime`, the state to all and `Gc1to1WarResult{201, own k-th id, 0, 0}` to all;
/// else [`war_wait`] again.
pub(super) fn war(svc: &mut Services, ctx: &mut GameCtx<'_>, idx: usize) {
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    let (Some(o0), Some(o1)) = (
        present_kth(svc, ctx, &a, 0, None),
        present_kth(svc, ctx, &a, 1, None),
    ) else {
        war_wait(svc, ctx, idx);
        return;
    };
    audit_round(ctx, svc.social.gc1to1.round(), &a, "start");
    to_war_stage(ctx, &a, 0, o0);
    to_war_stage(ctx, &a, 1, o1);
    let now = ctx.clock.game_ms;
    let war_time = svc.social.gc1to1.config.war_time_ms;
    if let Some(m) = arena_mut(svc, idx) {
        m.state = ArenaState::War;
        m.due = GameMs(now.0.saturating_add(war_time));
    }
    if let Some(a) = arena_clone(svc, idx) {
        send_state_all(svc, ctx, &a);
        send_war_start_all(svc, ctx, &a);
    }
}

/// The audit line of a round: `gc1to1.round{world, round, ids, result}` with `result` = `start`, `W0`, `W1` or `D`.
fn audit_round(ctx: &GameCtx<'_>, round: u32, a: &Arena, result: &str) {
    ctx.audit.emit(flyff_db::AuditEvent::new(
        "gc1to1.round",
        json!({
            "world": a.world.0,
            "round": round,
            "prog": a.prog,
            "ids": [a.kth(0).map_or(0, |c| c.0), a.kth(1).map_or(0, |c| c.0)],
            "result": result,
        }),
    ));
}

/// `GuildCombat1to1WarResultCheck` (`470-553`); `leaver` (the logout of a fighter) counts as absent.
///
/// Both k-th fighters present: a side with 0 lives loses; otherwise only when the timer ran out: more lives wins, equal
/// lives draw (otherwise nothing happens, the round goes on). Exactly one present: it wins and hears `WARNOOTHER`. None
/// present: a draw.
pub(crate) fn result_check(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    idx: usize,
    leaver: Option<CharacterId>,
) {
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    let now = ctx.clock.game_ms;
    let p0 = present_kth(svc, ctx, &a, 0, leaver);
    let p1 = present_kth(svc, ctx, &a, 1, leaver);
    let (winner, no_other) = match (p0, p1) {
        (Some(_), Some(_)) => {
            if a.side(0).lives == 0 {
                (Some(1), false)
            } else if a.side(1).lives == 0 {
                (Some(0), false)
            } else if now >= a.due {
                match a.side(0).lives.cmp(&a.side(1).lives) {
                    std::cmp::Ordering::Greater => (Some(0), false),
                    std::cmp::Ordering::Less => (Some(1), false),
                    std::cmp::Ordering::Equal => (None, false),
                }
            } else {
                return;
            }
        }
        (Some(_), None) => (Some(0), true),
        (None, Some(_)) => (Some(1), true),
        (None, None) => (None, false),
    };
    apply_result(svc, ctx, idx, winner, [p0, p1], no_other);
}

/// The result of a round (`SetLastWinState`, `556-591`, and `SetWinChip`, `255-314`): `wins += 1` for the winner, `last`
/// of both sides, both lives back to `PlayerLife`, state Finish, the audit line; a draw says `WARDRAW` to each present
/// k-th fighter, otherwise the present winner gets the chips (`WARWIN [chips]`) and the present loser `WARLOST`.
pub(super) fn apply_result(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    idx: usize,
    winner: Option<usize>,
    present: [Option<ObjId>; 2],
    no_other: bool,
) {
    let life = svc.social.gc1to1.config.player_life;
    let round = svc.social.gc1to1.round();
    let Some(m) = arena_mut(svc, idx) else {
        return;
    };
    if let Some(w) = winner {
        m.side_mut(w).wins += 1;
    }
    for i in 0..2 {
        let s = m.side_mut(i);
        s.lives = life;
        s.last = match winner {
            Some(w) if w == i => PlayerResult::Win,
            Some(_) => PlayerResult::Lose,
            None => PlayerResult::Draw,
        };
    }
    m.state = ArenaState::Finish;
    let a = m.clone();
    audit_round(
        ctx,
        round,
        &a,
        match winner {
            Some(0) => "W0",
            Some(_) => "W1",
            None => "D",
        },
    );
    let Some(w) = winner else {
        for obj in present.into_iter().flatten() {
            let line = text_of(ctx, TID_GAME_GUILDCOMBAT_1TO1_WARDRAW);
            notice_diag(ctx, obj, &line);
        }
        return;
    };
    let (winner_obj, loser_obj) = if w == 0 {
        (present[0], present[1])
    } else {
        (present[1], present[0])
    };
    let chips = win_chips(svc.social.gc1to1.config.win_chip_num);
    if let (Some(obj), Some(c)) = (winner_obj, a.kth(w)) {
        let mut awards = Awards::default();
        give_chips(svc, ctx, c, obj, chips, &mut awards);
        flush_awards(svc, ctx, "Gc1to1Round", awards, Vec::new());
        let line = text_with(ctx, TID_GAME_GUILDCOMBAT_1TO1_WARWIN, &[chips.to_string()]);
        notice_diag(ctx, obj, &line);
        if no_other {
            defined(ctx, obj, TID_GAME_GUILDCOMBAT1TO1_WARNOOTHER, Vec::new());
        }
    }
    if let Some(obj) = loser_obj {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_1TO1_WARLOST);
        notice_diag(ctx, obj, &line);
    }
}

/// `AddDiagText` to one avatar.
fn notice_diag(ctx: &mut GameCtx<'_>, obj: ObjId, line: &str) {
    crate::chat::ingress::notice::diag(ctx, PlayerRef(obj), line);
}

/// Finish (`104-113`, handled at the step after the result): the state `{103, 0}` to all, the result of the round to every
/// present line-up member, both k-th fighters back to the waiting area (state 2), `prog += 1`, the next [`war_wait`].
pub(super) fn finish(svc: &mut Services, ctx: &mut GameCtx<'_>, idx: usize) {
    if let Some(m) = arena_mut(svc, idx) {
        m.due = GameMs(0);
    }
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    send_state_all(svc, ctx, &a);
    send_round_result_all(svc, ctx, &a);
    for side in 0..2 {
        if let Some(obj) = present_kth(svc, ctx, &a, side, None) {
            to_wait_stage(ctx, &a, side, obj);
        }
    }
    if let Some(m) = arena_mut(svc, idx) {
        m.prog += 1;
    }
    war_wait(svc, ctx, idx);
}

/// The death of a player (`DeathHooks::world_die`, `World.cpp:1946-1975`). FIX: only when the victim is the k-th fighter
/// of an arena in War and present does anything happen (the original re-ran the check for ANY death of a tender-guild
/// member: a death in the entrance could decide round 0 twice, a death after an arena closed ran its close wait again and
/// paid its chips again), and any killer counts (the original queued only deaths by a player). His side loses a life
/// (only while both k-th fighters are present, `594-613`), then the result check runs. With the flag off the frozen event
/// reacts to nothing.
pub fn on_world_die(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    if !enabled(ctx) {
        return;
    }
    let Some(c) = ctx
        .worlds
        .player(k.victim)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return;
    };
    let Some((idx, side)) = fighter_slot(svc, c) else {
        return;
    };
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    if present(svc, ctx, &a, side, c) != Some(k.victim) {
        return;
    }
    if present_kth(svc, ctx, &a, 1 - side, None).is_some()
        && let Some(m) = arena_mut(svc, idx)
    {
        m.side_mut(side).lives -= 1;
    }
    result_check(svc, ctx, idx, None);
}

/// `(arena index, side)` when `c` is the current k-th fighter of an arena in state War.
fn fighter_slot(svc: &Services, c: CharacterId) -> Option<(usize, usize)> {
    let guild = svc.social.guild.guild_of(c)?;
    let idx = svc.social.gc1to1.arena_of(guild)?;
    let a = svc.social.gc1to1.arenas.get(idx)?;
    if a.state != ArenaState::War {
        return None;
    }
    let side = a.side_of(guild)?;
    (a.kth(side) == Some(c)).then_some((idx, side))
}

/// The logout rule (`User.cpp:4283-4295`, `LogoutStep::Gc1to1Check`; the step is gated by the flag in
/// `world::logout::remove_player`): a leaver with `guild_combat_state == 1` who is the current k-th fighter of an arena in
/// War loses the round (`result_check` with him as the leaver). FIX: the original asked only for the state and so re-ran
/// the check in the up-to-one-second Finish window after a decided round (counted twice). `remove_player` has already
/// marked the avatar deleted: the guild, the world and the state are read on the mover itself.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(p) = ctx.worlds.player(who.0).and_then(|m| m.player()) else {
        return;
    };
    if p.social.guild_combat_state != 1 {
        return;
    }
    let c = p.ident.character_id;
    let Some((idx, _)) = fighter_slot(svc, c) else {
        return;
    };
    let in_arena = svc.social.gc1to1.arenas.get(idx).is_some_and(|a| {
        ctx.worlds
            .core(who.0)
            .is_some_and(|core| core.world == a.world)
    });
    if in_arena {
        result_check(svc, ctx, idx, Some(c));
    }
}
