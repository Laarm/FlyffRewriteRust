//! The four couple requests (Plan/21 §11.3; `src:WORLDSERVER/couplehelper.cpp:47-262`,
//! `src:databaseserver/couplehelper.cpp:287-330`): `Propose`, `Refuse`, `CoupleReq` (accept) and `Decouple`.
//!
//! **Trust (VR-001, VR-044).** The actor is the session's avatar. `Propose` names the target by name only to look
//! it up; `CoupleReq` and `Refuse` carry no payload and act on the proposal that was RECORDED for the sender
//! (`PlayerSocial.proposer`), so an accept can never bind to anybody else.
//!
//! The original answered through the database server; here the checks run in the order the two servers had, the
//! memory changes and the database ops leave in the same commit block.

use super::super::honor;
use super::super::relation::{avatar_name, avatar_of, character_of, commit, fill_d};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::movement::intents;
use crate::social::cheer::{play_motion, sfx};
use crate::social::party::wire::to_one;
use flyff_common::defs::honor::{HI_COUNT_CHECK, HS_COUPLE_COUNT};
use flyff_common::defs::neuz::MTI_KNEEDOWN;
use flyff_common::defs::obj::{XI_CHEERRECEIVEEFFECT, XI_CHEERSENDEFFECT};
use flyff_common::defs::text::{
    TID_GAME_COUPLE_E00, TID_GAME_COUPLE_E01, TID_GAME_COUPLE_E02, TID_GAME_COUPLE_E03,
    TID_GAME_COUPLE_E04, TID_GAME_COUPLE_E05, TID_GAME_COUPLE_E06, TID_GAME_COUPLE_M00,
    TID_GAME_COUPLE_S01,
};
use flyff_common::{CharacterId, UnixTime};
use flyff_db::{CharacterState, DbOp};
use flyff_proto::pb;
use flyff_rules::social::{COUPLE_COOLDOWN_S, couple_cooldown_parts};

/// The English text of `TID_GAME_COUPLE_M00` for a game without the text table.
const COOLDOWN_FALLBACK: &str =
    "Unable to propose. You must wait %d day(s) %d hour(s) and %d minute(s).";

/// The sex of an avatar (0 male, 1 female).
fn sex_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<u8> {
    ctx.worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| p.appearance.sex)
}

fn text(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    notice::defined(ctx, who, tid, Vec::new());
}

/// `CCoupleHelper::OnPropose` + `databaseserver OnPropose` + `OnProposeResult`. The refusals in the order of the
/// original: an unknown name (`E00` with the name), a known but offline character (`E00` - FIX: the original sent
/// the text without its argument), the sender himself (`E01`), the same sex while `features.gay_and_lesbian` is off
/// (`E01`), the sender's cooldown (`M00` with the days, hours and minutes left), the sender coupled (`E06`), the
/// target coupled (`E01`). Success: the cooldown (a day; with `features.no_couple_cd` nothing is stored), the target
/// remembers the proposer and gets `ProposeResult`, the proposer kneels and the sender hears `S01`.
pub fn propose(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, target_name: &str) {
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let name_arg = vec![target_name.to_owned()];
    let Some(entry) = svc.presence.by_name(target_name) else {
        notice::defined(ctx, who, TID_GAME_COUPLE_E00, name_arg);
        return;
    };
    let (target, target_display) = (entry.id, entry.name.clone());
    if entry.state == CharacterState::Deleted {
        notice::defined(ctx, who, TID_GAME_COUPLE_E00, name_arg);
        return;
    }
    let Some(t) = avatar_of(svc, ctx, target) else {
        notice::defined(ctx, who, TID_GAME_COUPLE_E00, name_arg);
        return;
    };
    if target == me {
        text(ctx, who, TID_GAME_COUPLE_E01);
        return;
    }
    if !ctx.cfg.rules.features.gay_and_lesbian && sex_of(ctx, who) == sex_of(ctx, t) {
        text(ctx, who, TID_GAME_COUPLE_E01);
        return;
    }
    let now = ctx.clock.unix_ms.to_unix_time();
    if let Some(until) = svc.social.couple.cooldown_of(me).filter(|u| u.0 > now.0) {
        let left = (until.0 - now.0).max(1);
        let (d, h, m) = couple_cooldown_parts(left);
        let template = ctx.data.texts.get(TID_GAME_COUPLE_M00);
        let template = if template.is_empty() || template == "error" {
            COOLDOWN_FALLBACK
        } else {
            template
        };
        let line = fill_d(template, &[d, h, m]);
        notice::text(ctx, who, &line);
        return;
    }
    if svc.social.couple.get(me).is_some() {
        text(ctx, who, TID_GAME_COUPLE_E06);
        return;
    }
    if svc.social.couple.get(target).is_some() {
        text(ctx, who, TID_GAME_COUPLE_E01);
        return;
    }
    // the cooldown (SetPropose): memory and database together; none at all with `no_couple_cd`
    if !ctx.cfg.rules.features.no_couple_cd {
        let until = UnixTime(now.0.saturating_add(COUPLE_COOLDOWN_S));
        svc.social.couple.set_cooldown(me, Some(until));
        commit(
            ctx,
            "CoupleProposal",
            vec![DbOp::SetCoupleProposal {
                proposer_id: me,
                cooldown_until: Some(until),
            }],
        );
    }
    // the target remembers who asked; a newer proposal replaces an older one
    if let Some(p) = ctx.worlds.player_mut(t.0).and_then(|m| m.player_mut()) {
        p.social.proposer = Some(me);
    }
    let my_name = avatar_name(ctx, who);
    to_one(
        ctx,
        t.0,
        pb::ProposeResult {
            proposer: me.0,
            name: my_name,
        },
    );
    play_propose_animation(ctx, who, t);
    notice::defined(ctx, who, TID_GAME_COUPLE_S01, vec![target_display]);
}

