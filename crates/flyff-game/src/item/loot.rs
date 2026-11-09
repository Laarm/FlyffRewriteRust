//! Loot rights, pickup and the multiple item picker (Plan/19 §6.5-§6.8, WP-ITEM-07; F-090, F-197, KQ-37;
//! src:_Common/MoverActEvent.cpp:2251-2677, WORLDSERVER/User.cpp:7116-7165, _Common/MoverEquip.cpp:2338-2448).
//!
//! Pickup is the movement arrival at a ground item: the movement pipeline calls
//! [`ItemArrival::player_arrived_at_obj`](crate::movement::MoveConsumers) in the barrier of the frame, so
//! everything here works on the world driver's `FrameCtx` (object store, sink, grid), without `Services`. The
//! pickup of a whole ground stack never needs a fresh serial (the item keeps its own, 19 §2.7); a ground item
//! without a serial, or an overflow that needs more stacks, gets serial 0 and the item job numbers it before the
//! snapshot pass of the tick (`DeferSerials`).
//!
//! The party share of monster items and gold (receiver choice, cursor, split; WP-ITEM-23) is the `party`
//! submodule: it reads the picker's party link, the only party state the world phase can reach.

use super::container::DeferSerials;
use super::env::{ItemCore, ItemEnv};
use super::fx::Fx;
use super::model::Item;
use super::ops::{ItemError, ItemOrigin, core as ops};
use crate::movement::MoveConsumers;
use crate::mover::Mover;
use crate::world::phases::FrameCtx;
use crate::world::{ItemDrop, LinkSet};
use flyff_common::defs::itemkind::{IK1_GENERAL, IK1_GOLD, IK1_RIDE, IK1_SYSTEM, IK3_GOLD};
use flyff_common::defs::text::{TID_GAME_LACKSPACE, TID_GAME_PRIORITYITEMPER, TID_GAME_REAPITEM};
use flyff_common::{CharacterId, GameMs, ObjId, PartyId, UnixTime};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_db::AuditLog;
use flyff_db::audit::ev;
use flyff_rules::consts::ITEM_MODE;
use serde_json::json;
use std::sync::Arc;

/// An unowned drop becomes lootable by anyone after this long (`SEC(7)`, `__S_9_ADD`).
pub const LOOT_FREE_AFTER_MS: u64 = 7_000;
/// The multiple item picker's radius (`IsRangeObj(this, 30.0f)`).
pub const PICKER_RANGE: f32 = 30.0;

mod party;

/// Loot data of a ground item (`GroundItemMeta`, 19 §6.5, embedded unchanged by `ItemDrop.meta`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroundItemMeta {
    /// `m_idOwn`: the mover with loot priority (`None` = anyone).
    pub owner: Option<ObjId>,
    /// `m_idHolder`: quest items, only this character may take it, permanently.
    pub holder: Option<CharacterId>,
    /// `m_dwDropTime`: set at creation.
    pub drop_time: GameMs,
    /// `m_bDropMob`: party share and gold split apply.
    pub from_monster: bool,
    /// `m_IdEventMonster` (doc 22 loot time).
    pub event_monster: Option<u32>,
    /// Ground gold seed amount (19 §2.1).
    pub gold: Option<i32>,
    /// Set by the respawner as `gm_index > 0` (15 §15.5); pets never loot these.
    pub respawned: bool,
}

/// What the owner of a ground item is now (`prj.GetMover(m_idOwn)` + `IsValidObj`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerState {
    /// The item has no owner: anyone may take it.
    NoOwner,
    /// The owner is gone: anyone may take it.
    Gone,
    /// The owner is in the world, in this party (if any).
    Present {
        /// `m_idparty` of the owner.
        party: Option<PartyId>,
    },
}

/// The loot reservation of an event monster (`EventMonster.lua`, doc 22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventLootRule {
    /// `nLootTime` in seconds.
    pub loot_time_s: u32,
    /// `bPet`: pets may pick the drops up.
    pub pet_able: bool,
}

