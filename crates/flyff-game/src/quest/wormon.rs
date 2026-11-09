//! `QuestTextTime` and the deferred Wormon processors (Plan/20 §9.11, WP-NPC-16; ref/16 §8; F-253 DEFER, D20-15).
//!
//! # What exists
//!
//! [`send_text_time`] sends `QuestTextTime{flag, state, time_ms}` (`SNAPSHOTTYPE_QUEST_TEXT_TIME 0x00BA`, the
//! countdown line of the quest window) to one player. Nothing calls it in normal play: the original's only senders
//! belong to the PARTY-quest processor that this port defers (see below), and the guild Wormon processor sends none.
//! The time travels as a duration in milliseconds. [`TEXT_TIME_CLEAR`] is the value of the clearing message
//! (`AddQuestTextTime(0, 0, 0xffffffff)`).
//!
//! # What is deferred, and the design for the day it is built
//!
//! `MonHuntStart` (guild) and `MonHuntStartParty` (party) stay the logged stubs of the NPC helper API: they log one
//! INFO line per helper and per run (target `lua.npc`, `NpcService.stub_logged`) and return 0, the original's
//! "slot busy" answer, so a script takes its "try again later" branch. `QuestDropHook::on_wormon_killed` stays the
//! default no-op. No processor exists in this crate (the quest data of the shipped game is empty, PD-01, so no
//! script can reach one; the two real Darkon guild scripts reach `MonHuntStart` only with a guild-quest property).
//! The design, from `_Common/guildquest.cpp` / `partyquest.cpp` and ref/16 §8:
//!
//! * **Slots.** 256 slots (`MAX_GUILD_QUEST`, `MAX_PARTY_QUEST`), one running quest per slot, any guild; a started
//!   quest occupies its slot until it ends.
//! * **Start** (`MonHuntStart(q, s, s2, n)`): the slot must be free, the player must be in a guild and the guild
//!   quest property `q` must exist; the Wormon boss (`prop.wormon`) is created at `prop.position`, the guild quest
//!   gets state `s` ([`super::guild::guild_quest_set`]), the end time is now + 60 minutes (phase WORMON), and the whole
//!   guild is teleported into the arena at `(x, y, (2z + y2) / 3)`. `MonHuntStartParty` spawns every boss of the
//!   party property (aggressive) in its world, teleports the party there and requires the party to pass the level
//!   check (`prop.level + 5`, else `TID_GAME_PARTYQUEST_NOT_JOIN`).
//! * **Phase WORMON** (60 minutes): every 10 ticks the processor checks whether any member is alive inside the arena
//!   rectangle `(x1, y2)-(x2, y1)` (`PropGuildQuest.inc` `Region`); none means failure: the guild gets the failure
//!   state `n`, the guild is teleported out (`ReplaceLodestar`), the boss is deleted; the party variant deletes every
//!   non-peaceful mover within 500 m of the rectangle centre, sends everyone to Madrigal (6968, 0, 3328.8) and the
//!   clearing message `(false, 0, TEXT_TIME_CLEAR)`.
//! * **Boss killed** (`Mover.cpp` 7389-7427, `QuestDropHook::on_wormon_killed`): the attacker's guild gets the success
//!   state `s2`, the phase becomes GETITEM for 20 minutes. The party variant sends `(true, PQP_GETITEM, 20 min)` to the
//!   members, but only when `objidWormon == boss id` and party quests store 0 (the original's bug: it never
//!   triggers; a port must decide whether to fix it).
//! * **Timeout** (60 minutes in WORMON, 20 minutes in GETITEM): the same cleanup as a failure.
//! * **`QuestTextTime` senders** (all of the party processor, none of the guild one): `(true, PQP_WORMON, 60 min)` to
//!   every member at the start (`partyquest.cpp` 298-307, 337-350), `(true, PQP_GETITEM, 20 min)` after the Wormon
//!   kill (`Mover.cpp` 7424), `(false, 0, TEXT_TIME_CLEAR)` at the failure or the end (`partyquest.cpp` 130-150, 211,
//!   271), and `AdjustPartyQuest` (`WORLDSERVER/User.cpp` 3762-3780) sends the REMAINING time when a member enters
//!   the quest rectangle.
//!
//! `IsWormonServer`, `events.wormon` and `/ctd` belong to the event work packages (WP-EVENT-21), not to this module.

use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef};
use flyff_proto::pb;

/// The `dwTime` of the clearing message, `AddQuestTextTime(0, 0, 0xffffffff)` (`partyquest.cpp`).
pub const TEXT_TIME_CLEAR: u32 = 0xffff_ffff;

/// `AddQuestTextTime(flag, state, time)`: sends `QuestTextTime{flag, state, time_ms}` to the player `pc` (nothing when
/// the avatar has no session). `state` is the processor phase (`PQP_WORMON`, `PQP_GETITEM`) the text belongs to,
/// `time_ms` the remaining duration or [`TEXT_TIME_CLEAR`].
pub fn send_text_time(ctx: &mut GameCtx<'_>, pc: PlayerRef, flag: bool, state: i32, time_ms: u32) {
    send_to(
        ctx,
        pc,
        pb::QuestTextTime {
            flag,
            state,
            time_ms,
        },
    );
}
