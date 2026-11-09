//! The server taskbar module (Plan/42 §11.6-§11.7, 13 §6.8; WP-UI-18): the five client messages that edit the
//! shortcut bars, the `TaskBar` message of the join batch, `reset_skill_shortcuts` for doc 16 and the action-point
//! costs of the skill queue for doc 17.
//!
//! The server only stores and bounds-checks; it never interprets a shortcut (doc 17 reads the queue). The runtime
//! state is the `TaskbarDoc` of the player (`PlayerExt.taskbar`, 13 §6.8: non-empty slots only, kept sorted by slot
//! so the snapshot and the `TaskBar` message list them in the legacy loop order). The action points live in
//! `PlayerExt.skills_rt.action_points` and are merged in when the document is saved (`persist::snapshot::docs`) and
//! when `TaskBar` is built ([`player_taskbar_msg`]).
//!
//! | message | rule (src:WORLDSERVER/DPSrvr.cpp:2175-2303) |
//! |---|---|
//! | `AddAppletTaskBar{index, shortcut}` | `index >= 18` ignored; kind `NONE` clears; CHAT text cut to 127 bytes at a UTF-8 boundary, other kinds drop the text |
//! | `RemoveAppletTaskBar{index}` | `index >= 18` ignored |
//! | `AddItemTaskBar{bar, slot, shortcut}` | `bar >= 8` or `slot >= 9` ignored; a CHAT shortcut while 10 CHAT entries exist in the item bars (the target slot's own entry counts: KEEP-QUIRK) is refused with `DefinedText{TID_GAME_MAX_SHORTCUT_CHAT}` |
//! | `RemoveItemTaskBar{bar, slot}` | bounds as above |
//! | `SkillTaskBar{slots}` | more than 5 entries: the whole message is ignored; entries apply in order and the first `slot >= 5` stops the processing (the earlier entries stay applied); never a text |
//!
//! Shortcut kinds above 11 are refused (strict decoding of the protocol, GP-04) and logged at debug level. Changes
//! are persisted with the next character snapshot (no urgent save, as legacy); there is no audit event.

use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef};
use crate::mover::Mover;
use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::defs::text::TID_GAME_MAX_SHORTCUT_CHAT;
use flyff_db::TaskbarDoc;
use flyff_db::model::{AppletSlot, ItemBarSlot, QueueSlot, SHORTCUT_KIND_CHAT, ShortcutRecord};
use flyff_proto::pb;

/// `MAX_SLOT_APPLET`: slots of the applet bar.
pub const MAX_SLOT_APPLET: u8 = 18;
/// `MAX_SLOT_ITEM_COUNT`: item bars (pages); 8 since `__VER >= 11`.
pub const MAX_SLOT_ITEM_COUNT: u8 = 8;
/// `MAX_SLOT_ITEM`: slots of one item bar.
pub const MAX_SLOT_ITEM: u8 = 9;
/// `MAX_SLOT_QUEUE`: slots of the skill queue (the action slot).
pub const MAX_SLOT_QUEUE: u8 = 5;
/// `MAX_SHORTCUT_STRING - 1`: the longest chat macro in bytes.
pub const MAX_SHORTCUT_TEXT: usize = 127;
/// `__CSC_VER11_5`: at most this many CHAT shortcuts over all item bars.
pub const MAX_CHAT_SHORTCUTS: usize = 10;

/// `SHORTCUT_NONE`: an empty slot.
pub const SHORTCUT_NONE: u32 = 0;
/// `SHORTCUT_APPLET`.
pub const SHORTCUT_APPLET: u32 = 2;
/// `SHORTCUT_MOTION`.
pub const SHORTCUT_MOTION: u32 = 3;
/// `SHORTCUT_ITEM`.
pub const SHORTCUT_ITEM: u32 = 5;
/// `SHORTCUT_SKILL`: the kind `reset_skill_shortcuts` removes.
pub const SHORTCUT_SKILL: u32 = 6;
/// `SHORTCUT_CHAT`: the only kind with a text.
pub const SHORTCUT_CHAT: u32 = SHORTCUT_KIND_CHAT;
/// `SHORTCUT_LORDSKILL`: the highest legal kind.
pub const SHORTCUT_LORDSKILL: u32 = 11;