/// The event-monster rule of a ground item (`None` for an ordinary drop).
pub fn event_rule(data: &GameData, meta: &GroundItemMeta) -> Option<EventLootRule> {
    let id = meta.event_monster?;
    // the single lookup of the event monsters (the first entry of EventMonster.lua wins; WP-ITEM-25)
    crate::event::monsters::find(&data.lua.event_monsters, id).map(|e| EventLootRule {
        loot_time_s: u32::try_from(e.loot_time_s).unwrap_or(0),
        pet_able: e.pet,
    })
}

/// The facts [`is_loot`] decides on.
#[derive(Clone, Copy, Debug)]
pub struct LootQuery {
    /// The one who wants the item.
    pub picker: ObjId,
    /// Its character id.
    pub picker_character: CharacterId,
    /// Its party.
    pub picker_party: Option<PartyId>,
    /// The state of the item's owner.
    pub owner: OwnerState,
    /// The game time.
    pub now: GameMs,
    /// The event-monster rule, if the drop is an event monster's.
    pub event: Option<EventLootRule>,
    /// A pet loots on behalf of the picker.
    pub by_pet: bool,
    /// Only for pets: the item (not gold) does not fit into the inventory.
    pub pet_inventory_full: bool,
}

/// `IsLoot(item, bPet)` (src:_Common/MoverActEvent.cpp:2251-2313, Plan/19 §6.6).
pub fn is_loot(q: &LootQuery, meta: &GroundItemMeta, deleted: bool) -> bool {
    if deleted || meta.holder.is_some_and(|h| h != q.picker_character) {
        return false;
    }
    let mut take = match (meta.owner, q.owner) {
        (None, _) | (_, OwnerState::NoOwner) => true,
        (Some(owner), state) => {
            let mut t = match state {
                OwnerState::Gone | OwnerState::NoOwner => true,
                OwnerState::Present { party } => {
                    q.picker_party.is_some() && party == q.picker_party
                }
            };
            if owner == q.picker {
                t = true;
            }
            t
        }
    };
    if let Some(rule) = q.event {
        if q.now.0.saturating_sub(meta.drop_time.0) >= u64::from(rule.loot_time_s) * 1000 {
            take = true;
        }
        if q.by_pet && !rule.pet_able {
            take = false;
        }
    } else if q.now.0.saturating_sub(meta.drop_time.0) >= LOOT_FREE_AFTER_MS {
        take = true;
    }
    if q.by_pet && take {
        if meta.respawned {
            take = false;
        }
        if q.pet_inventory_full {
            take = false;
        }
    }
    take
}

/// How a pickup ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LootResult {
    /// The item (or the gold) went to the picker; the ground item is to be deleted.
    Looted,
    /// `TID_GAME_LACKSPACE` was sent; the item stays on the ground.
    LackSpace,
    /// The ground item is deleted or invalid.
    Gone,
}

/// `IsLootSendText(prop)`: the pickup line is also copied into the chat for every kind but gold, general,
/// system and ride items.
pub fn is_loot_send_text(prop: &ItemProp) -> bool {
    !matches!(
        prop.item_kind1,
        IK1_GOLD | IK1_GENERAL | IK1_SYSTEM | IK1_RIDE
    )
}

/// The call environment of the world phase. `FrameCtx` has the game clock but no wall clock, and nothing on the
/// pickup path reads wall time (keep times travel with the record; only `UI_KEEPTIME` computes one), so the
/// wall time of this environment is 0.
fn pickup_env(core: &ItemCore) -> ItemEnv<'_> {
    core.env(UnixTime(0))
}

/// The picker's side of [`LootQuery`].
fn picker_facts(m: &Mover) -> Option<(CharacterId, Option<PartyId>)> {
    let p = m.player()?;
    Some((p.ident.character_id, p.social.party))
}

/// The gold amount of a ground gold seed (`CItemElem::GetGold`).
fn ground_gold(d: &ItemDrop) -> i64 {
    i64::from(d.meta.gold.unwrap_or(d.item.endurance))
}

