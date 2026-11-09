//! Say (local chat, Plan/21 §10.2; F-265).

use super::broadcast_filtered;
use super::filter::{ChatChannel, delivers};
use crate::admin::chat_tap::{self, ChatChannel as TapChannel};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use flyff_common::ObjId;
use flyff_common::defs::world::WI_WORLD_QUIZ;
use flyff_common::math::Vec3;
use flyff_proto::pb;

/// In the quiz world only listeners within this squared 3-D distance hear a line (`CUserMng::AddChat`).
const QUIZ_HEARING_D2: f32 = 10.0;

fn dist2(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.x * d.x + d.y * d.y + d.z * d.z
}

/// `route::say` (21 §10.2): `ChatNtf{text}` with `obj_id = speaker` to every player that has the speaker in its
/// view set, the speaker included (`Audience::NearInclSelf`), encoded once; in the quiz world only listeners
/// within squared distance 10. `text` is already cleaned by `say_pipeline`. The snoop copy of 23 §6.12 (M8) is
/// WP-GM-31's `snoop_copy` hook.
pub fn say(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, text: &str) {
    let Some(core) = ctx.worlds.core(who.0) else {
        return;
    };
    let (world, pos) = (core.world, core.pos);
    let quiz = world.0 == WI_WORLD_QUIZ;
    let speaker = who.0;
    broadcast_filtered(
        svc,
        ctx,
        speaker,
        Audience::NearInclSelf,
        pb::ChatNtf {
            text: text.to_owned(),
        },
        |_, listener, worlds| {
            (!quiz
                || worlds
                    .core(listener)
                    .is_some_and(|c| dist2(c.pos, pos) <= QUIZ_HEARING_D2))
                && delivers(svc, worlds, listener, speaker, ChatChannel::Say)
        },
    );
    // the admin console's chat tap (25 §4.9): the delivered text, after the ingress pipeline
    chat_tap::record(svc, ctx, who, TapChannel::Say, None, text);
}

/// `route::npc_say` (21 §10.2): NPC and monster speech (and `/id`) without any filter: `ChatNtf` from `npc` to
/// its observers.
pub fn npc_say(svc: &Services, ctx: &mut GameCtx<'_>, npc: ObjId, text: &str) {
    ctx.broadcast(
        Some(svc),
        npc,
        Audience::NearInclSelf,
        pb::ChatNtf {
            text: text.to_owned(),
        },
    );
}
