//! Offline notes (Plan/21 §9.6, §3.4 join step 7; WP-SOCIAL-20; F-224, VR-005): `SendTag{to, text}` leaves a short
//! message for a friend who is offline, the `Tag` message of the next join batch shows up to 20 of them, and
//! `MarkNotesDelivered` closes them after the join.
//!
//! * **State.** [`NoteBox`] (`Social.notes`) keeps the undelivered count per receiver in memory (the cap of 20
//!   lived in the SQL procedure of the original; the writer is asynchronous here, so the memory is the gate),
//!   seeded from `StartupState.note_counts`, +1 per insert, minus n at delivery; and, for every character in the
//!   world, the batch the join showed (the join state lives in the service, not on the avatar, like the
//!   messenger lists).
//! * **Send** ([`on_send_tag`]): a muted or silenced sender gets nothing (the mute text), a non-friend or an online
//!   friend is ignored silently (the original), my own block flag or a full mailbox answers
//!   `InsertTagResult{ok: false}`, the text is cut to 255 bytes on a character boundary (no word filter, no
//!   markup), an empty text is ignored; else `DbOp::InsertNote`, the count +1 and `InsertTagResult{ok: true}`.
//! * **Delivery** ([`on_join`], [`TagStep`], [`after_join`]): `on_join` keeps the oldest 20 undelivered notes of the
//!   `CharacterLoad`; [`TagStep`] writes `Tag` into the join batch AND into the re-attach replay (the client keeps
//!   its notes in memory only); the first `on_player_joined` after the batch ([`after_join`], never the re-attach)
//!   emits `MarkNotesDelivered{up_to_id = highest id shown}` and lowers the count, so a crash before it delivers
//!   the same notes again (at-least-once).
//! * **Receiver rules.** A note whose sender the receiver friend-blocked or ignores is dropped silently at delivery
//!   but still marked delivered; `privacy.whisper` does not apply (a refusal with a notice to the sender cannot
//!   work for a stored note).

use super::actor;
use super::block::friend_blocks;
use crate::chat::ingress::{is_mode_of, mute_text, notice};
use crate::chat::{privacy, send_to};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Outbox;
use crate::social::party::now_secs;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::CharacterId;
use flyff_common::gm::ModeFlags;
use flyff_db::model::MAX_NOTE_BYTES;
use flyff_db::{CharacterLoad, DbOp, NoteRecord};
use flyff_proto::pb;
use std::collections::BTreeMap;

/// Undelivered notes per receiver (the `TAG_STR` limit of the original): the 21st is refused.
pub const MAX_UNDELIVERED: u32 = 20;

/// The batch one join showed: the notes (oldest first, ids kept) and the mark still to be written.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    /// At most [`MAX_UNDELIVERED`] notes of the load.
    notes: Vec<NoteRecord>,
    /// `true` until [`after_join`] wrote `MarkNotesDelivered` for this batch.
    unmarked: bool,
}

/// The note state of the server (`Social.notes`): counts of the undelivered notes and the batches shown to the
/// characters in the world.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoteBox {
    /// Undelivered notes per receiver (entries are non-zero).
    counts: BTreeMap<CharacterId, u32>,
    /// The batch shown to each character in the world.
    shown: BTreeMap<CharacterId, Shown>,
}

impl NoteBox {
    /// The box of `StartupState.note_counts`.
    pub fn from_counts(rows: &[(CharacterId, u32)]) -> NoteBox {
        NoteBox {
            counts: rows
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(c, n)| (*c, *n))
                .collect(),
            shown: BTreeMap::new(),
        }
    }

    /// The undelivered notes of `c`.
    pub fn count(&self, c: CharacterId) -> u32 {
        self.counts.get(&c).copied().unwrap_or(0)
    }

    /// `true` when `c` cannot take another note.
    pub fn is_full(&self, c: CharacterId) -> bool {
        self.count(c) >= MAX_UNDELIVERED
    }

    /// One more undelivered note for `c`.
    pub(crate) fn add(&mut self, c: CharacterId) {
        let n = self.counts.entry(c).or_insert(0);
        *n = n.saturating_add(1);
    }

    /// `n` notes of `c` were delivered.
    pub(crate) fn delivered(&mut self, c: CharacterId, n: u32) {
        if let Some(count) = self.counts.get_mut(&c) {
            *count = count.saturating_sub(n);
            if *count == 0 {
                self.counts.remove(&c);
            }
        }
    }

    /// The notes the join of `c` showed (empty while `c` is not in the world).
    pub fn batch(&self, c: CharacterId) -> &[NoteRecord] {
        self.shown.get(&c).map_or(&[], |s| s.notes.as_slice())
    }

    /// `true` while the batch of `c` still waits for its `MarkNotesDelivered`.
    pub fn unmarked(&self, c: CharacterId) -> bool {
        self.shown.get(&c).is_some_and(|s| s.unmarked)
    }
}

// ---- the life cycle -------------------------------------------------------------------------------------------------

/// The join: keeps the oldest [`MAX_UNDELIVERED`] undelivered notes of `load` (a larger backlog stays in the
/// database for the next login). Called from `WorldRules::join` before the join batch is written, right after
/// `messenger::on_join`.
pub fn on_join(svc: &mut Services, load: &CharacterLoad) {
    let c = load.identity.id;
    let notes: Vec<NoteRecord> = load
        .notes
        .iter()
        .take(MAX_UNDELIVERED as usize)
        .cloned()
        .collect();
    if notes.is_empty() {
        svc.social.notes.shown.remove(&c);
    } else {
        svc.social.notes.shown.insert(
            c,
            Shown {
                notes,
                unmarked: true,
            },
        );
    }
}

