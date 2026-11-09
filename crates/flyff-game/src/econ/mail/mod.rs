//! Mail (Plan/19 §11, WP-ECON-06; ref/13 §17, ref/07b §16, ref/05 §8; src:WORLDSERVER/DPSrvr.cpp:7208-7593,
//! _Common/post.cpp; PL-054, PL-073, PL-157, VR-029, VR-018): the [`MailService`], the six client handlers, the
//! storage fee, the 15-day expiry and the system-mail API.
//!
//! * **State.** [`MailService`] holds every undeleted mail of the server in memory (loaded from
//!   `StartupState.mails`, legacy `LoadPost`), the box of every receiver and the id counter
//!   (`meta.next_mail_id`). The database is the durable copy: every change below is queued in the same commit
//!   block as the in-memory change.
//! * **Atomicity (VR-018).** A player send is ONE `AtomicBatch{MailSend}` = the sender's snapshot (item, penya
//!   and postage already gone) + `InsertMail` + `RaiseCounter(next_mail_id)`; taking the item is one
//!   `MailTakeItem` batch (the receiver's snapshot with the item and `UpdateMail{take_item_at}`), taking the penya
//!   one `MailTakeGold` batch. The database guards every take (`Conflict` unless the mail still holds the
//!   attachment), so a crash or a stale game state can neither lose nor duplicate an attachment; the game never
//!   compensates a failed batch (Plan/19 §20.3), it logs it. Read / delete / expiry are single `UpdateMail` ops.
//! * **System mail.** [`MailService::send_system`] is the one API of the other documents (rewards, refunds,
//!   `create_item_or_mail`); it needs no `GameCtx`: the clock is sampled once per tick by the game loop
//!   ([`MailService::set_clock`]) and the envelope (`MODE_MAILBOX`) of an online receiver is raised by
//!   [`tick`] at the end of the tick.
//! * **Envelope.** `MODE_MAILBOX` follows "the receiver has an unread mail": raised for an online receiver by
//!   any new mail, cleared by the last read / delete / expiry, and re-evaluated at the join
//!   ([`on_player_joined`]).

pub(crate) mod commit;
mod handlers;

pub use commit::{ItemDelivery, create_item_or_mail};
pub use handlers::{on_post, on_query_box, on_read, on_remove, on_take_gold, on_take_item};

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::gold::GOLD_MAX;
use crate::item::model::Item;
use flyff_common::{CharacterId, MailId, UnixTime};
use flyff_db::model::{MailRecord, SYSTEM_SENDER};
use flyff_db::{DbOp, MailStateRecord, StartupState};
use flyff_proto::pb;
use flyff_rules::consts::MAILBOX_MODE;
use std::collections::{BTreeMap, BTreeSet};

/// `MAX_MAIL`: mails per box.
pub const MAX_MAIL: usize = 50;
/// `nPostGold`: the postage of a mail (the ENG value, KQ-12).
pub const POSTAGE: i64 = 500;
/// `MAX_KEEP_MAX_DAY`: a mail is deleted after 15 days.
pub const MAX_KEEP_MAX_DAY: i64 = 15;
/// `MAX_KEEP_BASIC_DAY`: the storage fee starts after 7 days.
pub const MAX_KEEP_BASIC_DAY: i64 = 7;
/// VR-029: the shortest time between two accepted sends of one player.
pub const MAIL_SEND_MIN_INTERVAL_MS: u64 = 1000;
/// VR-029: the shortest time between two accepted box queries (legacy `CHECK_TICK_FROM_CLIENT`).
pub const MAILBOX_QUERY_MIN_INTERVAL_MS: u64 = 3000;
/// The longest mail title (`MAX_MAILTITLE` 32 including the terminator).
pub const MAX_TITLE_BYTES: usize = 31;
/// The longest mail text (`MAX_MAILTEXT` 256 including the terminator).
pub const MAX_TEXT_BYTES: usize = 255;

/// `RemoveMail.kind` values.
pub mod remove_kind {
    /// The mail is gone.
    pub const MAIL: i32 = 0;
    /// The attached item was taken.
    pub const ITEM: i32 = 1;
    /// The attached penya was taken.
    pub const GOLD: i32 = 2;
    /// The mail is now read.
    pub const READ: i32 = 3;
}

