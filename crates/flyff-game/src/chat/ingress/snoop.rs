//! Snoop copies (Plan/23 §6.12, F-279, PL-146; M8 lower priority, WP-GM-31): the hook the delivery routes call
//! after each delivery, and the runtime state behind it.
//!
//! * [`SnoopState`] (`Services.chat.snoop`) holds the guilds a GM switched snooping on for (`/sg`, WP-GM-31); it is
//!   runtime only and never persisted. WP-GM-31 adds the snoop table of `/snoop` next to it.
//! * [`snoop_copy`] is called by `route::{whisper, say, party, guild}` once per listener the line reached. The
//!   `Snoop{text}` copies of the snoopers are WP-GM-31's body; the guild lookup is real (WP-GM-21): a line of a
//!   flagged guild writes ONE `gm.snoop {guild, from, text}` audit record, on the call whose listener is the
//!   speaker (the speaker is always among the members the line is delivered to).
//! * The call recorder ([`SnoopState::start_recording`]) is what the tests of the routes and of WP-GM-31 assert:
//!   every call as `(kind, speaker, listener)`.

use crate::chat::actor;
use crate::chat::ingress::audit;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::{CharacterId, GuildId};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// What a snooper receives a copy of (`[w]`, `[c]`, `[p]`, `[g]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnoopKind {
    /// `[w][from->to] text`.
    Whisper,
    /// `[c][speaker->listener] text`: normal chat the target hears.
    Say,
    /// `[p][speaker->member] text`.
    Party,
    /// `[g][speaker-member] text`.
    Guild,
}

/// One recorded call of [`snoop_copy`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnoopCall {
    /// The channel.
    pub kind: SnoopKind,
    /// The speaker (the sender of a whisper).
    pub speaker: CharacterId,
    /// The listener the line reached (the target of a whisper).
    pub listener: CharacterId,
}

/// The runtime state of the snoop hooks (`Services.chat.snoop`, 23 §6.12): the snooped guilds and the call
/// recorder.
#[derive(Clone, Debug, Default)]
pub struct SnoopState {
    /// Guilds whose chat lines are audited (`/sg <guild>`).
    guilds: BTreeSet<GuildId>,
    /// The recorded calls while recording is on.
    recorded: Option<Vec<SnoopCall>>,
}

impl SnoopState {
    /// Switches snooping of `guild` on or off (`/sg <guild> [1]`, WP-GM-31).
    pub fn set_guild(&mut self, guild: GuildId, on: bool) {
        if on {
            self.guilds.insert(guild);
        } else {
            self.guilds.remove(&guild);
        }
    }

    /// Whether the chat of `guild` is snooped.
    pub fn guild_snooped(&self, guild: GuildId) -> bool {
        self.guilds.contains(&guild)
    }

    /// The snooped guilds in id order.
    pub fn guilds(&self) -> impl Iterator<Item = GuildId> + '_ {
        self.guilds.iter().copied()
    }

    /// Starts recording the calls of [`snoop_copy`] (tests; the list starts empty).
    pub fn start_recording(&mut self) {
        self.recorded = Some(Vec::new());
    }

    /// The calls recorded so far, drained (empty when recording is off).
    pub fn take_calls(&mut self) -> Vec<SnoopCall> {
        self.recorded
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// The calls recorded so far (empty when recording is off).
    pub fn calls(&self) -> &[SnoopCall] {
        self.recorded.as_deref().unwrap_or_default()
    }
}

/// `snoop_copy` (23 §6.12): `speaker`'s line `text` of channel `kind` reached `listener`.
///
/// * Records the call when the recorder is on.
/// * `Guild`: when the speaker's guild is flagged ([`SnoopState::guild_snooped`]) and `listener` is the speaker
///   himself, audits `gm.snoop {guild, from, text}` (legacy `LOG_GAMEMASTER_STR`): exactly one record per line,
///   because the route makes one call per delivered member and the speaker is always one.
/// * The `Snoop{text}` copies to `/snoop` GMs are WP-GM-31.
pub fn snoop_copy(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    kind: SnoopKind,
    speaker: PlayerRef,
    listener: CharacterId,
    text: &str,
) {
    let Some(a) = actor(ctx, speaker) else {
        return;
    };
    if let Some(calls) = svc.chat.snoop.recorded.as_mut() {
        calls.push(SnoopCall {
            kind,
            speaker: a.character,
            listener,
        });
    }
    if kind == SnoopKind::Guild && listener == a.character {
        let guild = ctx
            .player(speaker)
            .and_then(|m| m.player())
            .and_then(|p| p.social.guild);
        if let Some(g) = guild
            && svc.chat.snoop.guild_snooped(g)
        {
            let mut rec = audit::head(ctx, &a);
            rec.insert(
                "data".to_owned(),
                json!({ "guild": g.0, "from": a.name, "text": text }),
            );
            audit::emit(svc, ctx, "gm.snoop", Value::Object(rec));
        }
    }
}
