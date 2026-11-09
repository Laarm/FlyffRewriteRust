//! Presence (Plan/21 §3, WP-SOCIAL-01): who exists and who is online. [`PlayerDirectory`] is the one place that
//! resolves a character name to an id (whisper, friends, invitations, GM commands) and answers the name and
//! slot questions of character creation (12 §9) and deletion (12 §10). The login/logout fan-out of 21 §3.4
//! (`UpdatePlayerData`, guild/messenger notices) arrives with WP-SOCIAL-08 behind the hooks of
//! [`crate::social::hooks`]; this module owns the state they read and write.

pub mod directory;

pub use directory::{
    DirEntry, DirectoryError, NameKey, OnlineRef, PENDING_ID, PendingCreate, PlayerDirectory, SLOTS,
};
