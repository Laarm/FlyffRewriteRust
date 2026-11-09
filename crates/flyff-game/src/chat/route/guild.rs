//! Guild chat (Plan/21 §10.4; WP-SOCIAL-14): `/g` and the `GuildChat` delivery. The handler `cmd_guild`
//! (row 9, `commands.rs`) shares its pipeline with `cmd_party`; this is the delivery step.

use super::filter::{ChatChannel, delivers};
use crate::admin::chat_tap::{self, ChatChannel as TapChannel};
use crate::chat::actor;
use crate::chat::ingress::{SnoopKind, snoop_copy};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use flyff_common::{CharacterId, ObjId};
use flyff_proto::pb;

/// `route::guild(g, speaker, text)` (21 §10.4): a speaker that belongs to a guild sends `GuildChat{obj_id: speaker,
/// name, text}` to every online member, itself included, in any world (`Audience::Guild`, encoded once, filtered
/// by [`delivers`] with `ChatChannel::Guild`: a member who ignores the speaker is skipped, WP-SOCIAL-20); a speaker
/// without a guild sends nothing and gets no reply. The text is already cleaned by `channel_text`; `DONTALK_MODE`
/// does not block guild chat. Then `snoop_copy(Guild)` once per member the line reached (the speaker included) and
/// ONE chat-tap line (the admin console's chat monitor, 25 §4.9).
pub fn guild(svc: &mut Services, ctx: &mut GameCtx<'_>, speaker: PlayerRef, text: &str) {
    let Some(a) = actor(ctx, speaker) else {
        return;
    };
    let Some(gid) = svc.social.guild.guild_of(a.character) else {
        return;
    };
    let msg = pb::GuildChat {
        obj_id: speaker.0.0,
        name: a.name.clone(),
        text: text.to_owned(),
    };
    let speaker_obj = speaker.0;
    super::broadcast_filtered(
        svc,
        ctx,
        ObjId::NULL,
        Audience::Guild(gid),
        msg,
        |_, listener, worlds| delivers(svc, worlds, listener, speaker_obj, ChatChannel::Guild),
    );
    // the members the line reached: online with a session (the audience resolver skips a lingering avatar) and
    // past the delivery filter, in `CharacterId` order
    let reached: Vec<CharacterId> = svc
        .social
        .guild
        .online_objs(&svc.presence, gid)
        .into_iter()
        .filter(|o| ctx.session_of(PlayerRef(*o)).is_some())
        .filter(|o| delivers(svc, ctx.worlds, *o, speaker_obj, ChatChannel::Guild))
        .filter_map(|o| ctx.player(PlayerRef(o)))
        .filter_map(|m| m.player().map(|p| p.ident.character_id))
        .collect();
    for listener in reached {
        snoop_copy(svc, ctx, SnoopKind::Guild, speaker, listener, text);
    }
    chat_tap::record(svc, ctx, speaker, TapChannel::Guild, None, text);
}