/// What an edit did (the handlers ignore it, the tests read it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// The slot now holds the shortcut.
    Set,
    /// The slot was cleared (kind `NONE` or a remove message).
    Cleared,
    /// A bar, slot or queue index is out of range: nothing changed.
    OutOfRange,
    /// The shortcut kind is above 11: nothing changed.
    BadKind,
    /// The tenth-CHAT limit: nothing changed (the caller tells the player).
    ChatLimit,
    /// `SkillTaskBar` with more than 5 entries: nothing changed.
    TooMany,
}

/// The `ShortcutRecord` of a wire shortcut; `None` for a kind above 11. A CHAT text is cut to 127 bytes at a UTF-8
/// boundary and every other kind drops its text.
pub fn record_from_wire(sc: &pb::Shortcut) -> Option<ShortcutRecord> {
    if sc.shortcut_type > SHORTCUT_LORDSKILL {
        return None;
    }
    let text = (sc.shortcut_type == SHORTCUT_CHAT)
        .then(|| truncate_utf8(&sc.text, MAX_SHORTCUT_TEXT).to_owned());
    Some(ShortcutRecord {
        kind: sc.shortcut_type,
        id: sc.id,
        type_: sc.r#type,
        index: sc.index,
        user_id: sc.user_id,
        data: sc.data,
        text,
    })
}

/// The wire `Shortcut` of a record (the text only for CHAT).
pub fn wire_from_record(r: &ShortcutRecord) -> pb::Shortcut {
    pb::Shortcut {
        shortcut_type: r.kind,
        id: r.id,
        r#type: r.type_,
        index: r.index,
        user_id: r.user_id,
        data: r.data,
        text: if r.kind == SHORTCUT_CHAT {
            r.text.clone().unwrap_or_default()
        } else {
            String::new()
        },
    }
}

/// The longest prefix of `s` of at most `max` bytes that ends on a character boundary.
pub fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("")
}

/// A slot index that fits `limit`, as the `u8` the document stores.
fn slot_u8(v: u32, limit: u8) -> Option<u8> {
    u8::try_from(v).ok().filter(|v| *v < limit)
}

/// `OnAddAppletTaskBar` on the document.
pub fn add_applet(doc: &mut TaskbarDoc, index: u32, sc: &pb::Shortcut) -> Edit {
    let Some(slot) = slot_u8(index, MAX_SLOT_APPLET) else {
        return Edit::OutOfRange;
    };
    let Some(rec) = record_from_wire(sc) else {
        return Edit::BadKind;
    };
    doc.applets.retain(|a| a.slot != slot);
    if rec.kind == SHORTCUT_NONE {
        return Edit::Cleared;
    }
    doc.applets.push(AppletSlot {
        slot,
        shortcut: rec,
    });
    doc.applets.sort_by_key(|a| a.slot);
    Edit::Set
}

/// `OnRemoveAppletTaskBar` on the document.
pub fn remove_applet(doc: &mut TaskbarDoc, index: u32) -> Edit {
    let Some(slot) = slot_u8(index, MAX_SLOT_APPLET) else {
        return Edit::OutOfRange;
    };
    doc.applets.retain(|a| a.slot != slot);
    Edit::Cleared
}

/// The CHAT shortcuts in the item bars (the 8 x 9 array of the legacy loop; entries outside it and duplicates of a
/// damaged document are not counted).
pub fn count_chat_shortcuts(doc: &TaskbarDoc) -> usize {
    let mut seen = [[false; MAX_SLOT_ITEM as usize]; MAX_SLOT_ITEM_COUNT as usize];
    for s in &doc.items {
        if s.shortcut.kind != SHORTCUT_CHAT {
            continue;
        }
        if let Some(cell) = seen
            .get_mut(usize::from(s.page))
            .and_then(|row| row.get_mut(usize::from(s.slot)))
        {
            *cell = true;
        }
    }
    seen.iter().flatten().filter(|c| **c).count()
}

