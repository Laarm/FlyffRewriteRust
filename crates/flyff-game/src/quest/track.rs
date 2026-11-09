//! Quest progress tracking (Plan/20 §9.9, WP-NPC-15): the per-tick row P19 `quest_process` (the patrol zones and
//! the minute timers of `CMover::ProcessQuest`, with the server-side expiry of PL-108), the kill counters of
//! `DoDie` step 7 with the party share, the text item (`IK2_TEXT`) and the ctrl port (`UA_QUEST`,
//! `UA_QUEST_END`).
//!
//! With the shipped quest files (empty, PD-01) every entry point returns at once: no draw, no message.
//! The expiry check itself is [`ops::expire_if_due`] (one predicate for every caller).

use super::index::PatrolRect;
use super::{QS_BEGIN, QS_END, QuestRules, msg, ops};
use crate::chat::send_to;
use crate::combat::hooks::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::ops::UiParam;
use crate::item::{Fx, ops::core as item_ops};
use crate::mover::Mover;
use crate::world::ctrl::CtrlQuestPort;
use flyff_common::defs::text::TID_EVE_STARTQUEST;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::quest::QuestProp;
use flyff_db::audit::ev;
use flyff_db::model::QuestRecord;
use serde_json::json;
use std::sync::Arc;

/// `CRect::PtInRect(CPoint((int)x, (int)z))` of a normalised rectangle: `left <= x < right`, `top <= z < bottom`.
pub fn patrol_contains(r: &PatrolRect, pos: Vec3) -> bool {
    let (x, z) = (pos.x as i32, pos.z as i32);
    r.left <= x && x < r.right && r.top <= z && z < r.bottom
}

/// The scheduler row P19 `quest_process` (period 1, phase 0): `CMover::ProcessQuest` for every avatar of the
/// world, in ascending `ObjId` order.
///
/// 1. The real data case: no timed and no patrol quest exists, so nothing runs.
/// 2. For an avatar with active quests: the minute is due when `now >= minute_due` (then `minute_due = now + 60 s`,
///    the `Reset()` of `m_timerQuestLimitTime`). For each active record in book order whose property exists:
///    (a) the patrol zone, every tick; (b) the minute timer, when the minute is due. A timer that reaches 0 is
///    expired right after the `SetQuest` that showed it ([`ops::expire_if_due`]: PL-108).
/// 3. The safety net: a timed record at 0 minutes that no handler expired (the ctrl port and the text item run over
///    `Fx` without `Services`) goes through [`ops::expire_if_due`] now, at most one tick later.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let rules = svc.quest.rules();
    if rules.index.timed.is_empty() && rules.index.patrol.is_empty() {
        return;
    }
    let now = ctx.clock.game_ms;
    let data = Arc::clone(ctx.data);
    let avatars: Vec<ObjId> = ctx.sessions.iter().map(|e| e.player).collect();
    for id in avatars {
        let who = PlayerRef(id);
        let Some(ids) = begin_pass(ctx, &data, who, now) else {
            continue;
        };
        for (q, minute) in ids {
            step(svc, ctx, &rules, &data, who, q, minute);
        }
        safety_net(svc, ctx, &rules, who);
    }
}

/// The record ids of an avatar's book that have a property, in book order, each with the "minute is due" flag of
/// this tick. `None` for an avatar without active quests.
fn begin_pass(
    ctx: &mut GameCtx<'_>,
    data: &GameData,
    who: PlayerRef,
    now: GameMs,
) -> Option<Vec<(u32, bool)>> {
    let book = &mut ctx.player_mut(who)?.player_mut()?.quests;
    if book.doc.active.is_empty() {
        return None;
    }
    let minute = now >= book.minute_due;
    if minute {
        book.minute_due = GameMs(now.0.saturating_add(super::book::QUEST_MINUTE_MS));
    }
    Some(
        book.doc
            .active
            .iter()
            .map(|r| u32::from(r.quest_id))
            .filter(|q| data.quests.quests.contains(*q))
            .map(|q| (q, minute))
            .collect(),
    )
}

/// One record of `ProcessQuest`: the patrol zone, then the minute timer.
fn step(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    rules: &QuestRules,
    data: &GameData,
    who: PlayerRef,
    q: u32,
    minute: bool,
) {
    let Some(prop) = data.quests.quests.get(q) else {
        return;
    };
    // (a) the patrol zone: no state, layer or alive check (KEEP)
    if let Some((world, rect)) = rules.index.patrol.get(&q) {
        patrol(ctx, who, q, *world, rect);
    }
    // (b) the minute timer
    if minute && prop.end_cond_limit_time > 0 {
        let shown = ctx
            .player_mut(who)
            .and_then(|m| m.player_mut())
            .and_then(|p| {
                p.quests
                    .doc
                    .active
                    .iter_mut()
                    .find(|r| u32::from(r.quest_id) == q)
            })
            .filter(|r| r.time_left_min != 0 && !r.timer_frozen)
            .map(|r| {
                r.time_left_min -= 1;
                *r
            });
        if let Some(r) = shown {
            send_to(ctx, who, msg::set_quest(&r));
            if r.time_left_min == 0 {
                ops::expire_if_due(svc, ctx, who, q);
            }
        }
    }
}

