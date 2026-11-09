//! What a quest does when it starts and when it ends (Plan/20 §9.6-§9.7, WP-NPC-14): the start effects
//! ([`run_quest`], `__RunQuest`), the reward sequence ([`end_quest`], `__EndQuest`) and the item and gold
//! primitives both use ([`create_quest_item`], [`add_gold_with_text`], [`remove_gold_clamped`]: the one
//! implementation behind the script helpers `CreateItem`, `AddGold` and `RemoveGold` too).
//!
//! Rewards are ordinary single-player mutations: no `AtomicBatch` (no second party is involved). The urgent save
//! that [`ops::set_quest_state`] asks for writes the items, the gold and the quest
//! document in ONE character snapshot. Every draw uses the player's world stream; the draws are small pure
//! helpers over `&mut dyn GameRng` ([`roll_amount`], [`pk_reduction`]) so the tests can script them.

use super::ops::{self, RemoveReason};
use super::{PlayerSubject, QS_END};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::pets::system::PetCause;
use crate::item::pets::system::exp::level_up;
use crate::item::pets::system::hooks::with_player;
use crate::item::{self, Item, ItemFlags, ItemOrigin};
use crate::stats::Out;
use crate::stats::progression::{self, ExpGrant};
use crate::world::ReplaceKind;
use flyff_common::defs::text::{TID_EVE_REAPITEM, TID_GAME_REAPEXP, TID_QUEST_NOINVENTORYSPACE};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::quest::{QuestProp, QuestPropItem};
use flyff_db::audit::ev;
use serde_json::json;
use std::sync::Arc;

/// How an [`end_quest`] call ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndResult {
    /// The rewards were given and the quest is at state 14.
    Done,
    /// The end condition is not met (nothing happened, except the timer freeze of a true predicate).
    NotDone,
    /// The bag cannot hold the reward items (`TID_QUEST_NOINVENTORYSPACE` was sent, nothing was given).
    NoSpace,
    /// The quest has no property.
    NoQuest,
}

// ---- items and gold ----------------------------------------------------------------------------------------------

/// `__CreateItem(pc, item, n, ability_option, flag)` (`ScriptHelper.cpp:924-955`): nothing for an item without a
/// property; otherwise `TID_EVE_REAPITEM "\"<name>\""` goes out FIRST (KEEP-QUIRK: even when nothing is created),
/// then `n` (clamped to `1..=65535`; `n <= 0` creates nothing) pieces are created through the item system with
/// the origin `Quest` (it audits `item.create{source: quest}`). A full inventory creates nothing and says nothing
/// (KEEP-QUIRK, KQ-38).
pub fn create_quest_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    item_id: u32,
    n: i64,
    ability_option: i32,
    flag: u8,
) {
    let data = Arc::clone(ctx.data);
    let Some(prop) = data.item(item_id) else {
        return;
    };
    notice::defined(
        ctx,
        who,
        TID_EVE_REAPITEM,
        vec![format!("\"{}\"", prop.name)],
    );
    let count = u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0);
    if count == 0 {
        return;
    }
    let piece = Item {
        upgrade_level: ability_option,
        flags: ItemFlags(flag),
        ..Item::new(item_id, count)
    };
    // `LackSpace` is not reported
    let _ = item::ops::create_item(svc, ctx, who, piece, ItemOrigin::Quest);
}

