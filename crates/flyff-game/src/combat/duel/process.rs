//! The per-tick work of the duels (Plan/17 §8.6-§8.7; `Mover.cpp:8328-8460` `ProcessRecovery`, `2905-2935`
//! `ProcessRegion`, `3505-3520` `ProcessActionPoint`): for every player in a duel, in ascending object order, after
//! the world phase of the tick.
//!
//! 1. **Countdown**: once a second (the grid starts with the accept) `104 -> 103 -> 102 -> 101 -> 100`; the
//!    client gets `DuelCount{3}`, `{2}`, `{1}`; at `100` the state becomes `1` and the client gets
//!    `DuelStart{other, 1}` (party: `DuelPartyStart{..., 1}`) and `DuelCount{0}`.
//! 2. **Idle timeout**: no hit for 3 minutes ends the duel (`SetDuel` to both; a party duel is cancelled by the
//!    leaders).
//! 3. **Region rule**: when a duelist leaves its region class (`IsRegionMove`) the opponent wins; in a party duel
//!    only a leader's move counts.
//! 4. **Vanished opponent** (every 3 s): a solo duelist whose opponent is gone gets `DuelCancel{NULL}` and is
//!    cleared.
//!
//! A dead player is not processed.

use super::party::{cancel_pair, members, opposing_leader_wins};
use super::solo::duel_result;
use super::{
    COUNTDOWN_STEP_MS, OPPONENT_CHECK_MS, STATE_FIGHT, character_of, clear_duel, clear_duel_party,
    send_set_duel, tell, valid_player,
};
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, Services};
use crate::stats::out::Aud;
use crate::world::is_region_move;
use flyff_common::{GameMs, ObjId, PartyId};
use flyff_proto::pb;

/// Runs the per-tick work for every player in a duel (see the module documentation).
pub fn process(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let ids: Vec<ObjId> = s.combat.duel.active().collect();
    if ids.is_empty() {
        return;
    }
    let now = ctx.clock.game_ms;
    for id in ids {
        let Some(m) = valid_player(ctx, id) else {
            s.combat.duel.untrack(id);
            continue;
        };
        if m.combat.duel.duel == 0 {
            s.combat.duel.untrack(id);
            continue;
        }
        if is_die(m) {
            continue;
        }
        countdown(s, ctx, id, now);
        if !dueling(ctx, id) {
            continue;
        }
        timeout(s, ctx, id, now);
        if !dueling(ctx, id) {
            continue;
        }
        region_rule(s, ctx, id);
        if !dueling(ctx, id) {
            continue;
        }
        vanished_opponent(s, ctx, id, now);
    }
}

fn dueling(ctx: &GameCtx<'_>, id: ObjId) -> bool {
    ctx.worlds
        .mover(id)
        .is_some_and(|m| m.combat.duel.duel != 0)
}

/// The 1 s countdown (`ProcessRecovery`, `Mover.cpp:8328-8390`).
fn countdown(s: &mut Services, ctx: &mut GameCtx<'_>, id: ObjId, now: GameMs) {
    let Some(d) = ctx.worlds.mover(id).map(|m| m.combat.duel) else {
        return;
    };
    if now.0 <= d.next_tick.0 {
        return;
    }
    let mut fight = false;
    let mut count = None;
    if (100..200).contains(&d.state) {
        let state = d.state - 1;
        if state == 100 {
            fight = true;
        } else {
            count = Some(i32::from(state) - 100);
        }
        if let Some(m) = ctx.worlds.mover_mut(id) {
            m.combat.duel.state = if fight { STATE_FIGHT } else { state };
        }
    }
    if let Some(m) = ctx.worlds.mover_mut(id) {
        m.combat.duel.next_tick = GameMs(now.0 + COUNTDOWN_STEP_MS);
    }
    if let Some(c) = count {
        tell(ctx, id, Aud::SelfZero, pb::DuelCount { count: c });
    }
    if !fight {
        return;
    }
    match d.duel {
        1 => {
            // `prj.GetMover(m_idDuelOther)`: only when the opponent still exists
            let other = d.other.and_then(|o| ctx.worlds.mover(o)).map(character_of);
            if let Some(other) = other {
                tell(
                    ctx,
                    id,
                    Aud::SelfSubject,
                    pb::DuelStart {
                        id_target: other.0,
                        flag: 1,
                    },
                );
                tell(ctx, id, Aud::SelfZero, pb::DuelCount { count: 0 });
            }
        }
        2 => {
            let opp = ctx
                .worlds
                .mover(id)
                .and_then(|m| m.player())
                .map_or(0, |p| p.social.duel_party);
            if opp == 0 {
                return;
            }
            let opp = PartyId(opp);
            let Some(list) = members(s, opp) else {
                return;
            };
            let name = s
                .combat
                .duel
                .view
                .advanced_name(s, opp)
                .or_else(|| {
                    list.first().and_then(|l| {
                        s.presence
                            .get(l.character)
                            .map(|e| e.name.clone())
                            .or_else(|| {
                                l.obj
                                    .and_then(|o| ctx.worlds.mover(o))
                                    .and_then(|m| m.player())
                                    .map(|p| p.ident.name.clone())
                            })
                    })
                })
                .unwrap_or_default();
            tell(
                ctx,
                id,
                Aud::SelfSubject,
                pb::DuelPartyStart {
                    id_party: opp.0,
                    party_name: name,
                    members: list
                        .iter()
                        .map(|m| m.obj.map_or(u32::MAX, |o| o.0))
                        .collect(),
                    flag: 1,
                },
            );
            tell(ctx, id, Aud::SelfZero, pb::DuelCount { count: 0 });
        }
        _ => {}
    }
}