/// `OnAddItemTaskBar` on the document.
pub fn add_item(doc: &mut TaskbarDoc, bar: u32, slot: u32, sc: &pb::Shortcut) -> Edit {
    let (Some(page), Some(slot)) = (
        slot_u8(bar, MAX_SLOT_ITEM_COUNT),
        slot_u8(slot, MAX_SLOT_ITEM),
    ) else {
        return Edit::OutOfRange;
    };
    // the legacy reads the kind first, so a refused CHAT leaves the rest of the message unread
    if sc.shortcut_type == SHORTCUT_CHAT && count_chat_shortcuts(doc) >= MAX_CHAT_SHORTCUTS {
        // KEEP-QUIRK (DPSrvr.cpp:2252-2264): the count includes the entry of the target slot itself, so replacing a
        // CHAT macro is refused as well while ten exist
        return Edit::ChatLimit;
    }
    let Some(rec) = record_from_wire(sc) else {
        return Edit::BadKind;
    };
    doc.items.retain(|s| !(s.page == page && s.slot == slot));
    if rec.kind == SHORTCUT_NONE {
        return Edit::Cleared;
    }
    doc.items.push(ItemBarSlot {
        page,
        slot,
        shortcut: rec,
    });
    doc.items.sort_by_key(|s| (s.page, s.slot));
    Edit::Set
}

/// `OnRemoveItemTaskBar` on the document.
pub fn remove_item(doc: &mut TaskbarDoc, bar: u32, slot: u32) -> Edit {
    let (Some(page), Some(slot)) = (
        slot_u8(bar, MAX_SLOT_ITEM_COUNT),
        slot_u8(slot, MAX_SLOT_ITEM),
    ) else {
        return Edit::OutOfRange;
    };
    doc.items.retain(|s| !(s.page == page && s.slot == slot));
    Edit::Cleared
}

/// `OnSkillTaskBar` on the document. The returned number is how many entries were applied.
pub fn set_queue(doc: &mut TaskbarDoc, entries: &[pb::TaskBarQueueSlot]) -> (Edit, usize) {
    if entries.len() > usize::from(MAX_SLOT_QUEUE) {
        return (Edit::TooMany, 0);
    }
    let mut applied = 0;
    let mut result = Edit::Set;
    for e in entries {
        let Some(slot) = slot_u8(e.slot, MAX_SLOT_QUEUE) else {
            // the legacy `return`s: the entries before stay applied
            result = Edit::OutOfRange;
            break;
        };
        let Some(sc) = e.shortcut.as_ref() else {
            // a missing shortcut is an all-zero SHORTCUT (kind NONE)
            doc.queue.retain(|q| q.slot != slot);
            applied += 1;
            continue;
        };
        let Some(mut rec) = record_from_wire(sc) else {
            result = Edit::BadKind;
            break;
        };
        rec.text = None; // the queue never stores a text
        doc.queue.retain(|q| q.slot != slot);
        if rec.kind != SHORTCUT_NONE {
            doc.queue.push(QueueSlot {
                slot,
                shortcut: rec,
            });
        }
        applied += 1;
    }
    doc.queue.sort_by_key(|q| q.slot);
    (result, applied)
}

/// `InitTaskBarShorcutKind(SHORTCUT_SKILL)`: removes every SKILL entry of the applet bar, the item bars and the queue;
/// `true` when something was removed (doc 16 then resends the taskbar).
pub fn reset_skill_shortcuts(doc: &mut TaskbarDoc) -> bool {
    reset_kind(doc, SHORTCUT_SKILL)
}

/// `InitTaskBarShorcutKind(kind)`.
pub fn reset_kind(doc: &mut TaskbarDoc, kind: u32) -> bool {
    let before = doc.applets.len() + doc.items.len() + doc.queue.len();
    doc.applets.retain(|a| a.shortcut.kind != kind);
    doc.items.retain(|s| s.shortcut.kind != kind);
    doc.queue.retain(|q| q.shortcut.kind != kind);
    before != doc.applets.len() + doc.items.len() + doc.queue.len()
}

