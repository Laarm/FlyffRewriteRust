//! The quest primitives (Plan/20 §9.6, WP-NPC-12): `SetQuest` / `RemoveQuestNtf` frames, the texts, the audit
//! lines and the expiry rule around the [`QuestBook`](super::QuestBook). Free functions over
//! `(svc, ctx, who, ..)`; the `DialogHost` helpers (`npc/quest_ops.rs`), the client requests, the GM API and the
//! progress tracking of WP-NPC-14/15 all go through them.

use super::{QS_BEGIN, QS_END, QUEST_CREGUILD, msg};
use crate::chat::ingress::notice;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::stats::Out;
use flyff_common::defs::text::{TID_EVE_CANCELQUEST, TID_EVE_ENDQUEST, TID_EVE_STARTQUEST};
use flyff_db::audit::ev;
use flyff_db::model::QuestRecord;
use serde_json::json;
use std::sync::Arc;

/// Why a quest leaves the book (the `reason` of the audit line `quest.remove`, 13 §25.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RemoveReason {
    /// The player cancelled it (`RemoveQuestReq`).
    Player,
    /// The time ran out (PL-108).
    Timeout,
    /// A script removed it (`RemoveQuest`).
    Script,
    /// A GM removed it.
    Gm,
    /// The rewards of another quest removed it (`SetEndRemoveQuest`).
    Reward,
}

impl RemoveReason {
    /// `player|timeout|script|gm|reward`.
    pub fn as_str(self) -> &'static str {
        match self {
            RemoveReason::Player => "player",
            RemoveReason::Timeout => "timeout",
            RemoveReason::Script => "script",
            RemoveReason::Gm => "gm",
            RemoveReason::Reward => "reward",
        }
    }
}

/// `"title"` as the quest texts print it (`AddDefinedText(.., "\"%s\"", title)`).
fn quoted_title(title: &str) -> Vec<String> {
    vec![format!("\"{title}\"")]
}

/// `CMover::SetQuest(q, state)` plus its frame: the book sets the record (PL-160 timer and cap rules), `SetQuest`
/// goes to the player when a record was stored, then the expiry rule runs ([`expire_if_due`]: a timed quest whose
/// set zeroed the timer ends at once, PL-108). Returns the stored record.
pub fn apply_set(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    q: u32,
    state: u8,
) -> Option<QuestRecord> {
    let now = ctx.clock.game_ms;
    let data = Arc::clone(ctx.data);
    let record = ctx
        .player_mut(who)?
        .player_mut()?
        .quests
        .set(q, state, &data.quests, now);
    if let Some(r) = &record {
        send_to(ctx, who, msg::set_quest(r));
    }
    expire_if_due(svc, ctx, who, q);
    record
}

/// The PL-108 expiry rule: a quest with `SetEndCondLimitTime > 0` that is active, not at state 14, at 0 minutes
/// and not frozen is cancelled with reason `timeout` right after the `SetQuest` that showed the 0 (so the client
/// still prints its timeout text), ignoring `SetRemove(0)` and the request throttle. Returns `true` when it
/// expired.
pub fn expire_if_due(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, q: u32) -> bool {
    let timed = ctx.data.quests.quests.get(q).is_some_and(|p| p.is_timed());
    if !timed {
        return false;
    }
    let due = ctx
        .player(who)
        .and_then(|m| m.player())
        .and_then(|p| p.quests.find(q))
        .is_some_and(|r| r.state != QS_END && r.time_left_min == 0 && !r.timer_frozen);
    if due {
        cancel_quest(svc, ctx, who, q, RemoveReason::Timeout);
    }
    due
}

/// `set_quest_started(pc, q)` (`__SetQuest`): the text when the quest has a property, the book entry and its
/// `SetQuest`, the audit line `quest.start` (always: the original logged even a failure).
pub fn set_quest_started(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, q: u32) {
    if let Some(p) = ctx.data.quests.quests.get(q) {
        let args = quoted_title(&p.title);
        notice::defined(ctx, who, TID_EVE_STARTQUEST, args);
    }
    apply_set(svc, ctx, who, q, QS_BEGIN);
    crate::audit::emit_for(ctx, who, ev::QUEST_START, json!({ "quest": q }));
}

