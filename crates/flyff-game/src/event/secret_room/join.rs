//! The secret-room state in the join batch (Plan/22 §11.4, §19.2; legacy `CSecretRoomMng::GetAllInfo`,
//! `SecretRoom.cpp:326-362`, called unconditionally from `CDPDatabaseClient::OnJoin`): [`SecretRoomInfoStep`] at
//! [`JoinStep::SecretRoomInfo`] (after the five `GuildCombat` messages and the conditional `Gc1to1NowState`, before
//! `TaxAllInfo`), replayed on a re-attach.
//!
//! * the manager `Close` (flag 24 on OR off): `SecretRoomMngState{1, 0}` to EVERY joiner (the idle join batch grows by
//!   this one message, difference e of the task),
//! * `LineUp`: `{3, remain}` to the master of a tendering guild; nobody else gets anything,
//! * every other state: a member of the line-up of a side gets `{manager state, remain}`; during `War` a member of a
//!   FAILED guild gets `{101, 0}`, any other `{side state, side remain}` followed by the `SecretRoomInfo` guild list of
//!   the side (and the function stops).

use super::wire;
use super::{SecretRoomService, SrMngState, SrWarState, remain_ms};
use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::{CharacterId, GameMs, GuildId};
use flyff_proto::pb::server_msg::Msg;

/// `GetAllInfo` as a join step.
pub struct SecretRoomInfoStep;

impl JoinContributor for SecretRoomInfoStep {
    fn step(&self) -> JoinStep {
        JoinStep::SecretRoomInfo
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(c) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let guilds = &cx.svc.social.guild;
        let msgs = join_messages(
            &cx.svc.event.secret_room,
            c,
            guilds.guild_of(c),
            guilds.is_master(c),
            cx.clock.game_ms,
        );
        for m in msgs {
            out.to_session(cx.session, 0, m);
        }
    }
}

/// The messages `GetAllInfo` sends to the character `c` (member of `guild`, `is_master` of it) at `now`, in order.
pub fn join_messages(
    sr: &SecretRoomService,
    c: CharacterId,
    guild: Option<GuildId>,
    is_master: bool,
    now: GameMs,
) -> Vec<Msg> {
    let mut out: Vec<Msg> = Vec::new();
    if sr.state == SrMngState::Close {
        out.push(wire::mng_state(SrMngState::Close.code(), 0).into());
        return out;
    }
    for (k, side) in &sr.sides {
        let tender = guild.and_then(|g| side.tender(g));
        if sr.state == SrMngState::LineUp {
            if tender.is_some() && is_master {
                out.push(wire::mng_state(sr.state.code(), remain_ms(sr.deadline, now)).into());
            }
        } else if let Some(t) = tender.filter(|t| t.has_member(c)) {
            if sr.state != SrMngState::War {
                out.push(wire::mng_state(sr.state.code(), remain_ms(sr.deadline, now)).into());
            } else if t.war == SrWarState::Failed {
                out.push(wire::mng_state(super::SrSideState::Close.code(), 0).into());
            } else {
                out.push(wire::mng_state(side.state.code(), remain_ms(side.deadline, now)).into());
                let totals = [
                    sr.cfg
                        .monster_total(crate::event::scripts::SrMonsterKind::Normal),
                    sr.cfg
                        .monster_total(crate::event::scripts::SrMonsterKind::MidBoss),
                    sr.cfg
                        .monster_total(crate::event::scripts::SrMonsterKind::Boss),
                ];
                out.push(wire::guild_list(*k, &side.tenders, guild, totals).into());
                return out;
            }
        }
    }
    out
}
