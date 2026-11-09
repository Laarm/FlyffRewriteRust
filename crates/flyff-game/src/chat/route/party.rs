//! Party chat (Plan/21 §10.4; WP-SOCIAL-03): `/p` and the `PartyChat` delivery.

use super::filter::{ChatChannel, delivers};
use crate::admin::chat_tap::{self, ChatChannel as TapChannel};
use crate::chat::ingress::{SnoopKind, snoop_copy};
use crate::chat::{actor, send_to};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use crate::social::party::wire::ERROR_NOPARTY;
use flyff_common::ObjId;
use flyff_proto::pb;

/// `route::party(g, speaker, text)` (21 §10.4): a speaker that is a member of its party sends `PartyChat{obj_id:
/// speaker, name, text}` to every online member, itself included, in any world (`Audience::Party`); a speaker
/// without a party gets `ErrorParty{202}`. The text is already cleaned by `channel_text`; `DONTALK_MODE` does not
/// block party chat. The snoop copies of 23 §6.12 go after each delivery.
pub fn party(svc: &mut Services, ctx: &mut GameCtx<'_>, speaker: PlayerRef, text: &str) {
    let Some(a) = actor(ctx, speaker) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(a.character) else {
        send_to(
            ctx,
            speaker,
            pb::ErrorParty {
                code: ERROR_NOPARTY,
                skill: 0,
            },
        );
        return;
    };
    let msg = pb::PartyChat {
        obj_id: speaker.0.0,
        name: a.name.clone(),
        text: text.to_owned(),
    };
    let speaker_obj = speaker.0;
    super::broadcast_filtered(
        svc,
        ctx,
        ObjId::NULL,
        Audience::Party(pid),
        msg,
        |_, listener, worlds| delivers(svc, worlds, listener, speaker_obj, ChatChannel::Party),
    );
    let listeners: Vec<_> = svc
        .social
        .party
        .get(pid)
        .map(|p| {
            p.members
                .iter()
                .filter(|m| m.is_online())
                .map(|m| m.id)
                .collect()
        })
        .unwrap_or_default();
    for l in listeners {
        snoop_copy(svc, ctx, SnoopKind::Party, speaker, l, text);
    }
    // the admin console's chat tap (25 §4.9): one line per message, not per listener
    chat_tap::record(svc, ctx, speaker, TapChannel::Party, None, text);
}