/// One mail in memory (Plan/19 §11.1).
#[derive(Clone, Debug, PartialEq)]
pub struct MailEntry {
    /// `mails.id`.
    pub id: MailId,
    /// The receiver.
    pub receiver: CharacterId,
    /// The sender; `CharacterId(0)` is the system.
    pub sender: CharacterId,
    /// The title (at most 31 bytes).
    pub title: String,
    /// The text (at most 255 bytes).
    pub body: String,
    /// The attached penya (0 once taken).
    pub gold: i64,
    /// The attached item with all its data (PL-054); `None` once taken.
    pub item: Option<Item>,
    /// When it was sent.
    pub created_at: UnixTime,
    /// The receiver read it.
    pub read: bool,
}

impl MailEntry {
    /// The entry of a stored mail.
    pub fn from_record(r: &MailRecord) -> MailEntry {
        MailEntry {
            id: r.id,
            receiver: r.receiver_id,
            sender: r.sender_id,
            title: r.title.clone(),
            body: r.body.clone(),
            gold: r.gold,
            item: r.item.as_ref().map(|s| Item::from_record(&s.item)),
            created_at: r.created_at,
            read: r.read_at.is_some(),
        }
    }

    /// The row of `InsertMail`: unread, nothing taken.
    pub fn to_record(&self) -> MailRecord {
        let mut r = MailRecord::new(
            self.id,
            self.receiver,
            self.sender,
            self.title.clone(),
            self.body.clone(),
            self.created_at,
        )
        .with_gold(self.gold);
        if let Some(it) = &self.item {
            r = r.with_item(it.to_record(0));
        }
        if self.read {
            r.read_at = Some(self.created_at);
        }
        r
    }

    /// The wire form (`pb::Mail`): the item without a storage id, the creation time in unix seconds.
    pub fn to_pb(&self) -> pb::Mail {
        pb::Mail {
            mail_id: self.id.0,
            id_sender: self.sender.0,
            item: self.item.as_ref().map(|i| i.to_pb(0)),
            gold: u32::try_from(self.gold.clamp(0, GOLD_MAX)).unwrap_or(0),
            created_at: self.created_at.0,
            read: self.read,
            title: self.title.clone(),
            text: self.body.clone(),
        }
    }

    /// `true` for a mail of the system (rewards, refunds).
    pub fn is_system(&self) -> bool {
        self.sender == SYSTEM_SENDER
    }
}

/// `s` cut to at most `max` bytes at a character boundary.
pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default().to_owned()
}

/// The mail service (`Services.econ.mail`, Plan/19 §11.1).
#[derive(Debug)]
pub struct MailService {
    mails: BTreeMap<MailId, MailEntry>,
    /// receiver -> the ids of its mails, ascending (ids are allocated in increasing order).
    boxes: BTreeMap<CharacterId, Vec<MailId>>,
    /// `meta.next_mail_id`.
    next_id: u32,
    /// The wall clock of the current tick (sampled by the game loop).
    clock: UnixTime,
    /// Receivers whose envelope must be re-evaluated at the end of the tick.
    notify: BTreeSet<CharacterId>,
}

impl Default for MailService {
    fn default() -> Self {
        MailService {
            mails: BTreeMap::new(),
            boxes: BTreeMap::new(),
            next_id: 1,
            clock: UnixTime(0),
            notify: BTreeSet::new(),
        }
    }
}

impl MailService {
    /// The service of a starting game: every undeleted mail and the id counter (legacy `LoadPost`).
    pub fn from_startup(startup: &StartupState) -> MailService {
        let mut s = MailService::default();
        for r in &startup.mails {
            if r.deleted_at.is_some() {
                continue;
            }
            s.insert(MailEntry::from_record(r));
        }
        let highest = s.mails.keys().next_back().map_or(0, |id| id.0);
        s.next_id = startup
            .counters
            .next_mail_id
            .max(highest.saturating_add(1))
            .max(1);
        s
    }

    /// The wall clock of this tick; the game loop calls it first thing in every tick.
    pub fn set_clock(&mut self, now: UnixTime) {
        self.clock = now;
    }

    /// The clock of the current tick.
    pub fn clock(&self) -> UnixTime {
        self.clock
    }

    /// The id the next mail gets.
    pub fn next_id(&self) -> MailId {
        MailId(self.next_id)
    }

