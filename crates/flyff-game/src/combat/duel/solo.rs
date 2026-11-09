//! Solo duels (Plan/17 §8.6; `DPSrvr.cpp:1942-2065`, `MoverParam.cpp:754-850`, `Mover.cpp:5842-5850`,
//! `ActionMoverMsg.cpp:850-866`).

use super::fame::{fame_loss, solo_fame};
use super::{
    DUEL_IDLE_MS, DuelRequest, OPPONENT_CHECK_MS, RESULT_INVULNERABLE_TICKS, STATE_COUNTDOWN,
    avatar_of, character_of, clear_duel, send_set_duel, tell, text, valid_player,
};
use crate::buff::api::remove_flags;
use crate::buff::core::RemoveFlags;
use crate::combat::hooks::KillCtx;
use crate::combat::pvp::{pvp_inspection_solo, summon_state};
use crate::combat::state::DuelState;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::stats::out::{Aud, Out};
use flyff_common::defs::text::{
    TID_GAME_DECFAMEPOINT, TID_GAME_GETFAMEPOINT, TID_PK_MODE_REJECT, TID_PK_PARTY_LIMIT,
};
use flyff_common::{CharacterId, GameMs, ObjId};
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::consts::PVPCONFIRM_MODE;
use serde_json::json;

/// `DuelStart.flag` of the result: the receiver won / lost the solo duel (`AddDuelStart( ..., 3 / 4 )`).
const DUEL_WON: i32 = 3;
const DUEL_LOST: i32 = 4;

/// `DuelRequestReq` (`OnDuelRequest`): the session's player challenges `dst` to a solo duel.
///
/// VR-001: the requester is the session's player, the `src` field is ignored. Checked in order: the same party
/// (`TID_PK_PARTY_LIMIT`), either in any party, either trading, the requester's summon state, the target's
/// summon state (PL-084, `fidelity.duel_target_summon_check`), `IsPVPInspection(target, 1)`, the target's
/// `PVPCONFIRM_MODE` (`TID_PK_MODE_REJECT`). Effect: the request is stored on the requester (VR-050) and the
/// target gets `DuelRequestNtf`.
pub fn on_duel_request(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::DuelRequestReq,
) {
    let me = who.0;
    let Some(target) = avatar_of(s, ctx, CharacterId(m.dst)) else {
        return;
    };
    if target == me {
        s.combat.duel.stats.ignored += 1;
        return;
    }
    let (Some(a), Some(b)) = (valid_player(ctx, me), valid_player(ctx, target)) else {
        return;
    };
    let party = |m: &crate::mover::Mover| m.player().and_then(|p| p.social.party);
    if let (Some(x), Some(y)) = (party(a), party(b))
        && x == y
    {
        text(ctx, me, TID_PK_PARTY_LIMIT, Vec::new());
        return;
    }
    if party(a).is_some() || party(b).is_some() {
        return;
    }
    let trading = |m: &crate::mover::Mover| m.player().is_some_and(|p| p.econ.has_partner());
    if trading(a) || trading(b) {
        return;
    }
    if summon_state(a) != 0 {
        return;
    }
    if ctx.cfg.rules.fidelity.duel_target_summon_check && summon_state(b) != 0 {
        return;
    }
    if let Err(e) = pvp_inspection_solo(a, b, &s.stats.core().rules) {
        if let Some(tid) = e.tid() {
            text(ctx, me, tid, Vec::new());
        }
        return;
    }
    if b.mv.mode & PVPCONFIRM_MODE != 0 {
        text(ctx, me, TID_PK_MODE_REJECT, Vec::new());
        return;
    }
    // the request, bound to the invitee (VR-050)
    let (src_id, dst_id) = (character_of(a).0, character_of(b).0);
    let now = ctx.clock.game_ms;
    if let Some(m) = ctx.worlds.mover_mut(me) {
        m.combat.duel.pending = Some(DuelRequest {
            target,
            at: now,
            party: false,
        });
    }
    tell(
        ctx,
        target,
        Aud::SelfSubject,
        pb::DuelRequestNtf {
            src: src_id,
            dst: dst_id,
        },
    );
}