/// `CCoupleHelper::PlayProposeAnimation`: the proposer turns to the target, kneels once and the two effects play.
fn play_propose_animation(ctx: &mut GameCtx<'_>, proposer: PlayerRef, target: PlayerRef) {
    let Some(towards) = ctx.worlds.core(target.0).map(|c| c.pos) else {
        return;
    };
    play_motion(ctx, proposer.0, towards, MTI_KNEEDOWN);
    sfx(ctx, proposer.0, XI_CHEERSENDEFFECT);
    sfx(ctx, target.0, XI_CHEERRECEIVEEFFECT);
    if let Some(mv) = ctx.worlds.player_mut(proposer.0) {
        intents::force_pose_broadcast(mv);
    }
}

/// The proposal that was made to `who`.
fn pending_proposer(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<CharacterId> {
    ctx.worlds
        .player(who.0)
        .and_then(|m| m.player())
        .and_then(|p| p.social.proposer)
}

fn clear_proposer(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.social.proposer = None;
    }
}

/// `CCoupleHelper::OnRefuse`: no proposal -> `E02`; else the proposer (when online) hears `E03`, the sender `E04`
/// with the proposer's name, and the proposal is gone.
pub fn refuse(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(proposer) = pending_proposer(ctx, who) else {
        text(ctx, who, TID_GAME_COUPLE_E02);
        return;
    };
    if let Some(po) = avatar_of(svc, ctx, proposer) {
        text(ctx, po, TID_GAME_COUPLE_E03);
    }
    if let Some(name) = svc.presence.get(proposer).map(|e| e.name.clone()) {
        notice::defined(ctx, who, TID_GAME_COUPLE_E04, vec![name]);
    }
    clear_proposer(ctx, who);
}

