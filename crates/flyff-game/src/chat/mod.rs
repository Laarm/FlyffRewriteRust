//! Chat (Plan/23 §6, Plan/21 §10; WP-GM-05, WP-GM-06, WP-SOCIAL-02).
//!
//! * [`ingress`]: everything BEFORE delivery (doc 23): the normalisation of one typed line, the command dispatch,
//!   the say pipeline, the shout pipeline with the anti-spam state machine, the word filter, markup stripping,
//!   mode bits and mute, notices, the `gm.chat` audit;
//! * [`route`]: DELIVERY (doc 21): the `ChatReq` handler shell, `say`, `shout`, `whisper` and the command handlers
//!   of rows 3, 4 and 6.
//! * [`privacy`]: the privacy toggles and the ignore list stored on the server (doc 23 §6.11, WP-GM-22), their queries,
//!   the commands of rows 13-26 and `ChatSettingsNtf`; the delivery filters of [`route`] ask it.
//!
//! The functions take the split-borrow pair `(&mut Services, &mut GameCtx)` of Plan/10 §4.1.1 (the plan sketches
//! them with `&mut Game`) and the speaker as a [`PlayerRef`].

pub mod ingress;
pub mod privacy;
pub mod route;
pub mod vendor_room;

use crate::ctx::{GameCtx, PlayerRef};
use crate::gm::CmdOutput;
use flyff_common::gm::Authority;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, SessionId, WorldId};
use flyff_data::ClientData;
use flyff_proto::pb;
pub use ingress::emote::EmoteTable;
pub use ingress::filter::ChatFilter;
pub use ingress::spam::{ChatSpamState, SpamVerdict};

/// Server-side chat tables built from the client data (Plan/23 §13 Q6): the profanity filter of §6.9 and the
/// emote names of §6.10. `GameInit.chat_data`.
#[derive(Clone, Debug, Default)]
pub struct ChatData {
    /// `filter_ENG.inc`.
    pub filter: ChatFilter,
    /// `textEmotion.txt`.
    pub emotes: EmoteTable,
}

impl ChatData {
    /// The tables of the loaded client data (`ClientData.chat_filter`, `ClientData.text_emotions`).
    pub fn from_client(client: &ClientData) -> Self {
        Self {
            filter: ChatFilter::from_pairs(&client.chat_filter),
            emotes: EmoteTable::from_map(&client.text_emotions),
        }
    }
}

/// The chat state of one online player (Plan/23 §2.6, §6.3; `PlayerExt.chat`). The mute countdown is
/// `PlayerExt.misc.mute_remaining_s` (the one persisted location, 16 §3.3), not repeated here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerChat {
    /// The anti-spam windows and counters of the speaker.
    pub spam: ChatSpamState,
}

/// What the chat functions read of the speaker (a snapshot taken at the start of a call).
#[derive(Clone, Debug)]
pub struct Actor {
    /// The avatar.
    pub who: PlayerRef,
    /// Its bound session (`None` while it lingers).
    pub session: Option<SessionId>,
    /// `characters.id`.
    pub character: CharacterId,
    /// The name.
    pub name: String,
    /// The effective authority resolved at join.
    pub authority: Authority,
    /// The level.
    pub level: i32,
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// Position.
    pub pos: Vec3,
    /// `m_dwMode`.
    pub mode: u32,
}

/// Snapshot of the speaker; `None` when `who` is not a live player.
pub fn actor(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Actor> {
    let m = ctx.player(who)?;
    let p = m.player()?;
    Some(Actor {
        who,
        session: ctx.session_of(who),
        character: p.ident.character_id,
        name: p.ident.name.clone(),
        authority: p.ident.authority,
        level: m.mv.level,
        world: m.core.world,
        layer: m.core.layer,
        pos: m.core.pos,
        mode: m.mv.mode,
    })
}

/// Queues `msg` for the session of `who` (nothing when the avatar has no socket).
pub fn send_to(ctx: &mut GameCtx<'_>, who: PlayerRef, msg: impl Into<pb::server_msg::Msg>) {
    if let Some(sid) = ctx.session_of(who) {
        ctx.outbox.to_session(sid, 0, msg);
    }
}

/// Delivers the collected command output to a player (the `OutLine` mapping of Plan/23 §3.2).
pub fn deliver_lines(ctx: &mut GameCtx<'_>, who: PlayerRef, out: CmdOutput) {
    let Some(sid) = ctx.session_of(who) else {
        return;
    };
    for line in out.lines {
        ctx.outbox.to_session(sid, 0, line.into_message());
    }
}
