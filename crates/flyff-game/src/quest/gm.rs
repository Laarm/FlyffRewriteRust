//! The GM API of the quest engine (Plan/20 §9.14; ref/24 §6.4-6.6): the contract `gm/cmd/quest.rs` (WP-GM-25)
//! calls for `/bq /eq /qs /rq /raq /rcq`. The caller parses the commands and the target name and passes the legacy
//! WORD (`n as u16`, widened to `u32`); a target without a live mover is a silent no-op.

use super::{QS_BEGIN, QS_END, msg, ops};
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_db::audit::ev;
use serde_json::json;

fn live(ctx: &GameCtx<'_>, target: PlayerRef) -> bool {
    ctx.player(target).and_then(|m| m.player()).is_some()
}

/// `/bq`: `book.set(q, 0)` and `SetQuest`; no text, no start effects, no audit. A timed quest whose set left the
/// timer at 0 expires at once (the PL-108 rule inside [`ops::apply_set`]).
pub fn gm_begin(svc: &mut Services, ctx: &mut GameCtx<'_>, target: PlayerRef, q: u32) {
    if live(ctx, target) {
        ops::apply_set(svc, ctx, target, q, QS_BEGIN);
    }
}

/// `/eq`: `book.set(q, 14)` and `SetQuest`.
pub fn gm_end(svc: &mut Services, ctx: &mut GameCtx<'_>, target: PlayerRef, q: u32) {
    if live(ctx, target) {
        ops::apply_set(svc, ctx, target, q, QS_END);
    }
}

/// `/qs`: requires `0 <= s < 14`, else nothing happens; `true` when a record was stored.
pub fn gm_set_state(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: PlayerRef,
    q: u32,
    s: i32,
) -> bool {
    if !live(ctx, target) {
        return false;
    }
    match u8::try_from(s) {
        Ok(state) if state < QS_END => ops::apply_set(svc, ctx, target, q, state).is_some(),
        _ => false,
    }
}

/// `/rq`: the quest leaves the active, completed and checked lists; `RemoveQuestNtf{0, q}` and the audit line
/// `quest.remove{quest, reason: "gm", state}` (the active state or -1) are ALWAYS written, as `TextCmd_RemoveQuest`
/// did.
pub fn gm_remove(_svc: &mut Services, ctx: &mut GameCtx<'_>, target: PlayerRef, q: u32) {
    let Some(p) = ctx.player_mut(target).and_then(|m| m.player_mut()) else {
        return;
    };
    let state = p.quests.find(q).map_or(-1, |r| i32::from(r.state));
    p.quests.remove(q);
    send_to(ctx, target, msg::remove_quest_ntf(0, q));
    crate::audit::emit_for(
        ctx,
        target,
        ev::QUEST_REMOVE,
        json!({ "quest": q, "reason": ops::RemoveReason::Gm.as_str(), "state": state }),
    );
}

/// `/raq`: `RemoveAllQuest` (the active, completed and checked lists) and `RemoveQuestNtf{1, 0}`.
pub fn gm_remove_all(_svc: &mut Services, ctx: &mut GameCtx<'_>, target: PlayerRef) {
    let Some(p) = ctx.player_mut(target).and_then(|m| m.player_mut()) else {
        return;
    };
    p.quests.remove_all();
    send_to(ctx, target, msg::remove_quest_ntf(1, 0));
}

/// `/rcq`: `RemoveCompleteQuest` and `RemoveQuestNtf{2, 0}`.
pub fn gm_remove_completed(_svc: &mut Services, ctx: &mut GameCtx<'_>, target: PlayerRef) {
    let Some(p) = ctx.player_mut(target).and_then(|m| m.player_mut()) else {
        return;
    };
    p.quests.remove_all_completed();
    send_to(ctx, target, msg::remove_quest_ntf(2, 0));
}
