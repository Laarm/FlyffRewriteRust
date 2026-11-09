//! The vendor chat rooms of the plan's module path (`crates/flyff-game/src/chat/vendor_room.rs`, Plan/19 §1.3,
//! WP-ECON-11): the implementation lives in [`crate::econ::room`] (the task's module layout); this module
//! re-exports it under the plan's names - `ChatRooms`, `VendorRoom`, the four handlers `on_enter`,
//! `on_chatting`, `on_open_room`, `on_close_room` and every `ChattingNtf` builder.

pub use crate::econ::room::*;
