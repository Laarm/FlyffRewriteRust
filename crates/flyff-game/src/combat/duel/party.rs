//! Party duels (Plan/17 §8.7; `DPSrvr.cpp:2069-2159`, `party.cpp:575-735`, `MoverParam.cpp:810-845`): two party
//! leaders agree, every online member of both parties fights; the duel ends when the losing **leader** dies
//! (`DuelPartyResult`: fame for every winner, a loss for every loser), when it idles for 3 minutes, when a
//! party dissolves or when a duelist is killed by a stranger (`DuelPartyCancel`).

use super::fame::{fame_loss, party_fame};
use super::{
    DUEL_IDLE_MS, DuelMember, DuelRequest, OPPONENT_CHECK_MS, STATE_COUNTDOWN, avatar_of,
    character_of, clear_duel_party, send_set_duel, tell, text, valid_player,
};
use crate::combat::pvp::pvp_inspection_base;
use crate::combat::state::DuelState;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::stats::out::{Aud, Out};
use flyff_common::defs::text::{
    TID_GAME_DECFAMEPOINT, TID_GAME_GETFAMEPOINT, TID_PK_LEVEL_GAP, TID_PK_MODE_REJECT,
    TID_PK_NO_IPARTYLEADER, TID_PK_NO_UPARTYLEADER, TID_PK_PARTY_LIMIT,
};
use flyff_common::{CharacterId, GameMs, ObjId, PartyId};
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::consts::PVPCONFIRM_MODE;
use serde_json::json;

use super::solo::duel_result;

/// The level gap of the two leaders from which parties cannot duel.
const PARTY_LEVEL_GAP: i32 = 30;

/// The members of `party` (member order, index 0 the leader).
pub(crate) fn members(s: &Services, party: PartyId) -> Option<Vec<DuelMember>> {
    s.combat.duel.view.members(s, party)
}

/// The online members of a member list that are live players in the world.
fn online(ctx: &GameCtx<'_>, list: &[DuelMember]) -> Vec<ObjId> {
    list.iter()
        .filter_map(|m| m.obj)
        .filter(|id| valid_player(ctx, *id).is_some())
        .collect()
}

/// The name the opposing side sees for `party`: the advanced party's name, else the name of the leader.
fn party_name(s: &Services, ctx: &GameCtx<'_>, party: PartyId, list: &[DuelMember]) -> String {
    if let Some(n) = s.combat.duel.view.advanced_name(s, party) {
        return n;
    }
    let Some(leader) = list.first() else {
        return String::new();
    };
    s.presence
        .get(leader.character)
        .map(|e| e.name.clone())
        .or_else(|| {
            leader
                .obj
                .and_then(|o| ctx.worlds.mover(o))
                .and_then(|m| m.player())
                .map(|p| p.ident.name.clone())
        })
        .unwrap_or_default()
}

/// `IsPVPInspection(target, 2)` (this = `a`): both players are in a party; the leaders are online and not 30
/// levels apart; every online member of both parties passes the base inspection against the world of `a`.
/// `Err(None)` is a silent refusal.
fn pvp_inspection_party(
    s: &Services,
    ctx: &GameCtx<'_>,
    a: ObjId,
    b: ObjId,
) -> Result<(PartyId, PartyId), Option<u32>> {
    let party = |id: ObjId| {
        ctx.worlds
            .mover(id)
            .and_then(|m| m.player())
            .and_then(|p| p.social.party)
    };
    let (Some(pa), Some(pb_)) = (party(a), party(b)) else {
        return Err(None);
    };
    let (Some(ma), Some(mb)) = (members(s, pa), members(s, pb_)) else {
        return Err(None);
    };
    let leader = |list: &[DuelMember]| {
        list.first()
            .and_then(|m| m.obj)
            .filter(|o| valid_player(ctx, *o).is_some())
    };
    let (Some(la), Some(lb)) = (leader(&ma), leader(&mb)) else {
        return Err(None);
    };
    let level = |id: ObjId| ctx.worlds.mover(id).map_or(0, |m| m.mv.level);
    if (level(la) - level(lb)).abs() >= PARTY_LEVEL_GAP {
        return Err(Some(TID_PK_LEVEL_GAP));
    }
    let world = ctx.worlds.core(a).map(|c| c.world);
    let Some(world) = world else {
        return Err(None);
    };
    for id in online(ctx, &ma).into_iter().chain(online(ctx, &mb)) {
        if let Some(m) = ctx.worlds.mover(id) {
            let code = pvp_inspection_base(m, world, &s.stats.core().rules);
            if code != 0 {
                return Err(super::super::pvp::InspectError::Code(code).tid());
            }
        }
    }
    Ok((pa, pb_))
}