/// The idle timeout (`ProcessRecovery`, `Mover.cpp:8392-8440`).
fn timeout(s: &mut Services, ctx: &mut GameCtx<'_>, id: ObjId, now: GameMs) {
    let Some(d) = ctx.worlds.mover(id).map(|m| m.combat.duel) else {
        return;
    };
    if now.0 <= d.end_at.0 {
        return;
    }
    match d.duel {
        1 => {
            // only while the opponent exists
            let Some(other) = d.other.filter(|o| valid_player(ctx, *o).is_some()) else {
                return;
            };
            for who in [other, id] {
                if let Some(m) = ctx.worlds.mover_mut(who) {
                    clear_duel(m, now);
                }
                send_set_duel(ctx, who, who);
            }
            s.combat.duel.untrack(other);
            s.combat.duel.untrack(id);
            s.combat.duel.stats.cancelled += 1;
        }
        2 => {
            if let Some((own, opp)) = party_pair_if_leader(s, ctx, id) {
                cancel_pair(s, ctx, own, opp);
            }
        }
        _ => {}
    }
}

/// `(own party, opposing party)` when `id` leads its party in a party duel.
fn party_pair_if_leader(
    s: &Services,
    ctx: &GameCtx<'_>,
    id: ObjId,
) -> Option<(PartyId, Option<PartyId>)> {
    let m = ctx.worlds.mover(id)?;
    let p = m.player()?;
    let own = p.social.party?;
    let list = members(s, own)?;
    if list.first().map(|l| l.character) != Some(character_of(m)) {
        return None;
    }
    Some((
        own,
        (p.social.duel_party != 0).then_some(PartyId(p.social.duel_party)),
    ))
}

/// The region rule (`ProcessRegion`, `Mover.cpp:2905-2935`): leaving the region class ends the duel in favour of
/// the opponent.
fn region_rule(s: &mut Services, ctx: &mut GameCtx<'_>, id: ObjId) {
    let Some(m) = ctx.worlds.mover(id) else {
        return;
    };
    let Some(p) = m.player() else { return };
    if !is_region_move(p.old_region_attr, p.region_attr) {
        return;
    }
    let now = ctx.clock.game_ms;
    match m.combat.duel.duel {
        1 => {
            let Some(other) = m
                .combat
                .duel
                .other
                .filter(|o| valid_player(ctx, *o).is_some())
            else {
                return;
            };
            duel_result(s, ctx, other, id);
            for who in [other, id] {
                if let Some(m) = ctx.worlds.mover_mut(who) {
                    clear_duel(m, now);
                }
                send_set_duel(ctx, who, who);
            }
            s.combat.duel.untrack(other);
            s.combat.duel.untrack(id);
        }
        2 if party_pair_if_leader(s, ctx, id).is_some() => opposing_leader_wins(s, ctx, id),
        _ => {}
    }
}

/// The 3 s check of `ProcessActionPoint`: a solo duelist whose opponent is gone is told `DuelCancel{NULL}`.
fn vanished_opponent(s: &mut Services, ctx: &mut GameCtx<'_>, id: ObjId, now: GameMs) {
    let Some(d) = ctx.worlds.mover(id).map(|m| m.combat.duel) else {
        return;
    };
    if now.0 <= d.next_ap.0 {
        return;
    }
    if let Some(m) = ctx.worlds.mover_mut(id) {
        m.combat.duel.next_ap = GameMs(now.0 + OPPONENT_CHECK_MS);
    }
    match d.duel {
        1 => {
            let gone = d
                .other
                .is_none_or(|o| ctx.worlds.mover(o).is_none_or(|m| m.core.deleted));
            if gone {
                tell(
                    ctx,
                    id,
                    Aud::SelfSubject,
                    pb::DuelCancel { target: u32::MAX },
                );
                if let Some(m) = ctx.worlds.mover_mut(id) {
                    clear_duel(m, now);
                }
                s.combat.duel.untrack(id);
                s.combat.duel.stats.cancelled += 1;
            }
        }
        2 => {
            let (own, opp) = ctx
                .worlds
                .mover(id)
                .and_then(|m| m.player())
                .map_or((None, 0), |p| (p.social.party, p.social.duel_party));
            // a member that is no longer in a party that exists leaves the duel alone
            let Some(own_list) = own.and_then(|party| members(s, party)) else {
                tell(
                    ctx,
                    id,
                    Aud::SelfSubject,
                    pb::DuelPartyCancel { ids: Vec::new() },
                );
                if let Some(m) = ctx.worlds.mover_mut(id) {
                    clear_duel_party(m, now);
                }
                s.combat.duel.untrack(id);
                s.combat.duel.stats.cancelled += 1;
                return;
            };
            // a duel whose opposing party is gone, or one of whose leaders is not in the world any more (nobody
            // is left to time it out), is cancelled for both parties
            let online_leader = |list: &[super::DuelMember]| {
                list.first()
                    .and_then(|l| l.obj)
                    .is_some_and(|o| valid_player(ctx, o).is_some())
            };
            let opp_list = (opp != 0).then(|| members(s, PartyId(opp))).flatten();
            let broken = match &opp_list {
                None => true,
                Some(list) => !online_leader(&own_list) || !online_leader(list),
            };
            if broken && let Some(own) = own {
                cancel_pair(s, ctx, own, (opp != 0).then_some(PartyId(opp)));
            }
        }
        _ => {}
    }
}