/// (a) of [`process`]: the player stands in the patrol zone of an active quest whose flag is not set yet.
fn patrol(ctx: &mut GameCtx<'_>, who: PlayerRef, q: u32, world: WorldId, rect: &PatrolRect) {
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    if m.core.world != world || !patrol_contains(rect, m.core.pos) {
        return;
    }
    let Some(p) = m.player_mut() else { return };
    let Some(r) = p
        .quests
        .doc
        .active
        .iter_mut()
        .find(|r| u32::from(r.quest_id) == q)
        .filter(|r| !r.patrol_done)
    else {
        return;
    };
    r.patrol_done = true;
    let sent = *r;
    send_to(ctx, who, msg::set_quest(&sent));
}

/// (c) of [`process`].
fn safety_net(svc: &mut Services, ctx: &mut GameCtx<'_>, rules: &QuestRules, who: PlayerRef) {
    let due: Vec<u32> = ctx
        .player(who)
        .and_then(|m| m.player())
        .map(|p| {
            p.quests
                .doc
                .active
                .iter()
                .filter(|r| {
                    r.state != QS_END
                        && r.time_left_min == 0
                        && !r.timer_frozen
                        && rules.index.timed.contains(&u32::from(r.quest_id))
                })
                .map(|r| u32::from(r.quest_id))
                .collect()
        })
        .unwrap_or_default();
    for q in due {
        ops::expire_if_due(svc, ctx, who, q);
    }
}

// ---- kills -------------------------------------------------------------------------------------------------------

/// `DeathHooks.quest_kill` (`DoDie` step 7; the pipeline calls it for a non-player victim and a player killer).
pub fn kill_hook(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    on_monster_killed(svc, ctx, k.attacker, k.victim);
}

/// The kill counters of `Mover.cpp:5420-5466`. The monster is the victim's `GetIndex()`; nothing happens when no
/// quest names it (the real-data case). For each active record of the killer in book order and each of the two kill
/// slots naming the monster:
///
/// * a party quest (`SetBeginCondParty(2)`) counts for every online member of the killer's party (the killer
///   included, under the same test) that is within 64 m (3-D) of the dead monster in the same world and layer, has
///   the quest in any state and has not reached the goal; a killer without a party counts for nobody (KEEP);
/// * any other quest counts for the killer alone, up to the goal.
///
/// There is no state check (KEEP); both slots naming the same monster both count.
pub fn on_monster_killed(svc: &mut Services, ctx: &mut GameCtx<'_>, killer: ObjId, victim: ObjId) {
    let rules = svc.quest.rules();
    let Some((monster, world, layer, pos)) = ctx
        .worlds
        .mover(victim)
        .map(|v| (v.core.index, v.core.world, v.core.layer, v.core.pos))
    else {
        return;
    };
    if !rules.index.kill_targets.contains_key(&monster) {
        return;
    }
    let data = Arc::clone(ctx.data);
    let records: Vec<u32> = ctx
        .worlds
        .player(killer)
        .and_then(|m| m.player())
        .map(|p| {
            p.quests
                .doc
                .active
                .iter()
                .map(|r| u32::from(r.quest_id))
                .collect()
        })
        .unwrap_or_default();
    for q in records {
        let Some(prop) = data.quests.quests.get(q) else {
            continue;
        };
        for slot in 0..2 {
            if prop.end_cond_kill_npc[slot].mover != monster {
                continue;
            }
            if prop.begin_cond_party == 2 {
                party_share(ctx, killer, prop, slot, (world, layer, pos));
            } else {
                count_kill(ctx, killer, prop, slot);
            }
        }
    }
}

/// `++kills[slot]` of one player's record when below the goal; sends `SetQuest`.
fn count_kill(ctx: &mut GameCtx<'_>, who: ObjId, prop: &QuestProp, slot: usize) {
    let goal = prop.end_cond_kill_npc[slot].num;
    let Some(r) = ctx
        .worlds
        .player_mut(who)
        .and_then(|m| m.player_mut())
        .and_then(|p| {
            p.quests
                .doc
                .active
                .iter_mut()
                .find(|r| u32::from(r.quest_id) == prop.id)
        })
    else {
        return;
    };
    if i32::from(r.kills[slot]) >= goal {
        return;
    }
    r.kills[slot] = r.kills[slot].saturating_add(1);
    let sent: QuestRecord = *r;
    send_to(ctx, PlayerRef(who), msg::set_quest(&sent));
}

