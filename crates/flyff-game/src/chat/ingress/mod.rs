//! The chat ingress of Plan/23 §6: everything before delivery (WP-GM-05, WP-GM-06).
//!
//! ```text
//! ChatReq{text, shortcut}
//!  └─ route::on_chat_req (doc 21)
//!      └─ preprocess(..) -> Ingress                       // §6.1: normalise, gm.chat audit, command dispatch
//!           ├─ Command                                    // a command ran / the line was ignored / a reply was queued
//!           └─ Say{text, spam_ok, shortcut} -> say_pipeline(..) -> Some(clean) -> route::say
//! ```
//!
//! The bodies of the seam functions of 21 §10.1 are here; no function of this module calls `chat::route::*`.
//! They take `(&mut Services, &mut GameCtx)` and the speaker as a [`PlayerRef`](crate::ctx::PlayerRef) instead of
//! the `&mut Game` and `ObjId` of the plan's sketch.

pub mod audit;
pub mod emote;
pub mod filter;
pub mod markup;
pub mod mode;
pub mod normalize;
pub mod notice;
mod pipelines;
pub mod snoop;
pub mod spam;

pub use audit::{AuditTap, PrivateKind, audit_private};
pub use markup::{has_item_buff, markup_text};
pub use mode::{is_mode, is_mode_of, is_shout_full, mute_text};
pub use pipelines::{Ingress, LenRule, channel_text, preprocess, say_pipeline, shout_pipeline};
pub use snoop::{SnoopCall, SnoopKind, SnoopState, snoop_copy};

/// `Services.chat`: the chat tables built at start-up (23 §13 Q6).
#[derive(Debug, Default)]
pub struct ChatService {
    /// The profanity filter and the emotes.
    pub data: crate::chat::ChatData,
    /// The admin console's chat tap: the lines of this tick, recorded after delivery (Plan/25 §4.9, WP-ADMIN-19).
    pub tap: crate::admin::chat_tap::ChatTap,
    /// The snooped guilds and the call recorder of the snoop hooks (23 §6.12, WP-GM-21).
    pub snoop: SnoopState,
}

impl ChatService {
    /// The service of `GameInit.chat_data`.
    pub fn new(data: crate::chat::ChatData) -> Self {
        Self {
            data,
            tap: crate::admin::chat_tap::ChatTap::default(),
            snoop: SnoopState::default(),
        }
    }
}