/// `set_quest_state(pc, q, s)` (`__SetQuestState`): state 14 prints `TID_EVE_ENDQUEST` (with a property and not for
/// the guild creation quest), logs `quest.complete` and asks for an urgent save; state 0 logs `quest.start`; then
/// the record is set and the expiry rule runs.
pub fn set_quest_state(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    q: u32,
    state: u8,
) {
    if state == QS_END {
        if q != QUEST_CREGUILD
            && let Some(p) = ctx.data.quests.quests.get(q)
        {
            let args = quoted_title(&p.title);
            notice::defined(ctx, who, TID_EVE_ENDQUEST, args);
        }
        crate::audit::emit_for(ctx, who, ev::QUEST_COMPLETE, json!({ "quest": q }));
        svc.saves.mark_urgent(who.0);
    } else if state == QS_BEGIN {
        crate::audit::emit_for(ctx, who, ev::QUEST_START, json!({ "quest": q }));
    }
    apply_set(svc, ctx, who, q, state);
}

/// Ends the disguise a quest start gave (`NoDisguise` to the near set).
pub(super) fn undo_disguise(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let mut out = Out::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        crate::stats::fashion::clear_disguise(m, &mut out);
    }
    out.flush(ctx, who.0);
}

/// `remove_quest(pc, q)` (`__RemoveQuest`): nothing happens for a quest without a property; `TID_EVE_CANCELQUEST`
/// when the quest is active below state 14; the quest leaves the active, completed and checked lists;
/// `RemoveQuestNtf{0, q}` is sent ALWAYS (also when nothing was removed); the begin disguise ends when the quest
/// was active and had set one; audit `quest.remove`.
pub fn remove_quest(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    q: u32,
    reason: RemoveReason,
) {
    let data = Arc::clone(ctx.data);
    let Some(prop) = data.quests.quests.get(q) else {
        return;
    };
    let state = ctx
        .player(who)
        .and_then(|m| m.player())
        .and_then(|p| p.quests.find(q))
        .map(|r| r.state);
    if state.is_some_and(|s| s < QS_END) {
        notice::defined(ctx, who, TID_EVE_CANCELQUEST, quoted_title(&prop.title));
    }
    if let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        p.quests.remove(q);
    }
    send_to(ctx, who, msg::remove_quest_ntf(0, q));
    if state.is_some() && prop.begin_set_disguise_mover_index != 0 {
        undo_disguise(ctx, who);
    }
    crate::audit::emit_for(
        ctx,
        who,
        ev::QUEST_REMOVE,
        json!({ "quest": q, "reason": reason.as_str() }),
    );
}

/// `cancel_quest(pc, q, reason)` (the `REMOVEQUEST` path and the timeout): the quest leaves the book,
/// `RemoveQuestNtf{-1, q}` makes the client print `TID_GAME_REMOVEQUEST`, audit `quest.remove{quest, reason}`, and
/// the begin disguise ends.
pub fn cancel_quest(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    q: u32,
    reason: RemoveReason,
) {
    if let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        p.quests.remove(q);
    }
    send_to(ctx, who, msg::remove_quest_ntf(-1, q));
    crate::audit::emit_for(
        ctx,
        who,
        ev::QUEST_REMOVE,
        json!({ "quest": q, "reason": reason.as_str() }),
    );
    let disguised = ctx
        .data
        .quests
        .quests
        .get(q)
        .is_some_and(|p| p.begin_set_disguise_mover_index != 0);
    if disguised {
        undo_disguise(ctx, who);
    }
}

/// The notes of the load normalisation of a joined player (13 §17.4 step 3): a `TimedOut` quest becomes the audit
/// line `quest.remove{quest, reason: "timeout"}`, the other notes one `info!` each. The notes are taken, so a
/// re-attach finds the list empty.
pub fn emit_load_notes(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let notes = ctx
        .player_mut(who)
        .and_then(|m| m.player_mut())
        .map(|p| std::mem::take(&mut p.quests.load_notes))
        .unwrap_or_default();
    for n in notes {
        match n {
            super::LoadNote::TimedOut(q) => crate::audit::emit_for(
                ctx,
                who,
                ev::QUEST_REMOVE,
                json!({ "quest": q, "reason": RemoveReason::Timeout.as_str() }),
            ),
            other => tracing::info!(player = who.0.0, "quest list normalised at load: {other:?}"),
        }
    }
}