/// `__AddGold(pc, n)`: the gold rules of doc 19 plus the gain text (`AddGoldText`). `n == 0` does nothing.
pub fn add_gold_with_text(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, n: i64) {
    if n == 0 {
        return;
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = item::Fx::new();
    {
        let mut serials = item::ops::CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
        if let Some(m) = ctx.worlds.player_mut(who.0) {
            item::gold::core::add_gold(m, &env, n, true, &mut fx);
            item::gold::core::gold_text(m, &env, &mut serials, n, &mut fx);
        }
    }
    fx.flush(svc, ctx, who);
}

/// `__RemoveGold(pc, n)`: subtracts `n`; with less than `n` the gold becomes 0 (KEEP-QUIRK clamp, KQ-38).
pub fn remove_gold_clamped(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, n: i64) {
    let gold = ctx
        .player(who)
        .and_then(|m| m.player())
        .map_or(0, |p| i64::from(p.items.gold));
    let take = n.clamp(0, gold);
    if take > 0 {
        item::gold::add_gold(svc, ctx, who, -take, true);
    }
}

// ---- the start ---------------------------------------------------------------------------------------------------

/// `__RunQuest(pc, npc, q)` (`ScriptHelper.cpp:211-241`; the flows' `QUEST_BEGIN_YES` and the `BeginQuest`
/// helper): without a property nothing at all; otherwise the begin items (`create_quest_item`, slot order), the
/// begin gold, the begin disguise (`Disguise` to the near set), then
/// [`ops::set_quest_started`] and an urgent save (§9.13). There is no condition check: `BeginQuest` of a
/// completed quest gives the items again while the book refuses the set (KEEP).
pub fn run_quest(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, _npc: ObjId, q: u32) {
    let data = Arc::clone(ctx.data);
    let Some(prop) = data.quests.quests.get(q) else {
        return;
    };
    for slot in &prop.begin_set_add_items {
        if slot.item != 0 {
            create_quest_item(svc, ctx, who, slot.item, i64::from(slot.num), 0, 0);
        }
    }
    if prop.begin_set_add_gold != 0 {
        add_gold_with_text(svc, ctx, who, i64::from(prop.begin_set_add_gold));
    }
    if prop.begin_set_disguise_mover_index != 0 {
        let mut out = Out::new();
        if let Some(m) = ctx.worlds.player_mut(who.0) {
            crate::stats::fashion::set_disguise(
                m,
                prop.begin_set_disguise_mover_index as u32,
                &mut out,
            );
        }
        out.flush(ctx, who.0);
    }
    ops::set_quest_started(svc, ctx, who, q);
    svc.saves.mark_urgent(who.0);
}

// ---- the draws -----------------------------------------------------------------------------------------------------

/// `min + xRandom(max - min + 1)` (the gold and EXP rewards), in `i64`. `max == min` still draws once
/// (`random_below(1)`, as `xRandom(1)`); with `max < min` the result is `min` and nothing is drawn (FIX: the
/// original's `xRandom` of a non-positive range, invisible with sane data).
pub fn roll_amount(rng: &mut dyn GameRng, min: i32, max: i32) -> i64 {
    if max < min {
        return i64::from(min);
    }
    let span = i64::from(max) - i64::from(min) + 1;
    let draw = rng.random_below(u32::try_from(span).unwrap_or(u32::MAX));
    i64::from(min) + i64::from(draw)
}

/// The PK reward `xRandom(min, max + 1)` (`[min, max]`): `None` when the pair is skipped (`min > max`, or a
/// negative bound: FIX, the original cast it to `DWORD` and could RAISE the PK value). Nothing is drawn then.
pub fn pk_reduction(rng: &mut dyn GameRng, min: i32, max: i32) -> Option<u32> {
    if (min == 0 && max == 0) || min > max {
        return None;
    }
    let (Ok(lo), Ok(hi)) = (u32::try_from(min), u32::try_from(max)) else {
        tracing::warn!(
            min,
            max,
            "a quest PK reward with a negative bound is skipped"
        );
        return None;
    };
    Some(rng.random_range(lo, hi.saturating_add(1)))
}

// ---- the end -----------------------------------------------------------------------------------------------------

/// Whether a reward entry applies to a player (`__EndQuest`'s two loops): the sex is -1 or the player's; `kind` 0
/// with a job of -1 or the player's; `kind` 1 with an item of -1 or one the player owns; any other `kind` never.
/// `owned(item)` is `GetItemNum` (KQ-27).
pub fn reward_applies(e: &QuestPropItem, sex: i32, job: i32, owned: &dyn Fn(u32) -> u32) -> bool {
    if e.sex != -1 && i32::from(e.sex) != sex {
        return false;
    }
    match e.kind {
        0 => e.job_or_item == -1 || e.job_or_item == job,
        1 => e.job_or_item == -1 || u32::try_from(e.job_or_item).is_ok_and(|id| owned(id) != 0),
        _ => false,
    }
}

/// The slots one reward entry needs: `ceil(num / pack_max)` (`pack_max == 0` counts as 1: FIX, the original
/// divided by zero; `num <= 0` needs none).
pub fn entry_slots(num: i32, pack_max: u32) -> usize {
    let pack = u64::from(pack_max.max(1));
    u64::try_from(num)
        .map_or(0, |n| n.div_ceil(pack))
        .try_into()
        .unwrap_or(usize::MAX)
}

/// The reward items that apply to the player, in entry order, with the bag slots they need in total
/// (`reward_slots`). An entry without an item (`item == 0`) is not a reward; an entry whose item property is
/// unknown counts 0 and is left out of the list, with one warning (FIX: the original dereferenced null).
pub fn reward_slots<'a>(
    data: &GameData,
    prop: &'a QuestProp,
    sex: i32,
    job: i32,
    owned: &dyn Fn(u32) -> u32,
) -> (usize, Vec<&'a QuestPropItem>) {
    let mut slots = 0_usize;
    let mut list = Vec::new();
    for e in &prop.end_reward_items {
        if e.item == 0 || !reward_applies(e, sex, job, owned) {
            continue;
        }
        let Some(item) = data.item(e.item) else {
            tracing::warn!(
                quest = prop.id,
                item = e.item,
                "a quest reward names an unknown item: skipped"
            );
            continue;
        };
        slots = slots.saturating_add(entry_slots(e.num, item.pack_max));
        list.push(e);
    }
    (slots, list)
}