    /// Allocates the next mail id.
    pub(crate) fn alloc_id(&mut self) -> MailId {
        let id = MailId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub(crate) fn insert(&mut self, e: MailEntry) {
        self.boxes.entry(e.receiver).or_default().push(e.id);
        self.mails.insert(e.id, e);
    }

    /// Removes a mail from memory.
    pub(crate) fn remove(&mut self, id: MailId) -> Option<MailEntry> {
        let e = self.mails.remove(&id)?;
        if let Some(b) = self.boxes.get_mut(&e.receiver) {
            b.retain(|x| *x != id);
            if b.is_empty() {
                self.boxes.remove(&e.receiver);
            }
        }
        Some(e)
    }

    /// The mail `id`.
    pub fn get(&self, id: MailId) -> Option<&MailEntry> {
        self.mails.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: MailId) -> Option<&mut MailEntry> {
        self.mails.get_mut(&id)
    }

    /// The mail `id` when it belongs to `receiver`'s box.
    pub fn get_of(&self, receiver: CharacterId, id: MailId) -> Option<&MailEntry> {
        self.mails.get(&id).filter(|e| e.receiver == receiver)
    }

    /// The box of `receiver` in id order.
    pub fn box_of(&self, receiver: CharacterId) -> impl Iterator<Item = &MailEntry> + '_ {
        self.boxes
            .get(&receiver)
            .into_iter()
            .flatten()
            .filter_map(|id| self.mails.get(id))
    }

    /// Every mail in memory, newest id first (the admin console's history of system mails, 25 §8.2).
    pub fn newest(&self) -> impl Iterator<Item = &MailEntry> + '_ {
        self.mails.values().rev()
    }

    /// The number of mails in the box of `receiver`.
    pub fn box_len(&self, receiver: CharacterId) -> usize {
        self.boxes.get(&receiver).map_or(0, Vec::len)
    }

    /// `true` when `receiver` has a mail they did not read.
    pub fn has_unread(&self, receiver: CharacterId) -> bool {
        self.box_of(receiver).any(|e| !e.read)
    }

    /// The number of mails in memory.
    pub fn len(&self) -> usize {
        self.mails.len()
    }

    /// `true` when there is no mail.
    pub fn is_empty(&self) -> bool {
        self.mails.is_empty()
    }

    /// Asks [`tick`] to re-evaluate the envelope of `receiver` (if they are online).
    pub(crate) fn notify(&mut self, receiver: CharacterId) {
        self.notify.insert(receiver);
    }

    /// The receivers to re-evaluate, in id order.
    pub(crate) fn take_notifications(&mut self) -> Vec<CharacterId> {
        std::mem::take(&mut self.notify).into_iter().collect()
    }

    /// The ids of the mails created before `cutoff`.
    pub(crate) fn expired(&self, cutoff: UnixTime) -> Vec<MailId> {
        self.mails
            .values()
            .filter(|e| e.created_at.0 < cutoff.0)
            .map(|e| e.id)
            .collect()
    }

    /// `MailService::send_system` (Plan/19 §11.1, normative for docs 13, 17, 20, 21, 22, 23): inserts the entry
    /// at once and returns the ops for the caller's `AtomicBatch`: `[InsertMail(MailRecord),
    /// RaiseCounter{next_mail_id, id + 1}]`. The sender is the system (0); `gold` is clamped to `0..=GOLD_MAX`; the
    /// item keeps its serial and its full data (PL-054); the title and text are cut to 31 / 255 bytes. None of the
    /// player-send checks apply (rate, box size, postage, NPC range). An online receiver gets `MODE_MAILBOX` at
    /// the end of the tick.
    pub fn send_system(
        &mut self,
        receiver: CharacterId,
        title: &str,
        body: &str,
        item: Option<Item>,
        gold: i64,
    ) -> [DbOp; 2] {
        self.send_system_with_id(receiver, title, body, item, gold)
            .1
    }

    /// [`MailService::send_system`] that also returns the new mail's id.
    pub fn send_system_with_id(
        &mut self,
        receiver: CharacterId,
        title: &str,
        body: &str,
        item: Option<Item>,
        gold: i64,
    ) -> (MailId, [DbOp; 2]) {
        let id = self.alloc_id();
        let entry = MailEntry {
            id,
            receiver,
            sender: SYSTEM_SENDER,
            title: clip(title, MAX_TITLE_BYTES),
            body: clip(body, MAX_TEXT_BYTES),
            gold: gold.clamp(0, GOLD_MAX),
            item: item.map(|mut i| {
                i.reserved = 0;
                i.vendor_price = 0;
                i
            }),
            created_at: self.clock,
            read: false,
        };
        let ops = DbOp::mail_ops(entry.to_record());
        self.insert(entry);
        self.notify(receiver);
        (id, ops)
    }
}

