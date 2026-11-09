//! The client requests of the quest log (Plan/20 §9.10, WP-NPC-12): `RemoveQuestReq` and `QuestCheck`. Both are
//! handled under the session's `Player` scope; `ClientCall` forwards them here (`handlers.rs`).

use super::{QS_END, msg, ops};
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_proto::pb;

/// The 400 ms throttle SHARED with `ScriptDlg` (`PlayerNpc.script_req_at`, VR-061): the stamp is set before any
/// other check, so a request that is refused later still counts. `true` = drop the request.
fn throttled(ctx: &mut GameCtx<'_>, who: PlayerRef) -> bool {
    let now = ctx.clock.game_ms;
    let throttle = u64::from(ctx.cfg.rules.gameplay.npc_script_throttle_ms);
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
        return true;
    };
    if p.npc
        .script_req_at
        .is_some_and(|last| now.0 < last.0.saturating_add(throttle))
    {
        return true;
    }
    p.npc.script_req_at = Some(now);
    false
}

/// `RemoveQuestReq{quest_id}` (`DPSrvr.cpp:1643-1678`): the quest must be active, not at state 14, have a property
/// and not be marked `SetRemove(0)`; then it is cancelled (`RemoveQuestNtf{-1}`, audit `player`, the begin
/// disguise ends). Anything else is a silent no-op, with no frame, no text and no audit: so the legacy client may
/// keep sending the request when a timer reaches 0 after the server has expired the quest (TQ-20-02).
pub fn on_remove_quest_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::RemoveQuestReq,
) {
    if throttled(ctx, who) {
        return;
    }
    let q = m.quest_id;
    let cancellable = ctx
        .player(who)
        .and_then(|m| m.player())
        .and_then(|p| p.quests.find(q))
        .is_some_and(|r| r.state != QS_END)
        && ctx.data.quests.quests.get(q).is_some_and(|p| !p.no_remove);
    if cancellable {
        ops::cancel_quest(svc, ctx, who, q, ops::RemoveReason::Player);
    }
}

/// `QuestCheck{quest_id, check}` (`DPSrvr.cpp:12595-12640`): see [`super::QuestBook::check`]; the reply is
/// `QuestChecked{quest_ids}`, none for an ignored request. Existence and activeness are not validated (KEEP).
pub fn on_quest_check(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::QuestCheck,
) {
    let Some(book) = ctx
        .player_mut(who)
        .and_then(|mv| mv.player_mut())
        .map(|p| &mut p.quests)
    else {
        return;
    };
    if book.check(m.quest_id, m.check) {
        let reply = msg::quest_checked(book);
        send_to(ctx, who, reply);
    }
}