/// `__EndQuest(pc, q, check_cond)` (`ScriptHelper.cpp:243-465`) in the exact order of Plan/20 §9.7: the end
/// condition (when `check_cond`: a true predicate freezes the timer, KEEP), the space check, the reward items, the
/// pet level-up, gold, EXP, skill points, PK, teleport, the end of the begin disguise, campus points, the removals
/// (items, gold, other quests) and finally state 14 through [`ops::set_quest_state`].
pub fn end_quest(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    q: u32,
    check_cond: bool,
) -> EndResult {
    let data = Arc::clone(ctx.data);
    let Some(prop) = data.quests.quests.get(q) else {
        return EndResult::NoQuest;
    };
    // 1. the end condition (its true result freezes a running timer)
    if check_cond {
        let preds = svc.quest.predicates();
        let ok = PlayerSubject::new(&*svc, &*ctx, who.0).is_some_and(|s| preds.can_end(&s, q));
        if !ok {
            return EndResult::NotDone;
        }
        if let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
            p.quests.freeze_timer(q);
        }
    }
    // 2. the space check
    let (slots, entries, empty) = {
        let Some(m) = ctx.player(who) else {
            return EndResult::NotDone;
        };
        let Some(p) = m.player() else {
            return EndResult::NotDone;
        };
        let owned = |id: u32| p.items.quest_item_count(id);
        let (slots, list) =
            reward_slots(&data, prop, i32::from(p.appearance.sex), m.mv.job, &owned);
        let entries: Vec<QuestPropItem> = list.into_iter().copied().collect();
        (slots, entries, p.items.inventory.empty_count())
    };
    if slots > empty {
        notice::defined(ctx, who, TID_QUEST_NOINVENTORYSPACE, Vec::new());
        return EndResult::NoSpace;
    }
    let Some(world) = ctx.worlds.core(who.0).map(|c| c.world) else {
        return EndResult::NotDone;
    };
    // 3. the reward items, in entry order
    for e in &entries {
        create_quest_item(
            svc,
            ctx,
            who,
            e.item,
            i64::from(e.num),
            e.ability_option,
            e.flag,
        );
    }
    // 4. the pet level-up
    if prop.end_reward_pet_levelup {
        with_player(svc, ctx, who, |m, cx, rng, fx| {
            level_up(m, cx, rng, PetCause::Quest, fx);
        });
    }
    // 5. gold
    if prop.end_reward_gold_min != 0 {
        let gold = roll_amount(
            ctx.rng.world(world),
            prop.end_reward_gold_min,
            prop.end_reward_gold_max,
        );
        add_gold_with_text(svc, ctx, who, gold);
    }
    // 6. experience (SetExperience and the level-up messages come from the grant), then the text without argument
    if prop.end_reward_exp_min != 0 {
        let exp = roll_amount(
            ctx.rng.world(world),
            prop.end_reward_exp_min,
            prop.end_reward_exp_max,
        );
        progression::grant_experience(svc, ctx, who, ExpGrant::reward(exp, false), Out::new());
        notice::defined(ctx, who, TID_GAME_REAPEXP, Vec::new());
    }
    // 7. skill points
    if prop.end_reward_skill_point != 0 {
        skill_points(svc, ctx, who, q, prop.end_reward_skill_point);
    }
    // 8. PK
    if let Some(cut) = pk_reduction(
        ctx.rng.world(world),
        prop.end_reward_pk_value_min,
        prop.end_reward_pk_value_max,
    ) {
        pk_reward(svc, ctx, who, q, cut);
    }
    // 9. teleport (layer 0)
    if prop.end_reward_teleport != 0 {
        let [x, y, z] = prop.end_reward_teleport_pos;
        if let Err(e) = ctx.worlds.replace(
            who.0,
            WorldId(prop.end_reward_teleport),
            Vec3::new(x, y, z),
            LayerId(0),
            ReplaceKind::Normal,
        ) {
            tracing::warn!(quest = q, error = %e, "the quest reward teleport was refused");
        }
    }
    // 10. the begin disguise ends
    if prop.begin_set_disguise_mover_index != 0 {
        ops::undo_disguise(ctx, who);
    }
    // 11. campus points: the reward, then the removal (two commits, as the original's two DB messages)
    campus(svc, ctx, who, prop.end_reward_tsp, prop.end_remove_tsp);
    // 12. removals
    for slot in &prop.end_remove_items {
        if slot.item != 0
            && let Ok(n) = u32::try_from(slot.num)
            && n > 0
        {
            item::ops::remove_item_by_id(
                svc,
                ctx,
                who,
                slot.item,
                Some(n),
                item::ops::RemoveReason::Other("quest"),
            );
        }
    }
    if prop.end_remove_gold != 0 {
        remove_gold_clamped(svc, ctx, who, i64::from(prop.end_remove_gold));
    }
    // 13. other quests
    for other in prop.end_remove_quests {
        if let Ok(other) = u32::try_from(other)
            && other != 0
        {
            ops::remove_quest(svc, ctx, who, other, RemoveReason::Reward);
        }
    }
    // 14. the end
    ops::set_quest_state(svc, ctx, who, q, QS_END);
    EndResult::Done
}