/// `DuelPartyRequestReq` (`OnDuelPartyRequest`): the leader of a party challenges the leader `dst` of another.
///
/// VR-001: the sender is the session's player. `PVPCONFIRM_MODE` of the target refuses (`TID_PK_MODE_REJECT`);
/// then `IsPVPInspection(target, 2)`; the sender must lead its party (`TID_PK_NO_IPARTYLEADER`) and so must the
/// target (`TID_PK_NO_UPARTYLEADER`). Effect: the request is stored (VR-050) and `DuelPartyRequestNtf` goes to
/// the target leader. A party cannot duel itself (`TID_PK_PARTY_LIMIT`).
pub fn on_duel_party_request(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::DuelPartyRequestReq,
) {
    let me = who.0;
    let Some(dst) = avatar_of(s, ctx, CharacterId(m.dst)) else {
        return;
    };
    if dst == me {
        s.combat.duel.stats.ignored += 1;
        return;
    }
    let (Some(a), Some(b)) = (valid_player(ctx, me), valid_player(ctx, dst)) else {
        return;
    };
    if b.mv.mode & PVPCONFIRM_MODE != 0 {
        text(ctx, me, TID_PK_MODE_REJECT, Vec::new());
        return;
    }
    let party = |m: &crate::mover::Mover| m.player().and_then(|p| p.social.party);
    if party(a).is_some() && party(a) == party(b) {
        text(ctx, me, TID_PK_PARTY_LIMIT, Vec::new());
        return;
    }
    let (pa, pb_) = match pvp_inspection_party(s, ctx, me, dst) {
        Ok(p) => p,
        Err(tid) => {
            if let Some(tid) = tid {
                text(ctx, me, tid, Vec::new());
            }
            return;
        }
    };
    let leads = |party: PartyId, who: ObjId, ctx: &GameCtx<'_>| {
        let c = ctx.worlds.mover(who).map(character_of);
        members(s, party).is_some_and(|l| l.first().map(|m| m.character) == c)
    };
    if !leads(pa, me, ctx) {
        text(ctx, me, TID_PK_NO_IPARTYLEADER, Vec::new());
        return;
    }
    if !leads(pb_, dst, ctx) {
        text(ctx, me, TID_PK_NO_UPARTYLEADER, Vec::new());
        return;
    }
    let (src_id, dst_id) = (character_of(a).0, character_of(b).0);
    let now = ctx.clock.game_ms;
    if let Some(m) = ctx.worlds.mover_mut(me) {
        m.combat.duel.pending = Some(DuelRequest {
            target: dst,
            at: now,
            party: true,
        });
    }
    tell(
        ctx,
        dst,
        Aud::SelfSubject,
        pb::DuelPartyRequestNtf {
            src: src_id,
            dst: dst_id,
        },
    );
}