/// `DoLoot(item)` for one picker: gold goes to the wallet (with the Perin converter and the gold text), anything
/// else into the inventory. The solo path of the original (`SubLootDropMobSingle` / `SubLootDropNotMob`):
/// the item keeps its serial, success sends `TID_GAME_REAPITEM` (plus the chat copy), a full inventory
/// `TID_GAME_LACKSPACE`.
pub fn do_loot(m: &mut Mover, env: &ItemEnv<'_>, drop: &ItemDrop, fx: &mut Fx) -> LootResult {
    let Some(prop) = env.data().item(drop.item.item_id) else {
        return LootResult::Gone;
    };
    if prop.item_kind1 == IK1_GOLD {
        let amount = ground_gold(drop);
        if amount <= 0 {
            tracing::error!(amount, "loot: a ground gold seed without gold");
            return LootResult::Looted;
        }
        super::gold::core::pickup_gold_core(
            m,
            env,
            &mut DeferSerials,
            amount,
            drop.meta.from_monster,
            fx,
        );
        return LootResult::Looted;
    }
    let item = Item::from_record(&drop.item);
    let count = u32::from(item.count);
    match ops::create_item(m, env, &mut DeferSerials, item, ItemOrigin::Pickup, fx) {
        Ok(_) => {
            let quoted = vec![format!("\"{}\"", prop.name)];
            fx.defined(TID_GAME_REAPITEM, quoted.clone());
            if is_loot_send_text(prop) {
                fx.defined_chat(TID_GAME_REAPITEM, quoted);
            }
            audit_pickup(fx, drop, count);
            LootResult::Looted
        }
        Err(ItemError::LackSpace) => {
            fx.text(TID_GAME_LACKSPACE);
            LootResult::LackSpace
        }
        Err(ItemError::NoProp) => LootResult::Gone,
        Err(e) => {
            tracing::warn!(error = %e, "loot: the item could not be created");
            LootResult::LackSpace
        }
    }
}

/// The `item.pickup` audit line (19 §21): every non-consumable pickup and consumable stacks above 9.
fn audit_pickup(fx: &mut Fx, drop: &ItemDrop, count: u32) {
    fx.audit(
        ev::ITEM_PICKUP,
        json!({
            "item": super::fx::item_json(&Item::from_record(&drop.item)),
            "count": count,
            "from_monster": drop.meta.from_monster,
        }),
    );
}

/// The facts of one pickup attempt read from the object store.
struct Attempt {
    drop: ItemDrop,
    query: LootQuery,
}

/// Reads the ground item and the picker's loot facts (`None` when either is gone or not a player).
fn attempt(
    ctx: &FrameCtx<'_>,
    env: &ItemEnv<'_>,
    picker: ObjId,
    target: ObjId,
    by_pet: bool,
) -> Option<Attempt> {
    let drop = ctx.store.item(target)?.clone();
    let m = ctx.store.mover(picker)?;
    let (character, party) = picker_facts(m)?;
    let owner = match drop.meta.owner {
        None => OwnerState::NoOwner,
        Some(o) => match ctx.store.mover(o) {
            Some(om) if !om.core.deleted => OwnerState::Present {
                party: om.player().and_then(|p| p.social.party),
            },
            _ => OwnerState::Gone,
        },
    };
    let pet_inventory_full = by_pet
        && env.data().item(drop.item.item_id).is_some_and(|prop| {
            prop.item_kind3 != IK3_GOLD
                && prop.item_kind1 != IK1_GOLD
                && m.player().is_some_and(|p| {
                    p.items.inventory.is_full(
                        &Item::from_record(&drop.item),
                        prop.pack_max.max(1),
                        u32::from(drop.item.count),
                    )
                })
        });
    Some(Attempt {
        query: LootQuery {
            picker,
            picker_character: character,
            picker_party: party,
            owner,
            now: ctx.now,
            event: event_rule(env.data(), &drop.meta),
            by_pet,
            pet_inventory_full,
        },
        drop,
    })
}

/// Flushes the effects of a pickup through the sink.
fn flush(ctx: &mut FrameCtx<'_>, picker: ObjId, fx: Fx, audit: &AuditLog) {
    if let Some(m) = ctx.store.mover(picker) {
        fx.flush_sink(&mut *ctx.sink, m, audit);
    }
}