/// `DuelYes` (`OnDuelYes`): the invited player accepts.
///
/// VR-001: the acceptor is the session's player (`__HACK_1130`). VR-050: the requester `src` must have a pending
/// *solo* request that names the acceptor and is not older than `gameplay.duel_request_ttl_s`; a late answer
/// clears the request, any other answer changes nothing. Then: the same party (`TID_PK_PARTY_LIMIT` to both),
/// either in a party or running a stall: ignored; `IsPVPInspection` (the messages go to the requester). Effect:
/// both players get `duel = 1, state = 104` and `DuelStart{other, 0}`; the idle timer starts.
pub fn on_duel_yes(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::DuelYes) {
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
    let pending = valid_player(ctx, src).and_then(|m| m.combat.duel.pending);
    match pending {
        Some(r) if r.target == dst && !r.party => {
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
    let (Some(a), Some(b)) = (valid_player(ctx, src), valid_player(ctx, dst)) else {
        return;
    };
    let party = |m: &crate::mover::Mover| m.player().and_then(|p| p.social.party);
    if let (Some(x), Some(y)) = (party(a), party(b))
        && x == y
    {
        text(ctx, src, TID_PK_PARTY_LIMIT, Vec::new());
        text(ctx, dst, TID_PK_PARTY_LIMIT, Vec::new());
        return;
    }
    if party(a).is_some() || party(b).is_some() {
        return;
    }
    let vending = |m: &crate::mover::Mover| m.player().is_some_and(|p| p.econ.vending);
    if vending(a) || vending(b) {
        return;
    }
    if let Err(e) = pvp_inspection_solo(a, b, &s.stats.core().rules) {
        if let Some(tid) = e.tid() {
            text(ctx, src, tid, Vec::new());
        }
        return;
    }
    let (src_char, dst_char) = (character_of(a), character_of(b));
    for (id, other) in [(src, dst), (dst, src)] {
        if let Some(m) = ctx.worlds.mover_mut(id) {
            m.combat.duel = DuelState {
                duel: 1,
                state: STATE_COUNTDOWN,
                other: Some(other),
                end_at: GameMs(now.0 + DUEL_IDLE_MS),
                next_tick: now,
                next_ap: GameMs(now.0 + OPPONENT_CHECK_MS),
                leader: None,
                finishing: false,
                pending: None,
            };
            // `SetPosChanged(TRUE)`: the region attribute is evaluated again
            m.core.pos_changed = true;
        }
        s.combat.duel.track(id);
    }
    s.combat.duel.stats.solo_started += 1;
    tell(
        ctx,
        src,
        Aud::SelfSubject,
        pb::DuelStart {
            id_target: dst_char.0,
            flag: 0,
        },
    );
    tell(
        ctx,
        dst,
        Aud::SelfSubject,
        pb::DuelStart {
            id_target: src_char.0,
            flag: 0,
        },
    );
}

/// `DuelNoReq` (`OnDuelNo`): the invited player declines; the requester hears who (`DuelNoNtf`). The request
/// is dropped when it names the decliner (a third player cannot cancel somebody else's request).
pub fn on_duel_no(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::DuelNoReq) {
    let me = who.0;
    let Some(src) = avatar_of(s, ctx, CharacterId(m.src)) else {
        return;
    };
    if let Some(r) = ctx.worlds.mover_mut(src).filter(|x| {
        x.combat
            .duel
            .pending
            .is_some_and(|r| r.target == me && !r.party)
    }) {
        r.combat.duel.pending = None;
    }
    s.combat.duel.stats.cancelled += 1;
    tell(ctx, src, Aud::SelfSubject, pb::DuelNoNtf { decliner: me.0 });
}

/// `SubDuel(attacker)` (this = `victim`): a duelist died to somebody - the killer collects the result.
pub fn sub_duel(s: &mut Services, ctx: &mut GameCtx<'_>, attacker: ObjId, victim: ObjId) {
    if ctx
        .worlds
        .mover(victim)
        .is_some_and(|v| v.combat.duel.duel != 0)
    {
        duel_result(s, ctx, attacker, victim);
    }
}

/// `CMover::DuelResult(lose)` (this = `winner`, `MoverParam.cpp:784-850`): the fame of a solo duel and the
/// invulnerability afterwards, or the result of a party duel when the losing leader is the one that fell.
/// Returns `false` when the result does not apply (the loser is not the winner's partner, a non-leader of a
/// party duel).
pub fn duel_result(s: &mut Services, ctx: &mut GameCtx<'_>, winner: ObjId, loser: ObjId) -> bool {
    let (Some(w), Some(l)) = (ctx.worlds.mover(winner), ctx.worlds.mover(loser)) else {
        return false;
    };
    match l.combat.duel.duel {
        1 => {
            if w.combat.duel.other != Some(loser) {
                return false;
            }
            let (winner_other, loser_other) = (w.combat.duel.other, l.combat.duel.other);
            change_fame(s, ctx, winner, loser);
            // `__PVPDEBUFSKILL`: 2 s of invulnerability and the other's debuffs fall off, for both. The loser's
            // death still has to run: `finishing` lets `do_die` pass its double-death guard.
            for (id, enemy) in [(loser, loser_other), (winner, winner_other)] {
                if let Some(m) = ctx.worlds.mover_mut(id) {
                    m.combat.dead_ticks = RESULT_INVULNERABLE_TICKS;
                    if id == loser {
                        m.combat.duel.finishing = true;
                    }
                }
                if let Some(enemy) = enemy {
                    remove_flags(s, ctx, id, RemoveFlags::ATTACKER, enemy.0);
                }
            }
            s.combat.duel.stats.results += 1;
            true
        }
        2 => super::party::party_result(s, ctx, winner, loser),
        _ => true,
    }
}

/// `ChangeFame(lose)` (this = `winner`, `MoverParam.cpp:754-796`): the winner gains and the loser loses fame by
/// the level gap ([`solo_fame`]); both flying doubles the gain.
fn change_fame(s: &mut Services, ctx: &mut GameCtx<'_>, winner: ObjId, loser: ObjId) {
    let (Some(w), Some(l)) = (ctx.worlds.mover(winner), ctx.worlds.mover(loser)) else {
        return;
    };
    let gap = l.mv.level - w.mv.level;
    let (mut gain, percent) = solo_fame(gap);
    let both_fly = w.sim.act.flags.fly() && l.sim.act.flags.fly();
    let loser_fame = l.player().map_or(0, |p| p.progression.fame);
    if gain != 0 {
        if both_fly {
            gain = gain.wrapping_mul(2);
        }
        let fame = ctx
            .worlds
            .mover_mut(winner)
            .and_then(|m| m.player_mut())
            .map(|p| {
                p.progression.fame = p.progression.fame.wrapping_add(gain);
                p.progression.fame
            });
        if let Some(fame) = fame {
            let mut out = Out::new();
            out.near(pb::SetFame { fame });
            out.flush(ctx, winner);
            text(ctx, winner, TID_GAME_GETFAMEPOINT, vec![gain.to_string()]);
            crate::social::hooks::honor_recheck(s, ctx, PlayerRef(winner), true);
        }
    }
    if percent != 0.0 {
        let dec = fame_loss(loser_fame, percent, true);
        let fame = ctx
            .worlds
            .mover_mut(loser)
            .and_then(|m| m.player_mut())
            .map(|p| {
                p.progression.fame = p.progression.fame.wrapping_sub(dec).max(0);
                p.progression.fame
            });
        if let Some(fame) = fame {
            let mut out = Out::new();
            out.near(pb::SetFame { fame });
            out.flush(ctx, loser);
            text(ctx, loser, TID_GAME_DECFAMEPOINT, vec![dec.to_string()]);
            crate::audit::emit_for(
                ctx,
                PlayerRef(winner),
                ev::CHAR_PK,
                json!({ "kind": "duel", "loser": loser.0, "gain": gain, "loss": dec }),
            );
            crate::social::hooks::honor_recheck(s, ctx, PlayerRef(loser), true);
        }
    }
}

/// `SubPVP`, solo branch: `victim` was killed by somebody who is not its duel partner. Both duels end without a
/// result: each player is told `DuelCancel{opponent}` and cleared (no `SetDuel`).
pub fn cancel_solo(s: &mut Services, ctx: &mut GameCtx<'_>, victim: ObjId) {
    let now = ctx.clock.game_ms;
    let Some(other) = ctx.worlds.mover(victim).and_then(|v| v.combat.duel.other) else {
        return;
    };
    if valid_player(ctx, other).is_some() {
        let its_opponent = ctx.worlds.mover(other).and_then(|o| o.combat.duel.other);
        tell(
            ctx,
            other,
            Aud::SelfSubject,
            pb::DuelCancel {
                target: its_opponent.map_or(u32::MAX, |o| o.0),
            },
        );
        if let Some(o) = ctx.worlds.mover_mut(other) {
            clear_duel(o, now);
        }
    }
    tell(
        ctx,
        victim,
        Aud::SelfSubject,
        pb::DuelCancel { target: other.0 },
    );
    if let Some(v) = ctx.worlds.mover_mut(victim) {
        clear_duel(v, now);
    }
    s.combat.duel.stats.cancelled += 1;
}

/// The duel part of `OBJMSG_DIE` (`ActionMoverMsg.cpp:850-895`, run by `do_die` right after the death state):
/// the dead player's duel with its killer ends (`SetDuel` to both); a dead party duelist is put out (`state 300`,
/// `SetDuel` to every member of both parties).
pub fn on_die_message(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let (v, a) = (k.victim, k.attacker);
    let now = ctx.clock.game_ms;
    let (Some(victim), Some(attacker)) = (valid_player(ctx, v), valid_player(ctx, a)) else {
        return;
    };
    if victim.combat.duel.duel == 1 && victim.combat.duel.other == Some(a) {
        let (victim_char, attacker_char) = (character_of(victim), character_of(attacker));
        for id in [v, a] {
            if let Some(m) = ctx.worlds.mover_mut(id) {
                clear_duel(m, now);
            }
        }
        send_set_duel(ctx, v, v);
        send_set_duel(ctx, a, a);
        // `CAttackArbiter::OnDiedPVP`: "each side is told the other's id, 3 won, 4 lost" - the client opens the duel result window
        // (`CWndDuelResult`) from this; it follows the `SetDuel` and comes before `MoverDeath`
        tell(
            ctx,
            a,
            Aud::SelfSubject,
            pb::DuelStart {
                id_target: victim_char.0,
                flag: DUEL_WON,
            },
        );
        tell(
            ctx,
            v,
            Aud::SelfSubject,
            pb::DuelStart {
                id_target: attacker_char.0,
                flag: DUEL_LOST,
            },
        );
        s.combat.duel.untrack(v);
        s.combat.duel.untrack(a);
    } else if victim.combat.duel.duel == 2
        && victim.player().map_or(0, |p| p.social.duel_party)
            == attacker
                .player()
                .and_then(|p| p.social.party)
                .map_or(0, |x| x.0)
    {
        if let Some(m) = ctx.worlds.mover_mut(v) {
            m.combat.duel.state = super::STATE_OUT;
        }
        super::party::broadcast_set_duel(s, ctx, v);
    }
}