/// Step 7: `AddSkillPoint`, `SetExperience`, and the audit line `skill.points_gain{source: "quest"}`.
fn skill_points(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, q: u32, n: i32) {
    let core = svc.stats.handle();
    let mut out = Out::new();
    let granted = ctx.worlds.player_mut(who.0).map_or(0, |m| {
        let granted = progression::add_skill_points(m, &core, n);
        progression::emit_experience(m, &mut out);
        granted
    });
    out.flush(ctx, who.0);
    crate::audit::emit_for(
        ctx,
        who,
        ev::SKILL_POINTS_GAIN,
        json!({ "source": "quest", "quest": q, "n": granted }),
    );
}

/// Step 8: the PK value falls by `cut` (not below 0), `PkRelation{PkValue}` goes to the player, audit
/// `char.pk{source: "quest"}`, and the honor titles are re-checked.
fn pk_reward(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, q: u32, cut: u32) {
    let value = {
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return;
        };
        let current = m.player().map_or(0, |p| i64::from(p.pk.value));
        let next = (current - i64::from(cut)).clamp(0, i64::from(i32::MAX)) as i32;
        crate::combat::pvp::pk::set_pk_value(m, next);
        next
    };
    let mut out = Out::new();
    out.to_self(crate::combat::pvp::pk::pk_value_relation(value));
    out.flush(ctx, who.0);
    crate::audit::emit_for(
        ctx,
        who,
        ev::CHAR_PK,
        json!({ "source": "quest", "quest": q, "pk_value": value }),
    );
    crate::social::hooks::honor_recheck(svc, ctx, who, true);
}

/// Step 11: `SendUpdateCampusPoint(.., 'Q')` for the reward and, separately, for the removal.
fn campus(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, reward: i32, remove: i32) {
    use crate::social::campus::points::{PointReason, add_points};
    if reward == 0 && remove == 0 {
        return;
    }
    let Some(character) = crate::social::relation::character_of(ctx, who) else {
        return;
    };
    if reward != 0 {
        add_points(svc, ctx, character, reward, PointReason::Quest);
    }
    if remove != 0 {
        add_points(
            svc,
            ctx,
            character,
            remove.saturating_neg(),
            PointReason::Quest,
        );
    }
}