/// `CUserTaskBar::Serialize` (storing): the `TaskBar` message of a document. Only the non-`NONE` slots inside the
/// arrays are listed, in the legacy loop order (applets by slot, item bars by page then slot, queue by slot). A damaged
/// document (unsorted, duplicated or out-of-range entries) is canonicalised: the last entry of a slot wins.
pub fn taskbar_msg(doc: &TaskbarDoc) -> pb::TaskBar {
    let mut applets: [Option<&ShortcutRecord>; MAX_SLOT_APPLET as usize] =
        [None; MAX_SLOT_APPLET as usize];
    for a in &doc.applets {
        if let Some(cell) = applets.get_mut(usize::from(a.slot)) {
            *cell = Some(&a.shortcut);
        }
    }
    let mut items =
        [[None::<&ShortcutRecord>; MAX_SLOT_ITEM as usize]; MAX_SLOT_ITEM_COUNT as usize];
    for s in &doc.items {
        if let Some(cell) = items
            .get_mut(usize::from(s.page))
            .and_then(|row| row.get_mut(usize::from(s.slot)))
        {
            *cell = Some(&s.shortcut);
        }
    }
    let mut queue: [Option<&ShortcutRecord>; MAX_SLOT_QUEUE as usize] =
        [None; MAX_SLOT_QUEUE as usize];
    for q in &doc.queue {
        if let Some(cell) = queue.get_mut(usize::from(q.slot)) {
            *cell = Some(&q.shortcut);
        }
    }
    let live = |r: &&ShortcutRecord| r.kind != SHORTCUT_NONE && r.kind <= SHORTCUT_LORDSKILL;
    pb::TaskBar {
        applets: applets
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let r = (*r)?;
                live(&r).then(|| pb::TaskBarApplet {
                    slot: i as u32,
                    shortcut: Some(wire_from_record(r)),
                })
            })
            .collect(),
        items: items
            .iter()
            .enumerate()
            .flat_map(|(page, row)| {
                row.iter().enumerate().filter_map(move |(slot, r)| {
                    let r = (*r)?;
                    live(&r).then(|| pb::TaskBarItem {
                        bar: page as u32,
                        slot: slot as u32,
                        shortcut: Some(wire_from_record(r)),
                    })
                })
            })
            .collect(),
        queue: queue
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let r = (*r)?;
                live(&r).then(|| {
                    // the queue never carries a text (`SCnoStr`)
                    let mut wire = wire_from_record(r);
                    wire.text.clear();
                    pb::TaskBarQueueSlot {
                        slot: i as u32,
                        shortcut: Some(wire),
                    }
                })
            })
            .collect(),
        action_point: doc.action_points,
    }
}

/// The `TaskBar` message of a player: the document with the live action points of `skills_rt`.
pub fn player_taskbar_msg(m: &Mover) -> Option<pb::TaskBar> {
    let p = m.player()?;
    let mut msg = taskbar_msg(&p.taskbar);
    msg.action_point = p.skills_rt.action_points;
    Some(msg)
}

/// Sends the `TaskBar` of `who` to its own session again (job change and skill reset, doc 16 WP-MOVER-18/19).
pub fn resend(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(msg) = ctx.worlds.mover(who.0).and_then(player_taskbar_msg) else {
        return;
    };
    crate::chat::send_to(ctx, who, msg);
}

/// `SKILL`-kind reset of the player's taskbar with the resend when something changed (the call doc 16 makes after a
/// job change and after a skill reset). Returns `true` when the bars changed.
pub fn reset_player_skill_shortcuts(ctx: &mut GameCtx<'_>, who: PlayerRef) -> bool {
    let changed = ctx
        .worlds
        .mover_mut(who.0)
        .and_then(Mover::player_mut)
        .is_some_and(|p| reset_skill_shortcuts(&mut p.taskbar));
    if changed {
        resend(ctx, who);
    }
    changed
}

// ---- action points of the skill queue (doc 17 WP-SKILL-09 reads these) -----------------------------------------------

/// The action points the execution of the queue spends before the skill in queue slot `index` (`SetNextSkill`,
/// UserTaskBar.cpp:203-242): nothing for slot 0, then 6, 8, 11 and 30. `SM_ACTPOINT` mode spends nothing.
pub fn queue_ap_cost(index: usize, sm_actpoint: bool) -> i32 {
    if sm_actpoint {
        return 0;
    }
    match index {
        1 => 6,
        2 => 8,
        3 => 11,
        4 => 30,
        _ => 0,
    }
}

