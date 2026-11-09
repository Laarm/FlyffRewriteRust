//! Joining, leaving and dissolving a campus (Plan/21 §12.3; `src:WORLDSERVER/CampusHelper.cpp:30-140, 472-489`,
//! `databaseserver/CampusDBCtrl.cpp`).
//!
//! Every change is one memory change plus ONE `AtomicBatch` (`CampusJoin`, `CampusLeave`) of the ops that belong
//! together: a new campus with its two members, a pupil's removal with the penalty of the requester and the
//! dissolution that follows it. The players concerned hear `CampusUpdate` / `CampusRemove` and the texts of the
//! original, in the original order.

use super::points::{PointReason, apply};
use super::{reset_mirror, send_update};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::party::wire::{name_of, to_one};
use crate::social::relation::{avatar_name, avatar_of, character_of, commit};
use flyff_common::CharacterId;
use flyff_common::defs::text::{TID_GAME_TS_ACCSTUDENT, TID_GAME_TS_ACCTEATHER, TID_GAME_TS_BREAK};
use flyff_db::audit::social as audit;
use flyff_db::{AuditEvent, CampusMemberRecord, CampusRole, DbOp};
use flyff_proto::pb;
use flyff_rules::social::CAMPUS_REMOVE_POINTS;

fn quoted(name: &str) -> Vec<String> {
    vec![format!("\"{name}\"")]
}

fn emit(ctx: &GameCtx<'_>, who: Option<PlayerRef>, ev: AuditEvent) {
    let ev = match who.and_then(|w| crate::audit::actor_of(ctx, w)) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
}

/// `OnAddCampusMember`: `pupil` joins the campus of `master` (a new campus when the master has none). The pupil
/// first (`ACCSTUDENT` with his own quoted name), then the master (`ACCTEATHER`) for an existing campus; the master
/// first for a new one; then `CampusUpdate` to every online member in id order. Audited as `campus.join`.
pub(super) fn join(svc: &mut Services, ctx: &mut GameCtx<'_>, master: PlayerRef, pupil: PlayerRef) {
    let (Some(mc), Some(pc)) = (character_of(ctx, master), character_of(ctx, pupil)) else {
        return;
    };
    let now = ctx.clock.unix_ms.to_unix_time();
    let record = |c: CharacterId, id: u32, role: CampusRole| CampusMemberRecord {
        character_id: c,
        campus_id: id,
        role,
        joined_at: now,
    };
    let existing = svc.social.campus.campus_id_of(mc);
    let (id, ops, fresh) = match existing {
        Some(id) => {
            let owns = svc.social.campus.get(id).is_some_and(|c| c.master == mc);
            if !owns || !svc.social.campus.add_pupil(id, pc) {
                tracing::error!(
                    campus = id,
                    master = mc.0,
                    pupil = pc.0,
                    "campus: adding the pupil failed"
                );
                return;
            }
            (
                id,
                vec![DbOp::UpsertCampusMember(record(pc, id, CampusRole::Pupil))],
                false,
            )
        }
        None => {
            let id = svc.social.campus.alloc_id();
            svc.social.campus.create(id, mc, pc, now);
            let members = vec![
                record(mc, id, CampusRole::Master),
                record(pc, id, CampusRole::Pupil),
            ];
            (id, DbOp::campus_ops(id, now, members), true)
        }
    };
    commit(ctx, "CampusJoin", ops);
    for who in [master, pupil] {
        if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
            p.social.campus = id;
        }
    }
    let (master_name, pupil_name) = (avatar_name(ctx, master), avatar_name(ctx, pupil));
    if fresh {
        notice::defined(ctx, master, TID_GAME_TS_ACCTEATHER, quoted(&master_name));
        notice::defined(ctx, pupil, TID_GAME_TS_ACCSTUDENT, quoted(&pupil_name));
    } else {
        notice::defined(ctx, pupil, TID_GAME_TS_ACCSTUDENT, quoted(&pupil_name));
        notice::defined(ctx, master, TID_GAME_TS_ACCTEATHER, quoted(&master_name));
    }
    send_update(svc, ctx, id);
    if fresh {
        emit(ctx, Some(master), audit::campus_join(id, mc, "master"));
    }
    emit(ctx, Some(pupil), audit::campus_join(id, pc, "pupil"));
}