/// The final removal of the avatar: its batch goes.
pub fn on_leave(svc: &mut Services, c: CharacterId) {
    svc.social.notes.shown.remove(&c);
}

/// After the join batch (`on_player_joined`, 21 §3.4 join step 7): the notes shown are marked delivered and leave
/// the count. Runs once per join; a re-attach finds the batch marked and does nothing.
pub fn after_join(svc: &mut Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    let Some(shown) = svc.social.notes.shown.get_mut(&c) else {
        return;
    };
    if !shown.unmarked {
        return;
    }
    shown.unmarked = false;
    let up_to_id = shown.notes.iter().map(|n| n.id).max().unwrap_or(0);
    let n = u32::try_from(shown.notes.len()).unwrap_or(u32::MAX);
    svc.social.notes.delivered(c, n);
    ctx.persist.op(DbOp::MarkNotesDelivered {
        to_id: c,
        up_to_id,
        at: now_secs(ctx),
    });
}

// ---- the join step -----------------------------------------------------------------------------------------------------

/// `YYYYMMDD` of the Unix time `created_at` in the zone `utc_offset_s` seconds east of UTC (the `date` of a `Tag`
/// note; the zone is the offset of the current tick, so a DST change moves old notes by an hour at most).
pub fn note_date(created_at: flyff_common::UnixTime, utc_offset_s: i32) -> u32 {
    let local = created_at.0.saturating_add(i64::from(utc_offset_s));
    let days = local.div_euclid(86_400);
    // days since 1970-01-01 -> proleptic Gregorian date (H. Hinnant's civil_from_days)
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    u32::try_from(y * 10_000 + m * 100 + d).unwrap_or(0)
}

/// The notes of the batch of `c` that survive the receiver's rules (a friend-blocked or ignored sender is dropped),
/// as the wire records. The sender's name comes from the player directory and is empty for a purged character.
fn visible_notes(cx: &JoinCtx<'_>, c: CharacterId) -> Vec<pb::tag::Note> {
    let batch = cx.svc.social.notes.batch(c);
    let offset = cx.clock.local.utc_offset_s;
    batch
        .iter()
        .filter_map(|n| {
            let from_name = cx
                .svc
                .presence
                .get(n.from_id)
                .map_or("", |e| e.name.as_str());
            let dropped = friend_blocks(cx.svc, c, n.from_id)
                || (!from_name.is_empty() && privacy::ignores(cx.worlds, cx.player, from_name));
            (!dropped).then(|| pb::tag::Note {
                from_name: from_name.to_owned(),
                date: note_date(n.created_at, offset),
                text: n.text.clone(),
            })
        })
        .collect()
}

/// The join step `Tag`: the undelivered notes of the joiner, only when there is something to show (21 §3.5).
#[derive(Debug, Clone, Copy)]
pub struct TagStep;

impl JoinContributor for TagStep {
    fn step(&self) -> JoinStep {
        JoinStep::Tag
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(c) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let notes = visible_notes(cx, c);
        if !notes.is_empty() {
            out.to_session(cx.session, 0, pb::Tag { notes });
        }
    }
}

// ---- SendTag -------------------------------------------------------------------------------------------------------------

/// `text` cut to at most `max` bytes on a character boundary (the original's `szString[255] = 0`, but never in the
/// middle of a UTF-8 sequence).
pub fn cut_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// `SendTag{to, text}` (`CDPCacheSrvr::OnSendTag`, 21 §9.6). The sender is the session's player (VR-001); every
/// refusal is silent unless stated:
///
/// 1. `SAYTALK` (GM silence) drops it; a mute gives the mute text and no result (a note is an offline instant
///    message, like `/say`);
/// 2. `to` is not in my friend list: ignored;
/// 3. my block flag on `to`: `InsertTagResult{false}`;
/// 4. `to` is online (or not a known character): ignored; `to` has [`MAX_UNDELIVERED`] undelivered notes:
///    `InsertTagResult{false}`;
/// 5. the text is cut to [`MAX_NOTE_BYTES`] bytes and trimmed; empty: ignored;
/// 6. else `InsertNote`, the count +1 and `InsertTagResult{true}`.
pub fn on_send_tag(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::SendTag) {
    let Some(a) = actor(ctx, who) else { return };
    if is_mode_of(ctx, who, ModeFlags::SAYTALK) {
        return;
    }
    if let Some(tid) = ctx.player(who).and_then(mute_text) {
        notice::defined(ctx, who, tid, Vec::new());
        return;
    }
    let to = CharacterId(m.to);
    let Some(view) = svc
        .social
        .messenger
        .get(a.character)
        .and_then(|l| l.friends.get(&to))
    else {
        return;
    };
    if view.blocked {
        send_to(ctx, who, pb::InsertTagResult { ok: false });
        return;
    }
    // an online friend is reached by the instant message (the original ignores it); so is a character nobody knows
    if svc.presence.get(to).is_none_or(|e| e.online.is_some()) {
        return;
    }
    if svc.social.notes.is_full(to) {
        send_to(ctx, who, pb::InsertTagResult { ok: false });
        return;
    }
    let text = cut_bytes(&m.text, MAX_NOTE_BYTES).trim();
    if text.is_empty() {
        return;
    }
    ctx.persist.op(DbOp::InsertNote(NoteRecord::new(
        to,
        a.character,
        text,
        now_secs(ctx),
    )));
    svc.social.notes.add(to);
    send_to(ctx, who, pb::InsertTagResult { ok: true });
}
