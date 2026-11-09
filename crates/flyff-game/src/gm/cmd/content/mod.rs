//! The content-control GM commands (Plan/23 §3.1 group `content`, §4; WP-GM-26): start, stop and advance every timed
//! M7 feature of doc 21 and doc 22 from a chat line or the console.
//!
//! | file | rows |
//! |---|---|
//! | `gc1to1.rs` | 96 `/gc1to1open [1]`, 97 `/gc1to1close`, 98 `/gc1to1next` |
//! | `secret_room.rs` | 166 `/sropen`, 167 `/srnext`, 171 `/srclose` |
//! | `lord.rs` | 178 `/ep`, 179 `/ebc`, 180 `/ebv`, 181 `/eev`, 184 `/leinitialize` |
//! | `tax.rs` | 188 `/tan` |
//! | `layer.rs` | 193 `/cl`, 194 `/dl`, 195 `/lay` |
//! | `rainbow.rs` | 197 `/rropen`, 198 `/rrnext`, 199 `/rrpass` |
//! | `housing.rs` | 209 `/hgmra` |
//! | `quiz.rs` | 211 `/qeo`, 213 `/qsn`, 214 `/qec` |
//! | `control.rs` | N17 `/content` (alias `ctl`): one state line per feature and the verbs of the rows above |
//!
//! Every handler is a thin wrapper over the `gm` module of the owning feature (`event::{lord, tax, secret_room,
//! rainbow, quiz, guild_house, housing}::gm`, `social::gc1to1::gm`): those functions are pure calls that never print and
//! never check authority, and every "next" is the deadline of the owner's machine moved to now, so the owner's own slot
//! item or periodic job runs the real transition at its next run (no transition exists twice). This layer parses, gates
//! on the event flag, prints and audits (the dispatcher writes the `gm.command` record).
//!
//! The row handlers and the `/content` verbs share the action functions of the per-feature files, so a verb moves
//! exactly the step of its legacy row (`Err(text)` is the refusal text of the row, printed as `Text` by the row and as
//! `"<key>: <text>"` by the verb).
//!
//! `events.guild_combat_1to1`, `events.secret_room` and `events.rainbow_race` are off by default (PD-14): while a flag is
//! off every row of its feature prints its "is off" text and changes nothing (B8's `GUILD_COMBAT_OFF` ruling; the legacy
//! open and process functions returned silently).

use crate::gm::CmdCtx;
use flyff_common::config::EventId;

pub mod control;
pub mod gc1to1;
pub mod housing;
pub mod layer;
pub mod lord;
pub mod quiz;
pub mod rainbow;
pub mod secret_room;
pub mod tax;

/// What the 1:1 guild combat rows print while `events.guild_combat_1to1` is off.
pub const GC1TO1_OFF: &str = "guild combat 1:1 is off (events.guild_combat_1to1)";
/// What the secret-room rows print while `events.secret_room` is off.
pub const SECRET_ROOM_OFF: &str = "secret room is off (events.secret_room)";
/// What the rainbow-race rows print while `events.rainbow_race` is off.
pub const RAINBOW_OFF: &str = "rainbow race is off (events.rainbow_race)";

/// `/gc1to1open` outside the closed state (`TextCmd_GuildCombat1to1Open`).
pub const ALREADY_OPEN: &str = "already OPEN State!!!";
/// `/gc1to1close` and `/gc1to1next` in the closed state (the legacy printed nothing; new text).
pub const IS_CLOSE_STATE: &str = "Is CLOSE State!!!";
/// `/sropen` outside the closed state (`TextCmd_SecretRoomOpen`).
pub const NOT_CLOSE_STATE: &str = "Is Not Close State!!!";
/// `/srclose` outside the war state (`TextCmd_SecretRoomClose`).
pub const NOT_WAR_STATE: &str = "Is Not War State!!!";
/// `/lay` into a world that is not loaded (`TextCmd_Layer`).
pub const UNDEFINED_WORLD: &str = "UNDEFINED WORLD";
/// `/lay` into a layer that does not exist (`TextCmd_Layer`).
pub const LAYER_NO_EXISTS: &str = "LAYER NO EXISTS";
/// `/lay` to a position outside the world (`TextCmd_Layer`).
pub const OUT_OF_WORLD: &str = "OUT OF WORLD";
/// `/dl` of a layer that a service owns or that has a player in it (new text: Plan/23 row 194 "refused while players are
/// in it").
pub const LAYER_IN_USE: &str = "LAYER IN USE";

/// `true` when the event flag `id` is on (`LiveConfig::event_state`, runtime-changeable with `/le`).
pub(crate) fn flag_on(cx: &CmdCtx<'_, '_>, id: EventId) -> bool {
    cx.ctx.live.event_state(id) != 0
}

/// The gate of a feature row: `true` when the flag is on; otherwise prints `off_text` and returns `false`.
pub(crate) fn gate(cx: &mut CmdCtx<'_, '_>, id: EventId, off_text: &str) -> bool {
    if flag_on(cx, id) {
        return true;
    }
    cx.out.text(off_text);
    false
}