/// The party loop of a party quest: members in party order within `IsValidArea(member, 64)` of the monster.
fn party_share(
    ctx: &mut GameCtx<'_>,
    killer: ObjId,
    prop: &QuestProp,
    slot: usize,
    place: (WorldId, LayerId, Vec3),
) {
    let members: Vec<ObjId> = ctx
        .worlds
        .player(killer)
        .and_then(|m| m.player())
        .and_then(|p| p.social.party_link.as_ref())
        .map(|l| l.online_objs().collect())
        .unwrap_or_default();
    for member in members {
        if ctx
            .worlds
            .mover(member)
            .is_some_and(|m| valid_area(m, place))
        {
            count_kill(ctx, member, prop, slot);
        }
    }
}

/// `CMover::IsValidArea(member, 64)` seen from the dead monster at `place`: the member exists, shares the world
/// and the layer, and its 3-D squared distance is below `64²`.
pub fn valid_area(member: &Mover, (world, layer, pos): (WorldId, LayerId, Vec3)) -> bool {
    !member.core.deleted
        && member.core.world == world
        && member.core.layer == layer
        && (member.core.pos - pos).length_squared() < AREA_SQ
}

/// `64 * 64`.
const AREA_SQ: f32 = 4096.0;

// ---- the text item -----------------------------------------------------------------------------------------------

/// `IK2_TEXT` (`MoverSkill.cpp:1467-1474`): using a quest book or scroll whose property names a quest
/// (`ItemProp.quest_id`) that has not been started with this very item (`text_quest_started`) starts that quest:
/// the text `TID_EVE_STARTQUEST` (when the quest has a property), the book entry and its `SetQuest`, the audit line
/// `quest.start`, and the item flag, which `UpdateItem` sends even when the book refused the quest (KEEP).
/// It works over `Fx` because the item rows have no `Services`; a timed quest that this zeroes is expired by
/// [`process`] one tick later. Returns `true` when it ran; the consumption of the item is the caller's.
pub fn on_text_item_used(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    now: GameMs,
    fx: &mut Fx,
) -> bool {
    let Some(item) = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .map(|it| (it.item_id, it.text_quest_started))
    else {
        return false;
    };
    let quest = env.data().item(item.0).map_or(0, |p| p.quest_id);
    if quest == 0 || item.1 {
        return false;
    }
    let data = env.data();
    if let Some(p) = data.quests.quests.get(quest) {
        fx.defined(TID_EVE_STARTQUEST, vec![format!("\"{}\"", p.title)]);
    }
    if let Some(r) = m
        .player_mut()
        .and_then(|p| p.quests.set(quest, QS_BEGIN, &data.quests, now))
    {
        fx.out.to_self(msg::set_quest(&r));
    }
    fx.audit(ev::QUEST_START, json!({ "quest": quest }));
    item_ops::update_item(m, env, id, UiParam::Ir, 1, 0, fx);
    true
}

// ---- the ctrl port -----------------------------------------------------------------------------------------------

/// The quest side of the ctrl open conditions (`UA_QUEST`, `UA_QUEST_END`; `User.cpp:7170-7357`) over the quest
/// book and the quest titles.
#[derive(Clone, Debug)]
pub struct QuestCtrl {
    data: Arc<GameData>,
}

impl QuestCtrl {
    /// The port over the loaded data.
    pub fn new(data: Arc<GameData>) -> QuestCtrl {
        QuestCtrl { data }
    }
}

impl CtrlQuestPort for QuestCtrl {
    fn state(&self, player: &Mover, quest: u32) -> Option<u32> {
        player
            .player()?
            .quests
            .find(quest)
            .map(|r| u32::from(r.state))
    }

    fn title(&self, quest: u32) -> Option<String> {
        self.data.quests.quests.get(quest).map(|p| p.title.clone())
    }

    /// `SetQuest(id, flag)` + `AddSetQuest` of an active quest (`UA_QUEST_END`): the state is stored as a `BYTE`,
    /// the counters reset, `SetQuest` goes to the player. No text, no audit line, no urgent save (original). A
    /// timed quest zeroed this way is expired by [`process`] at the next tick.
    fn set_state(&mut self, player: &mut Mover, quest: u32, state: u32, now: GameMs, fx: &mut Fx) {
        let Some(p) = player.player_mut() else { return };
        if let Some(r) = p.quests.set(quest, state as u8, &self.data.quests, now) {
            fx.out.to_self(msg::set_quest(&r));
        }
    }
}
