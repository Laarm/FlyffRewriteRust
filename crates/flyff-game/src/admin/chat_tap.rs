//! The chat tap (Plan/25 §4.9, §11.5 `ChatSub`; types and hooks: WP-ADMIN-06, the tap itself: **WP-ADMIN-19**).
//!
//! # How a line gets from a player to the console
//!
//! The chat routes of 21 (`chat::route::{say, shout, party, whisper}`) call [`record`] **after** their delivery,
//! with the text that was delivered: that is the text *after* the ingress pipeline of 23 §6.1-§6.9 (normalised,
//! word-filtered, marked up), so a filtered word appears in the console as the players saw it. [`record`] is one
//! branch while nobody watches (`ChatTap::enabled`, switched by the registry whenever a chat subscription opens or
//! closes); otherwise it appends a [`ChatLine`] to the per-tick buffer in `Services.chat.tap`. The buffer is bounded
//! ([`MAX_LINES_PER_TICK`]; the overflow is counted, never allocated).
//!
//! The `post` phase of the tick ([`Game::run_admin_views`](crate::tick::Game)) drains the buffer once
//! (`deliver`): every line goes to the subscriptions whose [`ChatFilter`] matches it
//! (`AdminViews::chat_subs`), as an [`AdminEvent::Chat`] into the connection's bounded queue. A slow console loses
//! lines (and gets `Resync`), never the game thread's time.
//!
//! `original_text` (the text before the profanity filter changed it) is noted by the pipelines that run the filter
//! ([`note_original`]: `say_pipeline`, `shout_pipeline`, and the party command, which filters in `channel_text`)
//! and attached to the next line of that speaker. The gateway forwards it to `view.chat.private` sessions only.
//!
//! **Which channels an identity may see is decided by the gateway** (`view.chat`: say and shout; `view.chat.private`
//! adds party, guild, whisper and messenger, audited as `gm.snoop`), never here. The tap is an observer: it reads
//! the game and writes nothing the simulation can see (ADM-D4), so replay does not change with consoles open.
//!
//! | channel | recorded by |
//! |---|---|
//! | `Say`, `Shout` | `chat::route::say`, `chat::route::shout` |
//! | `Party` | `chat::route::party` |
//! | `Whisper`, `Messenger` | `chat::route::whisper` (`/w`, and the `/say` messenger message) |
//! | `Guild` | `chat::route::guild` (WP-SOCIAL-14) |
//! | `System` | not recorded in v1: the notices are the console's own (Communication, WP-ADMIN-27) |

use super::events::{AdminEvent, SubError, SubErrorCode};
use super::views::AdminViews;
use crate::chat::actor;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::{CharacterId, ObjId, WorldId};

/// The longest text filter a subscription may carry (bytes).
pub const MAX_FILTER_TEXT: usize = 64;
/// Lines the tap keeps per tick; the rest of a burst is counted in [`ChatTap::dropped`].
pub const MAX_LINES_PER_TICK: usize = 1024;

/// A chat channel (`ChatChannel` of the protocol).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ChatChannel {
    /// Normal chat.
    Say = 1,
    /// Shout.
    Shout = 2,
    /// Party chat.
    Party = 3,
    /// Guild chat.
    Guild = 4,
    /// A whisper.
    Whisper = 5,
    /// Messenger chat.
    Messenger = 6,
    /// System notices.
    System = 7,
}

impl ChatChannel {
    /// The bit of this channel in [`ChatFilter::channels`].
    pub const fn bit(self) -> u8 {
        1 << (self as u8)
    }

    /// `true` for the channels only `view.chat.private` may see (party, guild, whisper, messenger; 25 §4.9).
    pub const fn is_private(self) -> bool {
        matches!(
            self,
            ChatChannel::Party | ChatChannel::Guild | ChatChannel::Whisper | ChatChannel::Messenger
        )
    }
}

/// Every channel bit a filter may carry (`Say` .. `System`).
const ALL_BITS: u8 = 0b1111_1110;

/// What a chat subscription selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatFilter {
    /// A bitset of [`ChatChannel::bit`].
    pub channels: u8,
    /// Only this world; `None` = all.
    pub world: Option<WorldId>,
    /// Only this speaker (a whisper also matches its addressee); `None` = all.
    pub character: Option<CharacterId>,
    /// Only lines containing this text (case-insensitive); empty = all.
    pub text: String,
}

impl ChatFilter {
    /// Does `line` pass the filter? `to_character_id` is the addressee of a whisper or messenger message (`0`
    /// otherwise): a filter by character matches the speaker and the addressee.
    pub fn matches(&self, line: &ChatLine, to_character_id: u32) -> bool {
        if self.channels & line.channel.bit() == 0 {
            return false;
        }
        if self.world.is_some_and(|w| w.0 != line.world) {
            return false;
        }
        if let Some(c) = self.character
            && line.from_character_id != c.0
            && to_character_id != c.0
        {
            return false;
        }
        self.text.is_empty() || contains_ignore_case(&line.text, &self.text)
    }
}

/// `haystack` contains `needle`, ignoring case (Unicode lower-casing; `needle` is short, so this is cheap).
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    if haystack.is_ascii() && needle.is_ascii() {
        let h = haystack.as_bytes();
        let n = needle.as_bytes();
        return n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// One chat line as the tap hands it over (`ChatLine`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLine {
    /// Unix milliseconds.
    pub ts_ms: u64,
    /// The channel.
    pub channel: ChatChannel,
    /// The speaker's character id.
    pub from_character_id: u32,
    /// The speaker.
    pub from: String,
    /// The addressee (whispers), else empty.
    pub to: String,
    /// The text as delivered (after the profanity filter).
    pub text: String,
    /// The text before the filter changed it; empty when it did not (only sent to `P` sessions).
    pub original_text: String,
    /// The world the line was said in.
    pub world: u32,
}