/// `CUser::OnMsgArrival` for a ground item (Plan/19 §6.7): flying players and `ITEM_MODE` ignore the item;
/// a foreign reserved item gives `TID_GAME_PRIORITYITEMPER`; otherwise `do_loot`, the pickup motion and (with
/// `features.multiple_item_picker`) the loot of every identical stackable within 30 m **without** a loot-right
/// check (KQ-37).
pub fn on_arrival(
    core: &ItemCore,
    audit: &AuditLog,
    ctx: &mut FrameCtx<'_>,
    picker: ObjId,
    target: ObjId,
) {
    let env = pickup_env(core);
    let Some(m) = ctx.store.mover(picker) else {
        return;
    };
    if !m.is_player() || m.sim.act.flags.fly() || m.mv.mode & ITEM_MODE != 0 {
        return;
    }
    let Some(a) = attempt(ctx, &env, picker, target, false) else {
        return;
    };
    let mut fx = Fx::new();
    let name = env
        .data()
        .item(a.drop.item.item_id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    if !is_loot(&a.query, &a.drop.meta, a.drop.core.deleted) {
        fx.defined(TID_GAME_PRIORITYITEMPER, vec![format!("\"{name}\"")]);
        flush(ctx, picker, fx, audit);
        return;
    }
    let result = match party::do_loot_shared(&env, ctx, picker, &a.drop, &mut fx, audit) {
        Some(r) => r,
        None => match ctx.store.mover_mut(picker) {
            Some(m) => do_loot(m, &env, &a.drop, &mut fx),
            None => return,
        },
    };
    match result {
        LootResult::Looted => {
            ctx.delete(target);
            if let Some(m) = ctx.store.mover_mut(picker) {
                crate::movement::intents::broadcast_motion(m, flyff_sim::types::OBJMSG_PICKUP);
            }
            if core.cfg.multiple_item_picker {
                pick_nearby(
                    core,
                    audit,
                    ctx,
                    picker,
                    a.drop.item.item_id,
                    target,
                    &mut fx,
                );
            }
        }
        LootResult::Gone => ctx.delete(target),
        LootResult::LackSpace => {}
    }
    flush(ctx, picker, fx, audit);
}

/// The multiple item picker (KQ-37, 19 §6.7 step 3): every other ground item of the same id within
/// [`PICKER_RANGE`] of the `centre` object that is stackable (or gold) is looted without a loot-right check.
/// Returns the number of ground items looted.
fn pick_nearby(
    core: &ItemCore,
    audit: &AuditLog,
    ctx: &mut FrameCtx<'_>,
    picker: ObjId,
    item_id: u32,
    first: ObjId,
    fx: &mut Fx,
) -> usize {
    let env = pickup_env(core);
    let centre = picker;
    nearby_loot(&env, audit, ctx, picker, centre, item_id, first, fx)
}

/// [`pick_nearby`] around any `centre` (the pet, for a pet loot).
#[allow(clippy::too_many_arguments)]
fn nearby_loot(
    env: &ItemEnv<'_>,
    audit: &AuditLog,
    ctx: &mut FrameCtx<'_>,
    picker: ObjId,
    centre: ObjId,
    item_id: u32,
    first: ObjId,
    fx: &mut Fx,
) -> usize {
    let Some(c) = ctx.store.core(centre) else {
        return 0;
    };
    let pos = c.pos;
    let mpu = ctx.grid.geom().mpu.max(1) as f32;
    let n_range = ((PICKER_RANGE / mpu).ceil() as i32).saturating_add(1);
    let mut cands: Vec<ObjId> = Vec::new();
    ctx.grid
        .for_each_legacy(pos, n_range, LinkSet::Others, |id| cands.push(id));
    let mut looted = 0;
    for id in cands {
        if id == first {
            continue;
        }
        let Some(d) = ctx.store.item(id) else {
            continue;
        };
        if d.core.deleted || d.item.item_id != item_id {
            continue;
        }
        let diff = d.core.pos - pos;
        if diff.x * diff.x + diff.y * diff.y + diff.z * diff.z >= PICKER_RANGE * PICKER_RANGE {
            continue;
        }
        let Some(prop) = env.data().item(d.item.item_id) else {
            continue;
        };
        if !(prop.pack_max > 1 || prop.item_kind3 == IK3_GOLD) {
            continue;
        }
        let drop = d.clone();
        let res = match party::do_loot_shared(env, ctx, picker, &drop, fx, audit) {
            Some(r) => r,
            None => {
                let Some(m) = ctx.store.mover_mut(picker) else {
                    break;
                };
                do_loot(m, env, &drop, fx)
            }
        };
        match res {
            LootResult::Looted => {
                ctx.delete(id);
                looted += 1;
            }
            LootResult::Gone => ctx.delete(id),
            LootResult::LackSpace => {}
        }
    }
    looted
}

/// `pet_can_loot(owner, item)` (19 §6.8): the owner may loot the item on behalf of the pet (`is_loot` with
/// `by_pet`), the item has a prop (the scan of the original skips what it cannot describe) and the owner's pet
/// filter (`PlayerItems.pet_filter`, with `features.pet_filter`) does not skip it.
pub fn pet_can_loot(core: &ItemCore, ctx: &FrameCtx<'_>, owner: ObjId, item: ObjId) -> bool {
    let env = pickup_env(core);
    let Some(a) = attempt(ctx, &env, owner, item, true) else {
        return false;
    };
    let Some(prop) = env.data().item(a.drop.item.item_id) else {
        return false;
    };
    let filter = if core.cfg.pet_filter {
        ctx.store
            .mover(owner)
            .and_then(|m| m.player())
            .map_or(0, |p| p.items.pet_filter)
    } else {
        0
    };
    is_loot(&a.query, &a.drop.meta, a.drop.core.deleted)
        && super::pets::filter::pet_filter_allows(filter, prop)
}

/// `pet_arrived(owner, pet, item)` (19 §6.8): the pet reached the ground item. The owner loots it (`do_loot`
/// on his behalf); when that succeeds the multiple picker runs within 30 m **of the pet**. A deleted item
/// answers [`LootResult::Gone`], a full inventory [`LootResult::LackSpace`] (no picker).
pub fn pet_arrived(
    core: &ItemCore,
    audit: &AuditLog,
    ctx: &mut FrameCtx<'_>,
    owner: ObjId,
    pet: ObjId,
    item: ObjId,
) -> LootResult {
    let env = pickup_env(core);
    let Some(a) = attempt(ctx, &env, owner, item, false) else {
        return LootResult::Gone;
    };
    if a.drop.core.deleted {
        return LootResult::Gone;
    }
    let mut fx = Fx::new();
    let result = match party::do_loot_shared(&env, ctx, owner, &a.drop, &mut fx, audit) {
        Some(r) => r,
        None => match ctx.store.mover_mut(owner) {
            Some(m) => do_loot(m, &env, &a.drop, &mut fx),
            None => return LootResult::Gone,
        },
    };
    match result {
        LootResult::Looted => {
            ctx.delete(item);
            if core.cfg.multiple_item_picker {
                nearby_loot(
                    &env,
                    audit,
                    ctx,
                    owner,
                    pet,
                    a.drop.item.item_id,
                    item,
                    &mut fx,
                );
            }
        }
        LootResult::Gone => ctx.delete(item),
        LootResult::LackSpace => {}
    }
    flush(ctx, owner, fx, audit);
    result
}

/// The pickup hook of the world phase: installed with `MovementHooks::with_consumers`.
#[derive(Debug)]
pub struct ItemArrival {
    core: Arc<ItemCore>,
    audit: AuditLog,
}

impl ItemArrival {
    /// A pickup consumer over the shared item environment and the audit writer.
    pub fn new(core: Arc<ItemCore>, audit: AuditLog) -> ItemArrival {
        ItemArrival { core, audit }
    }
}

impl MoveConsumers for ItemArrival {
    fn player_arrived_at_obj(&mut self, ctx: &mut FrameCtx<'_>, player: ObjId, target: ObjId) {
        on_arrival(&self.core, &self.audit, ctx, player, target);
    }
}
