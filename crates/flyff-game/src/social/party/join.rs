//! The party steps of the join batch (Plan/21 §3.5, 11 §2.5): the stored party name (step 9, `AddPartyName`) and the
//! party record of a member (after `Tag`, `PartyMember`). Both are also part of the re-attach replay.

use super::wire::{name_of, party_info, party_member_msg};
use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// Step 9 `AddPartyName{name}`: the stored advanced-party name of the character, `""` when it has none.
pub struct AddPartyNameStep;

impl JoinContributor for AddPartyNameStep {
    fn step(&self) -> JoinStep {
        JoinStep::AddPartyName
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(character) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let name = cx
            .svc
            .social
            .party
            .names
            .of(character)
            .unwrap_or_default()
            .to_owned();
        out.to_session(cx.session, 0, pb::AddPartyName { name });
    }
}

/// `PartyMember{changed: 0, leader_name, member_name: "", party}` for a character that is (still) in a party
/// (`CDPDatabaseClient::OnJoin`); nothing otherwise.
pub struct PartyMemberStep;

impl JoinContributor for PartyMemberStep {
    fn step(&self) -> JoinStep {
        JoinStep::PartyMember
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(character) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let Some(p) = cx.svc.social.party.party_record_of(character) else {
            return;
        };
        let leader = p
            .leader_id()
            .map(|l| name_of(cx.svc, l))
            .unwrap_or_default();
        out.to_session(
            cx.session,
            0,
            party_member_msg(0, &leader, "", Some(party_info(cx.svc, p))),
        );
    }
}
