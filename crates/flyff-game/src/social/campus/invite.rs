//! Campus invitations (Plan/21 §12.3; `src:WORLDSERVER/CampusHelper.cpp:240-330, 434-470`,
//! `src:WORLDSERVER/DPSrvr.cpp:12640-12680`, `src:_Interface/FuncTextCmd.cpp:5104-5133`).
//!
//! **Trust (VR-044, PL-090).** An invitation is a server-side record `(Campus, requester, target)`; `CampusAccept`
//! and `CampusRefuse` need the live record of the named requester, otherwise they are ignored silently. The ids the
//! client sends are lookup keys only. The original accepted any online requester.
//!
//! Every text of [`invite_able`] goes to the REQUESTER with the names quoted (`"name"`), as the original call sites
//! formatted them.

use super::{Campus, config, members};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::social::invites::{InviteKey, InviteKind, InviteKindTag, PendingInvite};
use crate::social::party::wire::to_one;
use crate::social::relation::{avatar_name, avatar_of, character_of};
use flyff_common::defs::job::MAX_EXPERT;
use flyff_common::defs::text::{
    TID_DIAG_0060, TID_DIAG_0061, TID_GAME_TS_ALREADY, TID_GAME_TS_FULLSTUDENT,
    TID_GAME_TS_NOTLEVEL, TID_GAME_TS_NOTQUEST, TID_GAME_TS_REFUSAL, TID_GAME_TS_WANTMYTSP,
    TID_GAME_TS_WANTYOURTSP,
};
use flyff_common::{CharacterId, GameMs};
use flyff_proto::pb;
use flyff_rules::social::{campus_master_capable, campus_max_pupils, campus_pupil_capable};

/// `"name"` as the original formatted a name argument (`"\"%s\""`).
fn quoted(name: &str) -> Vec<String> {
    vec![format!("\"{name}\"")]
}

/// What `IsInviteAble` reads of one player.
struct Side {
    who: PlayerRef,
    character: CharacterId,
    name: String,
    level: i32,
    job: i32,
}