/// `CoupleReq`: `OnCouple` + `databaseserver OnCouple` + `OnCoupleResult`. No proposal -> `E02`. The proposer may be
/// OFFLINE (the original only needed the ids). A coupled proposer (`E06`) or a coupled sender (`E01`) is told to the
/// SENDER and the proposal stays. Otherwise one new couple is formed (the higher id is `first`), each online side
/// gets `CoupleResult` and `HS_COUPLE_COUNT +1` (an offline proposer misses the counter, as the original), the proposal
/// is cleared and `couple.create` is audited.
pub fn accept(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let Some(proposer) = pending_proposer(ctx, who) else {
        text(ctx, who, TID_GAME_COUPLE_E02);
        return;
    };
    if svc.social.couple.get(proposer).is_some() {
        text(ctx, who, TID_GAME_COUPLE_E06);
        return;
    }
    if svc.social.couple.get(me).is_some() {
        text(ctx, who, TID_GAME_COUPLE_E01);
        return;
    }
    let proposer_name = match svc.presence.get(proposer) {
        Some(e) if e.state != CharacterState::Deleted && proposer != me => e.name.clone(),
        _ => {
            // the proposer was deleted meanwhile: the proposal is void
            clear_proposer(ctx, who);
            text(ctx, who, TID_GAME_COUPLE_E02);
            return;
        }
    };
    let now = ctx.clock.unix_ms.to_unix_time();
    let id = svc.social.couple.alloc_id();
    let couple = svc.social.couple.couple(id, proposer, me, now);
    commit(
        ctx,
        "CoupleCreate",
        DbOp::couple_ops(couple.record()).into(),
    );
    let my_name = avatar_name(ctx, who);
    if let Some(po) = avatar_of(svc, ctx, proposer) {
        to_one(
            ctx,
            po.0,
            pb::CoupleResult {
                partner: me.0,
                name: my_name,
            },
        );
        honor::add(svc, ctx, po, HS_COUPLE_COUNT, HI_COUNT_CHECK, None);
    }
    to_one(
        ctx,
        who.0,
        pb::CoupleResult {
            partner: proposer.0,
            name: proposer_name,
        },
    );
    honor::add(svc, ctx, who, HS_COUPLE_COUNT, HI_COUNT_CHECK, None);
    clear_proposer(ctx, who);
    emit_audit(
        ctx,
        who,
        flyff_db::audit::social::couple_create(id, me, proposer),
    );
}

/// `Decouple`: not coupled -> `E05`. The couple is deleted, each online side (the sender first) gets
/// `DecoupleResult`, its `HS_COUPLE_LV` counter goes to 0 (and the list follows when the need is not below the old
/// counter). The SELECTED title is not cleared (KQ-39 f, KEEP: the next recheck drops it). The couple buffs go at
/// the next cadence step of each member.
pub fn decouple(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let Some(couple) = svc.social.couple.remove_of(me) else {
        text(ctx, who, TID_GAME_COUPLE_E05);
        return;
    };
    commit(
        ctx,
        "CoupleBreak",
        vec![DbOp::DeleteCouple {
            couple_id: couple.id,
        }],
    );
    let partner = couple.partner_of(me).unwrap_or(me);
    for member in [me, partner] {
        if let Some(o) = avatar_of(svc, ctx, member) {
            to_one(ctx, o.0, pb::DecoupleResult {});
            honor::reset_couple_level(ctx, o);
        }
    }
    emit_audit(
        ctx,
        who,
        flyff_db::audit::social::couple_break(couple.id, me, partner),
    );
}

fn emit_audit(ctx: &GameCtx<'_>, who: PlayerRef, ev: flyff_db::AuditEvent) {
    let ev = match crate::audit::actor_of(ctx, who) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
}

// ---- the client messages -------------------------------------------------------------------------------------------

/// `Propose{target_name}`.
pub fn on_propose(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::Propose) {
    propose(svc, ctx, who, &m.target_name);
}

/// `Refuse`.
pub fn on_refuse(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, _m: pb::Refuse) {
    refuse(svc, ctx, who);
}

/// `CoupleReq`.
pub fn on_couple_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, _m: pb::CoupleReq) {
    accept(svc, ctx, who);
}

/// `Decouple`.
pub fn on_decouple(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, _m: pb::Decouple) {
    decouple(svc, ctx, who);
}

/// A deleted character cannot be accepted any more: every online player whose pending proposer it is forgets the
/// proposal (21 §3.6).
pub fn forget_proposals_of(ctx: &mut GameCtx<'_>, deleted: CharacterId) {
    let owners: Vec<_> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|o| {
            ctx.worlds
                .player(*o)
                .and_then(|m| m.player())
                .is_some_and(|p| p.social.proposer == Some(deleted))
        })
        .collect();
    for o in owners {
        clear_proposer(ctx, PlayerRef(o));
    }
}