/// `DuelPartyYes` (`OnDuelPartyYes`): the invited leader accepts.
///
/// VR-001 / VR-050: the acceptor is the session's player; the requesting leader `src` must have a pending *party*
/// request naming it, not older than the TTL. Then the inspection (messages to the requester) and the leader
/// check of both. Effect: both leaders' idle timers start, every online member of both parties gets
/// `duel = 2, state = 104, duel_party = the other party` and `DuelPartyStart{..., 0}`.
pub fn on_duel_party_yes(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::DuelPartyYes,
) {
    let dst = who.0;
    let Some(src) = avatar_of(s, ctx, CharacterId(m.src)) else {
        return;
    };
    if src == dst {
        s.combat.duel.stats.ignored += 1;
        return;
    }
    let now = ctx.clock.game_ms;
    let ttl_ms = u64::from(ctx.cfg.rules.gameplay.duel_request_ttl_s) * 1000;
    match valid_player(ctx, src).and_then(|m| m.combat.duel.pending) {
        Some(r) if r.target == dst && r.party => {
            if now.0 > r.at.0.saturating_add(ttl_ms) {
                if let Some(m) = ctx.worlds.mover_mut(src) {
                    m.combat.duel.pending = None;
                }
                s.combat.duel.stats.ignored += 1;
                return;
            }
        }
        _ => {
            s.combat.duel.stats.ignored += 1;
            return;
        }
    }
    let (pa, pb_) = match pvp_inspection_party(s, ctx, src, dst) {
        Ok(p) => p,
        Err(tid) => {
            if let Some(tid) = tid {
                text(ctx, src, tid, Vec::new());
            }
            return;
        }
    };
    if pa == pb_ {
        return;
    }
    // both must be the leaders of their parties
    let (Some(ma), Some(mb)) = (members(s, pa), members(s, pb_)) else {
        return;
    };
    let (src_char, dst_char) = (
        ctx.worlds.mover(src).map(character_of),
        ctx.worlds.mover(dst).map(character_of),
    );
    if ma.first().map(|m| m.character) != src_char || mb.first().map(|m| m.character) != dst_char {
        tracing::warn!(
            src = src.0,
            dst = dst.0,
            "DuelPartyYes: not both party leaders"
        );
        return;
    }
    for id in [src, dst] {
        if let Some(m) = ctx.worlds.mover_mut(id) {
            m.combat.duel.end_at = GameMs(now.0 + DUEL_IDLE_MS);
            m.core.pos_changed = true;
        }
    }
    if let Some(m) = ctx.worlds.mover_mut(src) {
        m.combat.duel.pending = None;
    }
    s.combat.duel.pair(pa, pb_);
    s.combat.duel.stats.party_started += 1;
    start_side(s, ctx, pa, &ma, pb_, &mb);
    start_side(s, ctx, pb_, &mb, pa, &ma);
}

/// `CParty::DoDuelPartyStart(dst)` for the party `own`: every online member is put into the duel and told about
/// the opposing party (`DuelPartyStart{id_party, name, members, 0}`).
fn start_side(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    own: PartyId,
    own_members: &[DuelMember],
    opp: PartyId,
    opp_members: &[DuelMember],
) {
    let now = ctx.clock.game_ms;
    let name = party_name(s, ctx, opp, opp_members);
    let leader = own_members.first().and_then(|m| m.obj);
    let opp_objs: Vec<u32> = opp_members
        .iter()
        .map(|m| m.obj.map_or(u32::MAX, |o| o.0))
        .collect();
    let _ = own;
    for id in online(ctx, own_members) {
        if let Some(m) = ctx.worlds.mover_mut(id) {
            let end_at = GameMs(now.0 + DUEL_IDLE_MS);
            m.combat.duel = DuelState {
                duel: 2,
                state: STATE_COUNTDOWN,
                other: None,
                end_at,
                next_tick: now,
                next_ap: GameMs(now.0 + OPPONENT_CHECK_MS),
                leader,
                finishing: false,
                pending: m.combat.duel.pending,
            };
            if let Some(p) = m.player_mut() {
                p.social.duel_party = opp.0;
            }
        }
        s.combat.duel.track(id);
        tell(
            ctx,
            id,
            Aud::SelfSubject,
            pb::DuelPartyStart {
                id_party: opp.0,
                party_name: name.clone(),
                members: opp_objs.clone(),
                flag: 0,
            },
        );
    }
}