/// What `SetNextSkill` decides for queue slot `index` given the current action points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueStep {
    /// The queue ends (`OnEndSkillQueue`): past the last slot, an empty slot, or too few action points.
    End,
    /// Use the skill of this shortcut; the action points become `ap_after` (never negative).
    Use {
        /// The queue shortcut to execute (its `id` is the skill index).
        skill_id: u32,
        /// The action points after the cost.
        ap_after: i32,
    },
}

/// `SetNextSkill` for the slot `index` of `doc.queue`.
pub fn next_queue_step(
    doc: &TaskbarDoc,
    index: usize,
    action_points: i32,
    sm_actpoint: bool,
) -> QueueStep {
    let ap = action_points - queue_ap_cost(index, sm_actpoint);
    let slot = u8::try_from(index).ok();
    let entry = slot.and_then(|s| doc.queue.iter().find(|q| q.slot == s));
    match entry {
        Some(q) if index < usize::from(MAX_SLOT_QUEUE) && ap >= 0 && q.shortcut.kind != 0 => {
            QueueStep::Use {
                skill_id: q.shortcut.id,
                ap_after: ap.max(0),
            }
        }
        _ => QueueStep::End,
    }
}

// ---- handlers (called by `ClientCall`) -------------------------------------------------------------------------------

fn log_refused(what: &'static str, edit: Edit) {
    if matches!(edit, Edit::BadKind | Edit::OutOfRange | Edit::TooMany) {
        tracing::debug!(message = what, ?edit, "taskbar message ignored");
    }
}

fn doc_of<'a>(ctx: &'a mut GameCtx<'_>, who: PlayerRef) -> Option<&'a mut TaskbarDoc> {
    ctx.worlds
        .mover_mut(who.0)
        .and_then(Mover::player_mut)
        .map(|p| &mut p.taskbar)
}

/// `AddAppletTaskBar`.
pub fn on_add_applet(ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::AddAppletTaskBar) {
    let Some(doc) = doc_of(ctx, who) else {
        return;
    };
    let edit = match m.shortcut.as_ref() {
        Some(sc) => add_applet(doc, m.index, sc),
        None => add_applet(doc, m.index, &pb::Shortcut::default()),
    };
    log_refused("AddAppletTaskBar", edit);
}

/// `RemoveAppletTaskBar`.
pub fn on_remove_applet(ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::RemoveAppletTaskBar) {
    if let Some(doc) = doc_of(ctx, who) {
        log_refused("RemoveAppletTaskBar", remove_applet(doc, m.index));
    }
}

/// `AddItemTaskBar`; the tenth CHAT shortcut is answered with `DefinedText{TID_GAME_MAX_SHORTCUT_CHAT}`.
pub fn on_add_item(ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::AddItemTaskBar) {
    let Some(doc) = doc_of(ctx, who) else {
        return;
    };
    let edit = match m.shortcut.as_ref() {
        Some(sc) => add_item(doc, m.bar, m.slot, sc),
        None => add_item(doc, m.bar, m.slot, &pb::Shortcut::default()),
    };
    log_refused("AddItemTaskBar", edit);
    if edit == Edit::ChatLimit {
        notice::defined(ctx, who, TID_GAME_MAX_SHORTCUT_CHAT, Vec::new());
    }
}

/// `RemoveItemTaskBar`.
pub fn on_remove_item(ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::RemoveItemTaskBar) {
    if let Some(doc) = doc_of(ctx, who) {
        log_refused("RemoveItemTaskBar", remove_item(doc, m.bar, m.slot));
    }
}

/// `SkillTaskBar`.
pub fn on_skill_taskbar(ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::SkillTaskBar) {
    if let Some(doc) = doc_of(ctx, who) {
        let (edit, _) = set_queue(doc, &m.slots);
        log_refused("SkillTaskBar", edit);
    }
}

// ---- the join batch (15 §9.2 step 5) --------------------------------------------------------------------------------

/// The `TaskBar` step of the join sequence and of the re-attach replay (11 §2.5 item 5): the saved bars with the
/// live action points, wire `obj_id` 0.
#[derive(Debug, Default)]
pub struct TaskBarStep;

impl JoinContributor for TaskBarStep {
    fn step(&self) -> JoinStep {
        JoinStep::TaskBar
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        if let Some(msg) = cx.worlds.mover(cx.player).and_then(player_taskbar_msg) {
            out.to_session(cx.session, 0, msg);
        }
    }
}