// ---- the envelope (MODE_MAILBOX) ------------------------------------------------------------------------

/// Sets or clears `MODE_MAILBOX` of an online player and broadcasts `ModifyMode` when the bit changed.
pub(crate) fn set_envelope(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, on: bool) {
    let core = svc.stats.handle();
    let mut out = crate::stats::out::Out::new();
    if let Some(m) = ctx.worlds.mover_mut(who.0) {
        let has = m.mv.mode & MAILBOX_MODE != 0;
        if on && !has {
            crate::stats::modes::set_mode_bits(m, &core, MAILBOX_MODE, &mut out);
        } else if !on && has {
            crate::stats::modes::clear_mode_bits(m, &core, MAILBOX_MODE, &mut out);
        }
    }
    out.flush(ctx, who.0);
}

/// The avatar of an online character.
fn avatar_of(svc: &Services, c: CharacterId) -> Option<PlayerRef> {
    svc.presence
        .get(c)
        .and_then(|e| e.online)
        .map(|o| PlayerRef(o.obj))
}

/// Makes the envelope of `receiver` match their box (when they are online).
pub(crate) fn refresh_envelope(svc: &mut Services, ctx: &mut GameCtx<'_>, receiver: CharacterId) {
    let Some(who) = avatar_of(svc, receiver) else {
        return;
    };
    let unread = svc.econ.mail.has_unread(receiver);
    set_envelope(svc, ctx, who, unread);
}

/// End of tick (phase 3, after the item tick): raises `MODE_MAILBOX` for every online receiver of a mail that
/// arrived this tick (system mails, sends, re-mails) and clears it where the last unread mail went away.
pub fn tick(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    for receiver in svc.econ.mail.take_notifications() {
        refresh_envelope(svc, ctx, receiver);
    }
}

/// After the join (13 §20.5): the envelope shows exactly when unread mail exists.
pub fn on_player_joined(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(c) = ctx.entry(who).map(|e| e.character) else {
        return;
    };
    let unread = svc.econ.mail.has_unread(c);
    set_envelope(svc, ctx, who, unread);
}

// ---- expiry (P8) ----------------------------------------------------------------------------------------

/// More expired mails than this in one run are soft-deleted by one `ExpireMails` bulk op.
const BULK_EXPIRE_FROM: usize = 32;

/// P8 `econ::mail::expire` (Plan/19 §11.6; 10 §5.3, every 60 s): mails older than 15 days are soft-deleted
/// **with** their attachments (KEEP: no return to the sender), one `UpdateMail{deleted_at}` op per mail (one
/// `ExpireMails` op when more than 32 expire at once) and one `mail.expire` audit line per mail; an online receiver is not told (the next box query no longer lists
/// them), only the envelope follows. Purging the rows is WP-DB-16.
pub fn expire(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.unix_ms.to_unix_time();
    let cutoff = UnixTime(now.0 - MAX_KEEP_MAX_DAY * 86_400);
    let ids = svc.econ.mail.expired(cutoff);
    // a long downtime can expire thousands at once: one idempotent bulk op instead of one op per mail
    let bulk = ids.len() > BULK_EXPIRE_FROM;
    if bulk {
        ctx.persist.op(DbOp::ExpireMails {
            created_before: cutoff,
            at: now,
        });
    }
    let mut receivers = BTreeSet::new();
    for id in ids {
        let Some(e) = svc.econ.mail.remove(id) else {
            continue;
        };
        if !bulk {
            ctx.persist
                .op(DbOp::UpdateMail(MailStateRecord::delete(id, now)));
        }
        ctx.audit
            .emit(flyff_db::audit::econ::mail_expire(&e.to_record()));
        receivers.insert(e.receiver);
    }
    for r in receivers {
        refresh_envelope(svc, ctx, r);
    }
}