/// `DuelPartyNoReq` (`OnDuelPartyNo`): the invited leader declines; the requester hears who.
pub fn on_duel_party_no(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::DuelPartyNoReq,
) {
    let me = who.0;
    let Some(src) = avatar_of(s, ctx, CharacterId(m.src)) else {
        return;
    };
    if let Some(r) = ctx.worlds.mover_mut(src).filter(|x| {
        x.combat
            .duel
            .pending
            .is_some_and(|r| r.target == me && r.party)
    }) {
        r.combat.duel.pending = None;
    }
    s.combat.duel.stats.cancelled += 1;
    tell(
        ctx,
        src,
        Aud::SelfSubject,
        pb::DuelPartyNoNtf { decliner: me.0 },
    );
}

/// `CMover::DuelResult(lose)` for a party duel (this = `winner`): only the fall of the losing party's **leader**
/// counts. The level gap is the losing leader's against the winning party's leader (the winner itself when that
/// leader is not online). Returns `false` when the loser is not a leader or a party is missing.
pub(crate) fn party_result(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    winner: ObjId,
    loser: ObjId,
) -> bool {
    let (Some(w), Some(l)) = (ctx.worlds.mover(winner), ctx.worlds.mover(loser)) else {
        return false;
    };
    let party = |m: &crate::mover::Mover| m.player().and_then(|p| p.social.party);
    let (Some(lp), Some(wp)) = (party(l), party(w)) else {
        return false;
    };
    let (Some(lm), Some(wm)) = (members(s, lp), members(s, wp)) else {
        return false;
    };
    if lm.first().map(|m| m.character) != Some(character_of(l)) {
        return false;
    }
    let loser_level = l.mv.level;
    let gap = match wm
        .first()
        .and_then(|m| m.obj)
        .and_then(|o| valid_player(ctx, o))
    {
        Some(leader) => loser_level - leader.mv.level,
        None => loser_level - w.mv.level,
    };
    let (gain, percent) = party_fame(gap);
    s.combat.duel.unpair(lp, Some(wp));
    if let Some(p) = ctx.worlds.mover_mut(loser).and_then(|m| m.player_mut()) {
        p.death_flags.last_duel_party = true;
    }
    result_side(s, ctx, &lm, &wm, false, gain, percent);
    result_side(s, ctx, &wm, &lm, true, gain, percent);
    s.combat.duel.stats.results += 1;
    true
}

/// `CParty::DoDuelResult(other, win, add_fame, ratio)`: every online member learns the result, leaves the duel and
/// gets or loses fame.
fn result_side(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    side: &[DuelMember],
    other: &[DuelMember],
    win: bool,
    gain: i32,
    percent: f32,
) {
    let now = ctx.clock.game_ms;
    let ids: Vec<u32> = other.iter().map(|m| m.character.0).collect();
    for id in online(ctx, side) {
        tell(
            ctx,
            id,
            Aud::SelfSubject,
            pb::DuelPartyResult {
                win,
                ids: ids.clone(),
            },
        );
        if let Some(m) = ctx.worlds.mover_mut(id) {
            clear_duel_party(m, now);
        }
        s.combat.duel.untrack(id);
        let delta = if win {
            gain
        } else {
            let fame = ctx
                .worlds
                .mover(id)
                .and_then(|m| m.player())
                .map_or(0, |p| p.progression.fame);
            fame_loss(fame, percent, false)
        };
        let fame = ctx
            .worlds
            .mover_mut(id)
            .and_then(|m| m.player_mut())
            .map(|p| {
                if win {
                    p.progression.fame = p.progression.fame.wrapping_add(delta);
                } else {
                    p.progression.fame = p.progression.fame.wrapping_sub(delta).max(0);
                }
                p.progression.fame
            });
        text(
            ctx,
            id,
            if win {
                TID_GAME_GETFAMEPOINT
            } else {
                TID_GAME_DECFAMEPOINT
            },
            vec![delta.to_string()],
        );
        crate::audit::emit_for(
            ctx,
            PlayerRef(id),
            ev::CHAR_PK,
            json!({ "kind": "party_duel", "win": win, "fame_delta": if win { delta } else { -delta } }),
        );
        if let Some(fame) = fame {
            let mut out = Out::new();
            out.near(pb::SetFame { fame });
            out.flush(ctx, id);
        }
    }
}

