//! The party messages (Plan/21 §4.2, 11 §7.4): `PartyInfo` (`CParty::Serialize`), `PartyMember` and the little
//! senders every party module shares. Every function sends in the order of the original call stack.

use super::model::Party;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use flyff_common::{CharacterId, ObjId, PartyId};
use flyff_proto::pb;

/// The name the directory holds for `c` (empty when the character is unknown).
pub fn name_of(svc: &Services, c: CharacterId) -> String {
    svc.presence
        .get(c)
        .map(|e| e.name.clone())
        .unwrap_or_default()
}

/// `CParty::Serialize` (ref/08 §13.1): the wire record of `p`. The duel party id comes from the duel service.
pub fn party_info(svc: &Services, p: &Party) -> pb::PartyInfo {
    pb::PartyInfo {
        party_id: p.id.0,
        kind_troupe: p.kind.wire(),
        level: p.level,
        exp: p.exp,
        point: p.point,
        share_exp_mode: p.exp_share.wire(),
        share_item_mode: p.item_share.wire(),
        id_duel_party: svc.combat.duel.opposing(p.id).map_or(0, |x| x.0),
        mode_time: p.mode_time_wire(),
        name: if p.kind == super::PartyKind::Advanced {
            p.name.clone()
        } else {
            String::new()
        },
        members: p
            .members
            .iter()
            .map(|m| pb::PartyMemberRef {
                id_player: m.id.0,
                removed: !m.is_online(),
            })
            .collect(),
    }
}

/// `AddPartyMember( pParty, idPlayer, pszLeader, pszMember )`: `changed` is the member that joined or left (0 for
/// the remaining member of a dissolved party), `party` the record - absent when the receiver is no longer in a
/// party.
pub fn party_member_msg(
    changed: u32,
    leader_name: &str,
    member_name: &str,
    party: Option<pb::PartyInfo>,
) -> pb::PartyMember {
    pb::PartyMember {
        changed,
        leader_name: leader_name.to_owned(),
        member_name: member_name.to_owned(),
        party,
    }
}

/// Sends `msg` to one avatar (nothing when it has no session).
pub fn to_one(ctx: &mut GameCtx<'_>, obj: ObjId, msg: impl Into<pb::server_msg::Msg>) {
    send_to(ctx, PlayerRef(obj), msg);
}

/// Sends `msg` to every online member of `party`, encoded once, wire `obj_id` 0 (`Audience::Party`).
pub fn to_members(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    party: PartyId,
    msg: impl Into<pb::server_msg::Msg>,
) {
    ctx.broadcast(Some(svc), ObjId::NULL, Audience::Party(party), msg);
}

/// Sends `msg` to the listed avatars in order, wire `obj_id` 0.
pub fn to_list(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    list: &[ObjId],
    msg: impl Into<pb::server_msg::Msg>,
) {
    ctx.broadcast(
        Some(svc),
        ObjId::NULL,
        Audience::Players(list.iter().copied().collect()),
        msg,
    );
}

/// `ErrorParty{code, skill}` to one avatar.
pub fn error(ctx: &mut GameCtx<'_>, to: ObjId, code: u32, skill: u32) {
    send_to(ctx, PlayerRef(to), pb::ErrorParty { code, skill });
}

/// `ERROR_NOLEADER`.
pub const ERROR_NOLEADER: u32 = 200;
/// `ERROR_FULLPARTY`.
pub const ERROR_FULLPARTY: u32 = 201;
/// `ERROR_NOPARTY`.
pub const ERROR_NOPARTY: u32 = 202;
/// `ERROR_DIFFRENTPARTY`.
pub const ERROR_DIFFRENTPARTY: u32 = 203;
/// `ERROR_DIFFERNTUSERNAME`.
pub const ERROR_DIFFERNTUSERNAME: u32 = 204;
/// `ERROR_NOTPARTYPOINT`.
pub const ERROR_NOTPARTYPOINT: u32 = 205;
/// `ERROR_NOTPARTYSKILL`.
pub const ERROR_NOTPARTYSKILL: u32 = 206;
/// `ERROR_NOTTARGET`.
pub const ERROR_NOTTARGET: u32 = 207;
/// `ERROR_NOTMAMBER`.
pub const ERROR_NOTMAMBER: u32 = 208;
/// `ERROR_SAMLEADER`.
pub const ERROR_SAMLEADER: u32 = 209;