/// The state of one chat subscription.
#[derive(Clone, Debug)]
pub struct ChatSub {
    /// What it selects.
    pub filter: ChatFilter,
}

fn check(filter: &ChatFilter) -> Result<(), SubError> {
    if filter.channels & ALL_BITS == 0 {
        return Err(SubError::new(
            SubErrorCode::BadSpec,
            "the chat subscription names no channel",
        ));
    }
    if filter.text.len() > MAX_FILTER_TEXT {
        return Err(SubError::new(
            SubErrorCode::BadSpec,
            format!("the text filter is longer than {MAX_FILTER_TEXT} bytes"),
        ));
    }
    Ok(())
}

/// Creates a chat subscription (hook of `AdminViewOp::Open`).
pub fn open(filter: ChatFilter) -> Result<ChatSub, SubError> {
    check(&filter)?;
    Ok(ChatSub { filter })
}

/// Changes the filter of a chat subscription (hook of `AdminViewOp::Update`). Lines already delivered stay with
/// the client; the new filter applies from the next tick.
pub fn update(sub: &mut ChatSub, filter: ChatFilter) -> Result<(), SubError> {
    check(&filter)?;
    sub.filter = filter;
    Ok(())
}

// ---- the recorder -------------------------------------------------------------------------------------------------

/// A recorded line with what only the filter needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tapped {
    /// What the console receives.
    pub line: ChatLine,
    /// The addressee of a whisper or messenger message, else `0`.
    pub to_character_id: u32,
}

/// The per-tick buffer of tapped lines (`Services.chat.tap`).
#[derive(Debug, Default)]
pub struct ChatTap {
    /// A chat subscription is open: the routes record.
    enabled: bool,
    /// The lines of this tick, oldest first.
    lines: Vec<Tapped>,
    /// The pre-filter text noted by a pipeline for the next line of this speaker.
    original: Option<(ObjId, String)>,
    /// Lines recorded since the start.
    recorded: u64,
    /// Lines lost to [`MAX_LINES_PER_TICK`].
    dropped: u64,
}

impl ChatTap {
    /// Is anybody watching? (The routes skip all work when not.)
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Switched by the registry whenever a chat subscription opens or closes. Turning the tap off forgets the
    /// buffer.
    pub(crate) fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.lines.clear();
            self.original = None;
        }
    }

    /// Lines recorded since the start.
    pub fn recorded(&self) -> u64 {
        self.recorded
    }

    /// Lines lost to the per-tick cap.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Takes the lines of this tick.
    pub(crate) fn take(&mut self) -> Vec<Tapped> {
        std::mem::take(&mut self.lines)
    }

    fn push(&mut self, line: Tapped) {
        if self.lines.len() >= MAX_LINES_PER_TICK {
            self.dropped += 1;
            return;
        }
        self.recorded += 1;
        self.lines.push(line);
    }
}

/// Notes that the profanity filter changed what `speaker` typed (`before` -> `after`): the next line of that
/// speaker carries `before` as its `original_text`. A no-op while nobody watches or when the filter did nothing.
pub fn note_original(svc: &mut Services, speaker: ObjId, before: &str, after: &str) {
    let tap = &mut svc.chat.tap;
    // the shout body is the untrimmed rest of the command line: compare and show what a person would read
    let (before, after) = (before.trim(), after.trim());
    if tap.enabled && before != after {
        tap.original = Some((speaker, before.to_owned()));
    }
}

/// Records one delivered line (see the module documentation). `to` is the addressee of a whisper or a messenger
/// message. One branch while nobody watches.
pub fn record(
    svc: &mut Services,
    ctx: &GameCtx<'_>,
    speaker: PlayerRef,
    channel: ChatChannel,
    to: Option<(CharacterId, &str)>,
    text: &str,
) {
    if !svc.chat.tap.enabled {
        return;
    }
    let Some(a) = actor(ctx, speaker) else {
        return;
    };
    let tap = &mut svc.chat.tap;
    let original_text = match tap.original.take() {
        Some((who, before)) if who == speaker.0 => before,
        _ => String::new(),
    };
    tap.push(Tapped {
        line: ChatLine {
            ts_ms: u64::try_from(ctx.clock.unix_ms.0).unwrap_or(0),
            channel,
            from_character_id: a.character.0,
            from: a.name,
            to: to.map_or_else(String::new, |(_, name)| name.to_owned()),
            text: text.to_owned(),
            original_text,
            world: a.world.0,
        },
        to_character_id: to.map_or(0, |(id, _)| id.0),
    });
}

/// Hands the lines of this tick to the matching subscriptions (the `post` phase). A line goes through the
/// connection's bounded queue; a full queue drops it (the client was told with `Resync`).
pub(crate) fn deliver(views: &AdminViews, lines: Vec<Tapped>) {
    if lines.is_empty() {
        return;
    }
    let subs: Vec<_> = views.chat_subs().collect();
    if subs.is_empty() {
        return;
    }
    for t in lines {
        for (_, sub, filter, sink) in &subs {
            if filter.matches(&t.line, t.to_character_id) {
                let _ = sink.push(*sub, AdminEvent::Chat(Box::new(t.line.clone())));
            }
        }
    }
}