/// `CampusRemoveMember{target}` (`OnRemoveCampusMember`, also GM `/rcm`): the requester must be in a campus with
/// `target` as a member; a MASTER naming a pupil removes that pupil, a PUPIL naming his master removes himself,
/// anything else is ignored. The requester then pays 5 campus points (`PointReason::Delete`, the balance may go
/// negative) - on both paths, KEEP.
pub fn remove_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    requester: PlayerRef,
    target: CharacterId,
) {
    if super::config(ctx).is_none() {
        return;
    }
    let Some(rc) = character_of(ctx, requester) else {
        return;
    };
    let Some(campus) = svc.social.campus.campus_of(rc) else {
        return;
    };
    if !campus.is_member(target) {
        return;
    }
    let pupil = match (campus.role_of(rc), campus.role_of(target)) {
        (Some(CampusRole::Master), Some(CampusRole::Pupil)) => target,
        (Some(CampusRole::Pupil), Some(CampusRole::Master)) => rc,
        _ => return,
    };
    let mut ops = leave(svc, ctx, pupil, "remove");
    ops.push(apply(
        &mut svc.social.campus,
        ctx,
        rc,
        Some(requester),
        -CAMPUS_REMOVE_POINTS,
        PointReason::Delete,
    ));
    commit(ctx, "CampusLeave", ops);
}

/// A pupil leaves his campus (the memory side of `OnRemoveCampusMember` of the world): the pupil hears
/// `CampusRemove` and `TS_BREAK` with the master's quoted name, the master `TS_BREAK` with the pupil's, then a
/// campus with fewer than two members is dissolved (`CampusRemove` to the rest) and otherwise everybody left gets
/// `CampusUpdate`. Returns the database ops (`DeleteCampusMember`, and `DeleteCampus` for a dissolution).
pub(crate) fn leave(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    pupil: CharacterId,
    reason: &str,
) -> Vec<DbOp> {
    let Some(id) = svc.social.campus.campus_id_of(pupil) else {
        return Vec::new();
    };
    let Some(master) = svc.social.campus.get(id).map(|c| c.master) else {
        return Vec::new();
    };
    if master == pupil {
        return dissolve(svc, ctx, id, reason);
    }
    let mut ops = vec![DbOp::DeleteCampusMember {
        character_id: pupil,
    }];
    svc.social.campus.remove_pupil(pupil);
    let (pupil_name, master_name) = (name_of(svc, pupil), name_of(svc, master));
    let pupil_ref = avatar_of(svc, ctx, pupil);
    if let Some(po) = pupil_ref {
        to_one(ctx, po.0, pb::CampusRemove { id_campus: id });
        reset_mirror(ctx, po);
        notice::defined(ctx, po, TID_GAME_TS_BREAK, quoted(&master_name));
    }
    if let Some(mo) = avatar_of(svc, ctx, master) {
        notice::defined(ctx, mo, TID_GAME_TS_BREAK, quoted(&pupil_name));
    }
    emit(
        ctx,
        pupil_ref,
        audit::campus_leave(id, pupil, "pupil", reason),
    );
    let left = svc.social.campus.get(id).map_or(0, |c| c.len());
    if left < 2 {
        ops.extend(dissolve(svc, ctx, id, "dissolve"));
    } else {
        send_update(svc, ctx, id);
    }
    ops
}

/// `AddAllMemberRemoveCampus` + `RemoveCampus`: every member leaves; the online ones hear `CampusRemove`. Returns
/// `DeleteCampus` (the member rows go with it by cascade).
fn dissolve(svc: &mut Services, ctx: &mut GameCtx<'_>, id: u32, reason: &str) -> Vec<DbOp> {
    let Some(campus) = svc.social.campus.dissolve(id) else {
        return Vec::new();
    };
    for (m, role) in campus.members() {
        let online = avatar_of(svc, ctx, m);
        if let Some(o) = online {
            to_one(ctx, o.0, pb::CampusRemove { id_campus: id });
            reset_mirror(ctx, o);
        }
        let role = if role == CampusRole::Master {
            "master"
        } else {
            "pupil"
        };
        emit(ctx, online, audit::campus_leave(id, m, role, reason));
    }
    vec![DbOp::DeleteCampus { campus_id: id }]
}

/// The campus part of a character deletion (21 §3.6, 13 §14.2): the character leaves like a deletion - no penalty,
/// the dissolution rules apply (a deleted master takes the campus with him) - and the ops are appended to the
/// `CharacterDelete` batch after `SoftDeleteCharacter`. The character is offline, the others are told.
pub fn on_character_deleted(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    character: CharacterId,
    ops: &mut Vec<DbOp>,
) {
    // a master dissolves the campus (`leave` -> `DeleteCampus`, the member rows go by cascade)
    ops.extend(leave(svc, ctx, character, "delete"));
}

/// The graduation of a pupil at level 75 (`SendRemoveCampusMember` of `SetLevelUpReward`): leaves like a removal,
/// without the penalty. Returns the ops.
pub(crate) fn graduate(svc: &mut Services, ctx: &mut GameCtx<'_>, pupil: CharacterId) -> Vec<DbOp> {
    leave(svc, ctx, pupil, "graduate")
}