/// `CParty::DoDuelPartyCancel(other)`: every online member of `side` gets `DuelPartyCancel{ids of the other
/// side}` (empty for `None`) and leaves the duel.
fn cancel_side(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    side: &[DuelMember],
    other: Option<&[DuelMember]>,
) {
    let now = ctx.clock.game_ms;
    let ids: Vec<u32> = other
        .map(|o| o.iter().map(|m| m.character.0).collect())
        .unwrap_or_default();
    for id in online(ctx, side) {
        tell(
            ctx,
            id,
            Aud::SelfSubject,
            pb::DuelPartyCancel { ids: ids.clone() },
        );
        if let Some(m) = ctx.worlds.mover_mut(id) {
            clear_duel_party(m, now);
        }
        s.combat.duel.untrack(id);
    }
}

/// Cancels the duel of the parties `a` and `b` (both sides are told and cleared); the pair is removed.
pub(crate) fn cancel_pair(s: &mut Services, ctx: &mut GameCtx<'_>, a: PartyId, b: Option<PartyId>) {
    let ma = members(s, a);
    let mb = b.and_then(|b| members(s, b));
    s.combat.duel.unpair(a, b);
    if let Some(mb) = &mb {
        cancel_side(s, ctx, mb, ma.as_deref());
    }
    if let Some(ma) = &ma {
        cancel_side(s, ctx, ma, mb.as_deref());
    }
    s.combat.duel.stats.cancelled += 1;
}

/// `SubPVP`, party branch: `victim` was killed by a stranger (or by an opponent who is already out). When the
/// victim leads its party, the duel is cancelled for both parties.
pub fn cancel_party_of_leader(s: &mut Services, ctx: &mut GameCtx<'_>, victim: ObjId) {
    let Some(v) = ctx.worlds.mover(victim) else {
        return;
    };
    let (Some(own), duel_party, character) = (
        v.player().and_then(|p| p.social.party),
        v.player().map_or(0, |p| p.social.duel_party),
        character_of(v),
    ) else {
        return;
    };
    let Some(list) = members(s, own) else {
        return;
    };
    if list.first().map(|m| m.character) != Some(character) {
        return;
    }
    cancel_pair(
        s,
        ctx,
        own,
        (duel_party != 0).then_some(PartyId(duel_party)),
    );
}

/// A party was dissolved (or its leader left) while it duelled: both sides are told `DuelPartyCancel`
/// (`CPartyMng::DeleteParty`, `OnRemovePartyMember`). Doc 21 calls it **before** the party record disappears, so
/// that the membership is still readable.
pub fn on_party_dissolved(s: &mut Services, ctx: &mut GameCtx<'_>, party: PartyId) {
    let Some(opp) = s.combat.duel.opposing(party) else {
        return;
    };
    cancel_pair(s, ctx, party, Some(opp));
}

/// `AddSetDuel(about)` to every online member of the party of `about` and of the party it duels.
pub(crate) fn broadcast_set_duel(s: &mut Services, ctx: &mut GameCtx<'_>, about: ObjId) {
    let Some(v) = ctx.worlds.mover(about).and_then(|m| m.player()) else {
        return;
    };
    let (own, opp) = (v.social.party, v.social.duel_party);
    let mut to = Vec::new();
    for party in [own, (opp != 0).then_some(PartyId(opp))]
        .into_iter()
        .flatten()
    {
        if let Some(list) = members(s, party) {
            to.extend(online(ctx, &list));
        }
    }
    for id in to {
        send_set_duel(ctx, id, about);
    }
}

/// Used by the per-tick region rule: the leader of the opposing party wins (`pWinner->DuelResult(this)`).
pub(crate) fn opposing_leader_wins(s: &mut Services, ctx: &mut GameCtx<'_>, loser: ObjId) {
    let Some(v) = ctx.worlds.mover(loser).and_then(|m| m.player()) else {
        return;
    };
    let opp = v.social.duel_party;
    if opp == 0 {
        return;
    }
    let Some(list) = members(s, PartyId(opp)) else {
        return;
    };
    let Some(winner) = list.first().and_then(|m| m.obj) else {
        return;
    };
    duel_result(s, ctx, winner, loser);
}