fn side(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Side> {
    let m = ctx.worlds.player(who.0)?;
    let p = m.player()?;
    Some(Side {
        who,
        character: p.ident.character_id,
        name: p.ident.name.clone(),
        level: m.mv.level,
        job: m.mv.job,
    })
}

impl Side {
    fn master_capable(&self) -> bool {
        campus_master_capable(self.level)
    }

    fn pupil_capable(&self) -> bool {
        campus_pupil_capable(self.job, MAX_EXPERT as i32)
    }
}

/// `IsCompleteCampusQuest`: with `gameplay.campus_requires_quest` off (the default, PL-097/PD-22) the list of
/// required quests counts as empty; on, every quest of `Campus.lua` must be complete (the shipped quest data is
/// empty, so no quest id resolves and nobody qualifies).
fn quests_done(ctx: &GameCtx<'_>, who: PlayerRef) -> bool {
    if !ctx.cfg.rules.gameplay.campus_requires_quest {
        return true;
    }
    let data = ctx.data;
    let Some(cfg) = data.lua.campus.as_ref() else {
        return true;
    };
    let Some(p) = ctx.worlds.player(who.0).and_then(|m| m.player()) else {
        return false;
    };
    cfg.quests.iter().all(|q| p.quests.is_complete(q.quest))
}

/// The pupils a campus may take by the master's points.
fn is_full(campus: Option<&Campus>, points: i32) -> bool {
    campus.is_some_and(|c| c.pupils.len() >= campus_max_pupils(points))
}

/// `CCampusHelper::IsInviteAble( pRequest, pTarget )`:
///
/// 1. `points(req) < 0` -> `WANTMYTSP`; `points(tgt) < 0` -> `WANTYOURTSP`;
/// 2. requester master-capable (level >= 91) and target pupil-capable (job < `MAX_EXPERT`): the quests
///    (`NOTQUEST` with the requester's name), the master's pupils against `campus_max_pupils(points)` (`FULLSTUDENT`),
///    the target in a campus (`ALREADY` with the target's name);
/// 3. requester pupil-capable and target master-capable: the requester in a campus (`ALREADY`, requester), the
///    target's quests (`NOTQUEST`, target), the target's pupils (`FULLSTUDENT`, target);
/// 4. anything else `NOTLEVEL`.
///
/// Every text goes to the requester. Returns whether the invitation is allowed.
pub fn invite_able(svc: &Services, ctx: &mut GameCtx<'_>, req: PlayerRef, tgt: PlayerRef) -> bool {
    let (Some(r), Some(t)) = (side(ctx, req), side(ctx, tgt)) else {
        return false;
    };
    let campus = &svc.social.campus;
    let (rp, tp) = (campus.points(r.character), campus.points(t.character));
    if rp < 0 {
        notice::defined(ctx, req, TID_GAME_TS_WANTMYTSP, Vec::new());
        return false;
    }
    if tp < 0 {
        notice::defined(ctx, req, TID_GAME_TS_WANTYOURTSP, Vec::new());
        return false;
    }
    if r.master_capable() && t.pupil_capable() {
        if !quests_done(ctx, r.who) {
            notice::defined(ctx, req, TID_GAME_TS_NOTQUEST, quoted(&r.name));
            return false;
        }
        if is_full(campus.campus_of(r.character), rp) {
            notice::defined(ctx, req, TID_GAME_TS_FULLSTUDENT, quoted(&r.name));
            return false;
        }
        if campus.campus_of(t.character).is_some() {
            notice::defined(ctx, req, TID_GAME_TS_ALREADY, quoted(&t.name));
            return false;
        }
        true
    } else if r.pupil_capable() && t.master_capable() {
        if campus.campus_of(r.character).is_some() {
            notice::defined(ctx, req, TID_GAME_TS_ALREADY, quoted(&r.name));
            return false;
        }
        if !quests_done(ctx, t.who) {
            notice::defined(ctx, req, TID_GAME_TS_NOTQUEST, quoted(&t.name));
            return false;
        }
        if is_full(campus.campus_of(t.character), tp) {
            notice::defined(ctx, req, TID_GAME_TS_FULLSTUDENT, quoted(&t.name));
            return false;
        }
        true
    } else {
        notice::defined(ctx, req, TID_GAME_TS_NOTLEVEL, Vec::new());
        false
    }
}

/// `OnInviteCampusMember( pRequest, pTarget )`: when the invitation is allowed it is recorded for
/// `gameplay.invite_ttl_s` and the target gets `CampusInviteNtf`. Returns whether it was sent.
pub fn invite(svc: &mut Services, ctx: &mut GameCtx<'_>, req: PlayerRef, tgt: PlayerRef) -> bool {
    if config(ctx).is_none() || req == tgt {
        return false;
    }
    if !invite_able(svc, ctx, req, tgt) {
        return false;
    }
    let (Some(rc), Some(tc)) = (character_of(ctx, req), character_of(ctx, tgt)) else {
        return false;
    };
    let ttl = u64::from(ctx.cfg.rules.gameplay.invite_ttl_s) * 1000;
    svc.social.invites.offer(PendingInvite {
        kind: InviteKind::Campus,
        from: rc,
        to: tc,
        expires: GameMs(ctx.clock.game_ms.0.saturating_add(ttl)),
    });
    let name = avatar_name(ctx, req);
    to_one(
        ctx,
        tgt.0,
        pb::CampusInviteNtf {
            requester: rc.0,
            name,
        },
    );
    true
}

/// `CampusInviteReq{target}` (`OnInviteCampusMember`): an online target, then [`invite`].
pub fn on_invite_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::CampusInviteReq,
) {
    if config(ctx).is_none() {
        return;
    }
    if let Some(tgt) = avatar_of(svc, ctx, CharacterId(m.target)) {
        invite(svc, ctx, who, tgt);
    }
}

