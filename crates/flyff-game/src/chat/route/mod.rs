//! Chat delivery (Plan/21 §10; WP-SOCIAL-02): the `ChatReq` handler shell, recipients and audiences, echoes and the
//! error replies of say, shout, whisper and the messenger instant message. The pre-delivery pipeline is
//! `chat::ingress` (Plan/23 §6, the seam of 21 §10.1); party and guild chat (WP-SOCIAL-03/14) register their handlers in
//! the same registry.
//!
//! ```text
//! ChatReq -> on_chat_req -> ingress::preprocess -> Command                (a command ran: rows 3/4/6 call the handlers
//!                                              |                           of `commands`, which call `whisper`/`shout`)
//!                                              -> Say -> ingress::say_pipeline -> route::say
//! ```

pub mod commands;
pub mod filter;
pub mod guild;
pub mod party;
pub mod say;
pub mod shout;
pub mod whisper;

pub use filter::{ChatChannel, delivers};
pub use guild::guild;
pub use party::party;
pub use say::{npc_say, say};
pub use shout::{shout, shout_color};
pub use whisper::{WhisperKind, whisper};

use super::ingress::{self, Ingress};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::{Audience, GameView};
use flyff_common::ObjId;
use flyff_proto::pb;

/// The `ChatReq` handler (21 §10.1): `preprocess`, then for a normal line `say_pipeline` and `route::say`.
pub fn on_chat_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, req: pb::ChatReq) {
    match ingress::preprocess(svc, ctx, who, &req.text, req.shortcut) {
        Ingress::Command => {}
        Ingress::Say {
            text,
            spam_ok,
            shortcut,
        } => {
            if let Some(clean) = ingress::say_pipeline(svc, ctx, who, text, spam_ok, shortcut) {
                say(svc, ctx, who, &clean);
            }
        }
    }
}

/// `Outbox::broadcast_filtered` over the game view (15 §8.3): resolves `aud`, keeps the recipients for which
/// `keep(session, player, worlds)` holds and encodes once. `keep` reads the worlds (positions, settings of the
/// recipients) and must not mutate game state.
pub(crate) fn broadcast_filtered(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    subject: ObjId,
    aud: Audience,
    body: impl Into<pb::server_msg::Msg>,
    mut keep: impl FnMut(flyff_common::SessionId, ObjId, &crate::world::Worlds) -> bool,
) {
    let GameCtx {
        worlds,
        sessions,
        outbox,
        cfg,
        ..
    } = &mut *ctx;
    let worlds_ro: &crate::world::Worlds = worlds;
    let view = GameView {
        worlds: worlds_ro,
        sessions,
        services: Some(svc),
        rect_xz: cfg.rules.fidelity.world_msg_rect_xz,
    };
    outbox.broadcast_filtered(&view, subject, aud, body, |sid, o| keep(sid, o, worlds_ro));
}
