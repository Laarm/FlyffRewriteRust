//! The delivery filter (Plan/21 §10.6, WP-SOCIAL-20; PL-095): which recipients a chat channel reaches, over the
//! privacy and ignore queries of [`crate::chat::privacy`] (WP-GM-22) and the friend blocks of the messenger.
//!
//! [`delivers`] is the `keep` closure of every filtered broadcast (`Outbox::broadcast_filtered`: the recipients are
//! chosen first and the line is encoded once for all of them) and the predicate of the whisper route.

use crate::chat::privacy;
use crate::ctx::Services;
use crate::social::messenger::block::friend_blocks;
use crate::world::Worlds;
use flyff_common::ObjId;

/// The channels of 21 §10.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatChannel {
    /// Normal chat.
    Say,
    /// Shout.
    Shout,
    /// Whisper.
    Whisper,
    /// Messenger instant message.
    Im,
    /// Party chat.
    Party,
    /// Guild chat.
    Guild,
}

/// Whether `recipient` receives a line of `channel` spoken by `speaker` (both online avatars):
///
/// * the speaker always receives his own line (even with `privacy.shout` off, KEEP);
/// * `Say`, `Party`, `Guild`: unless the recipient ignores the speaker (`chat.ignore`, ASCII case-insensitive);
/// * `Shout`: the recipient accepts shouts (`privacy.shout`) and does not ignore the speaker;
/// * `Whisper`, `Im`: the recipient does not ignore the speaker and has not blocked him as a friend. The friend
///   block is [`friend_blocks`] (the recipient's own block flag on the speaker); the messenger's `is_blocked` is
///   true for a stranger and would drop every whisper of a non-friend. `privacy.whisper` is NOT read here: the
///   whisper route answers it with `BlockNtf{1}` before it asks this filter.
///
/// A speaker that is not a live player (an NPC) is not filtered.
pub fn delivers(
    svc: &Services,
    worlds: &Worlds,
    recipient: ObjId,
    speaker: ObjId,
    channel: ChatChannel,
) -> bool {
    if recipient == speaker {
        return true;
    }
    let Some(from) = worlds.player(speaker).and_then(|m| m.player()) else {
        return true;
    };
    if privacy::ignores(worlds, recipient, &from.ident.name) {
        return false;
    }
    match channel {
        ChatChannel::Say | ChatChannel::Party | ChatChannel::Guild => true,
        ChatChannel::Shout => privacy::accepts_shout(worlds, recipient),
        ChatChannel::Whisper | ChatChannel::Im => {
            let Some(to) = worlds.player(recipient).and_then(|m| m.player()) else {
                return true;
            };
            !friend_blocks(svc, to.ident.character_id, from.ident.character_id)
        }
    }
}