/// `/campusinvite <name>` (row 12, KQ-39 g): the name goes through the directory. An UNKNOWN name answers
/// `TID_DIAG_0060 "name"` and an OFFLINE one `TID_DIAG_0061` - swapped against `/partyinvite`, KEEP. An online target is
/// invited like a `CampusInviteReq`, and its `QueryPlayerDataNtf` goes to the requester whether or not the invitation
/// was allowed.
pub fn cmd_invite(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let name = args.token().to_owned();
    if config(cx.ctx).is_none() {
        return Ok(CmdDone::on(name));
    }
    match cx
        .svc
        .presence
        .by_name(&name)
        .map(|e| (e.id, e.online.is_some()))
    {
        Some((id, true)) => {
            if let Some(tgt) = avatar_of(cx.svc, cx.ctx, id) {
                invite(cx.svc, cx.ctx, me, tgt);
                if let Some(n) = crate::social::player_data::ntf_of(cx.svc, id) {
                    to_one(cx.ctx, me.0, n);
                }
            } else {
                notice::defined(cx.ctx, me, TID_DIAG_0061, quoted(&name));
            }
        }
        Some((_, false)) => notice::defined(cx.ctx, me, TID_DIAG_0061, quoted(&name)),
        None => notice::defined(cx.ctx, me, TID_DIAG_0060, quoted(&name)),
    }
    Ok(CmdDone::on(name))
}

/// `CampusAccept{requester}` (`OnAcceptCampusMember`): needs the live invitation of that requester to the sender
/// (VR-044, PL-090), an online requester, and the roles: the master-capable side is the master and the
/// invitation is checked once more. A new campus is created when the master has none.
pub fn on_accept(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::CampusAccept) {
    if config(ctx).is_none() {
        return;
    }
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let requester = CharacterId(m.requester);
    let key = InviteKey {
        kind: InviteKindTag::Campus,
        from: requester,
        to: me,
    };
    if svc.social.invites.take(key, ctx.clock.game_ms).is_none() {
        return;
    }
    let Some(req) = avatar_of(svc, ctx, requester) else {
        return;
    };
    let (Some(r), Some(t)) = (side(ctx, req), side(ctx, who)) else {
        return;
    };
    // the roles, as the original tested them (IsInviteAble always with the requester first)
    let roles = if r.master_capable() && t.pupil_capable() && invite_able(svc, ctx, req, who) {
        Some((req, who))
    } else if t.master_capable() && r.pupil_capable() && invite_able(svc, ctx, req, who) {
        Some((who, req))
    } else {
        None
    };
    if let Some((master, pupil)) = roles {
        members::join(svc, ctx, master, pupil);
    }
}

/// `CampusRefuse{requester}` (`OnRefuseCampusMember`): consumes the LIVE invitation (none or an expired one: ignored)
/// and tells the requester, when online, `TID_GAME_TS_REFUSAL` with the refuser's name.
pub fn on_refuse(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::CampusRefuse) {
    if config(ctx).is_none() {
        return;
    }
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let requester = CharacterId(m.requester);
    let key = InviteKey {
        kind: InviteKindTag::Campus,
        from: requester,
        to: me,
    };
    if svc.social.invites.take(key, ctx.clock.game_ms).is_none() {
        return;
    }
    if let Some(req) = avatar_of(svc, ctx, requester) {
        let name = avatar_name(ctx, who);
        notice::defined(ctx, req, TID_GAME_TS_REFUSAL, quoted(&name));
    }
}

/// `CampusRemoveMember{target}` (`OnRemoveCampusMember`): a target of 0 is ignored, the rest is
/// [`members::remove_member`].
pub fn on_remove_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::CampusRemoveMember,
) {
    if config(ctx).is_none() || m.target == 0 {
        return;
    }
    members::remove_member(svc, ctx, who, CharacterId(m.target));
}
