//! Canonical identifier newtypes (Plan/01 §5.5, stage1-review §5).
//!
//! Every id is a transparent newtype with a public field so crates can construct and match them
//! cheaply; width and meaning follow the legacy server (`m_idPlayer` = `u32`, ...).

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub $inner);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl From<$inner> for $name {
            fn from(v: $inner) -> Self {
                Self(v)
            }
        }
    };
}

id_newtype! {
    /// Runtime object id. `1..`, never `0` and never `0xFFFF_FFFF`, never reused within a process
    /// run (ADR-002).
    ObjId(u32)
}
id_newtype! {
    /// One per WebSocket connection, monotonic (Plan/01 §5.5).
    SessionId(u64)
}
id_newtype! {
    /// Account id (legacy width).
    AccountId(u32)
}
id_newtype! {
    /// Character id (legacy `m_idPlayer`), allocated from the `meta` counter (13 §15).
    CharacterId(u32)
}
id_newtype! {
    /// World id (legacy `WI_*`).
    WorldId(u32)
}
id_newtype! {
    /// Layer id: `0` normal, `< 0` ticket copies, `> 0` instances.
    LayerId(i32)
}
id_newtype! {
    /// Channel id; always `1` in v1 (the data model keeps it for later, D1).
    ChannelId(u16)
}
id_newtype! {
    /// Unique item serial (allocated from `meta` in blocks, D5).
    ItemSerial(u64)
}
id_newtype! {
    /// Guild id.
    GuildId(u32)
}
id_newtype! {
    /// Party id.
    PartyId(u32)
}
id_newtype! {
    /// Mail id.
    MailId(u32)
}
id_newtype! {
    /// Guild war id.
    GuildWarId(u32)
}
id_newtype! {
    /// Global monotonic save generation, allocated by the game thread (13 §16.5).
    SaveGen(u64)
}

impl ObjId {
    /// The invalid / null object id.
    pub const NULL: ObjId = ObjId(0);
    /// The legacy "no object" sentinel (`NULL_ID`).
    pub const NONE: ObjId = ObjId(0xFFFF_FFFF);

    /// `true` for ids that can name a live object (`1..0xFFFF_FFFF`).
    pub const fn is_valid(self) -> bool {
        self.0 != 0 && self.0 != 0xFFFF_FFFF
    }
}

impl ChannelId {
    /// The only channel of v1.
    pub const ONE: ChannelId = ChannelId(1);
}

impl SaveGen {
    /// "Nothing saved yet".
    pub const ZERO: SaveGen = SaveGen(0);

    /// The next generation (saturating; a `u64` counter never wraps in practice).
    pub const fn next(self) -> SaveGen {
        SaveGen(self.0.saturating_add(1))
    }
}
