//! Target resolution and the standard replies (Plan/23 §3.7, PL-099).
//!
//! Names are resolved through the game's `PlayerDirectory` (every non-purged character), ASCII
//! case-insensitively (characters are unique `COLLATE NOCASE`); "online" means an in-world avatar (including the
//! linger window). Self-target checks compare character ids.

use super::{CmdCtx, CmdError};
use crate::ctx::{PlayerRef, Services};
use flyff_common::{CharacterId, ObjId, SessionId};

/// A resolved character name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// No such character.
    Unknown,
    /// A character that is not in the world.
    Offline {
        /// The id.
        id: CharacterId,
        /// The stored spelling of the name.
        name: String,
    },
    /// A character with an avatar.
    Online {
        /// The id.
        id: CharacterId,
        /// The stored spelling of the name.
        name: String,
        /// The avatar.
        player: PlayerRef,
        /// The bound session, `None` while it lingers.
        session: Option<SessionId>,
    },
}

/// Looks `name` up in the directory.
pub fn find(svc: &Services, name: &str) -> Target {
    let Some(e) = svc.presence.by_name(name) else {
        return Target::Unknown;
    };
    match e.online {
        Some(o) => Target::Online {
            id: e.id,
            name: e.name.clone(),
            player: PlayerRef(o.obj),
            session: Some(o.session),
        },
        None => Target::Offline {
            id: e.id,
            name: e.name.clone(),
        },
    }
}

/// The online target of `name` for a command that needs one: unknown -> `NotFound`, offline -> `Offline`, the
/// issuer itself -> `NotSelf` when `forbid_self`.
pub fn online(
    cx: &CmdCtx<'_, '_>,
    name: &str,
    forbid_self: bool,
) -> Result<(PlayerRef, String), CmdError> {
    match find(cx.svc, name) {
        Target::Unknown => Err(CmdError::NotFound(name.to_owned())),
        Target::Offline { name, .. } => Err(CmdError::Offline(name)),
        Target::Online { player, name, .. } => {
            if forbid_self && cx.self_player == Some(player) {
                return Err(CmdError::NotSelf);
            }
            Ok((player, name))
        }
    }
}

/// The object id of the avatar `p`.
pub fn obj(p: PlayerRef) -> ObjId {
    p.0
}
