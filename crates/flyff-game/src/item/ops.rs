//! Item lifecycle primitives (Plan/19 §4.1, WP-ITEM-03; ref/13 §7; src:_Common/Mover.cpp:2355-2410,
//! MoverParam.cpp:3693-4142): `create_item`, `remove_item`, `update_item` and their messages.
//!
//! These are the **only** functions that mutate a player's inventory outside a commit block. Two layers:
//!
//! * the mover-level functions of [`core`] take `&mut Mover`, the [`ItemEnv`], a [`SerialSource`] and an
//!   [`Fx`]; they work from handlers, timers and the world phase alike;
//! * the `GameCtx` functions of this module ([`create_item`], [`remove_item`], [`remove_item_by_id`],
//!   [`update_item`], [`update_item_ex`]) are the API of Plan/19 §4.1 for the other documents: they resolve the
//!   player, allocate serials from `Services.serials`, run the core function and flush its effects.

use super::container::{AddError, AddPlan, SerialSource, StorageId};
use super::env::ItemEnv;
use super::fx::{Fx, audit_create, item_json};
use super::model::{
    Element, Item, MAX_COLLECTOR_BATTERY, MAX_PIERCING_ULTIMATE, MAX_PIERCING_WEAPON, MAX_VIS,
};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::persist::serial::{GamePersist, SerialError};
use crate::stats::equip::{reapply_part, unequip_for_removal, update_param};
use flyff_common::defs::item::{II_GEN_TOO_COL_EVENT, II_GEN_TOO_COL_NORMAL};
use flyff_common::defs::itemkind::IK2_TEXT;
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::item::random_option::RandomOptWord;
use serde_json::json;

/// Why an item could not be created.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ItemError {
    /// The item id is 0 or has no prop.
    #[error("unknown item")]
    NoProp,
    /// No room in the inventory (`TID_GAME_LACKSPACE`); nothing changed.
    #[error("no space in the inventory")]
    LackSpace,
    /// The item is reserved (trade, vendor).
    #[error("the item is reserved")]
    Reserved,
    /// The item does not exist.
    #[error("no such item")]
    NotFound,
    /// A fresh serial was needed and none could be allocated.
    #[error(transparent)]
    Serial(#[from] SerialError),
}

/// Where an item came from (audit and serial handling, 19 §2.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemOrigin {
    /// A monster or a player drop.
    Drop,
    /// Picked up from the ground (a move: the serial is kept).
    Pickup,
    /// Bought from an NPC shop.
    ShopBuy,
    /// Bought with Red Chips.
    ChipShop,
    /// Bought from a personal vendor.
    VendorBuy,
    /// Received in a trade.
    Trade,
    /// Taken from a mail.
    MailTake,
    /// Taken from a bank.
    BankGet,
    /// Moved between the bag and a pocket.
    PocketMove,
    /// Opened from a pack item.
    Pack,
    /// Opened from a gift box.
    GiftBox,
    /// An exchange script.
    Exchange,
    /// The result of an upgrade.
    Upgrade,
    /// An ultimate-weapon product.
    Ultimate,
    /// A transy product.
    Transy,
    /// Pet feeding.
    PetFeed,
    /// Egg transform.
    PetTransform,
    /// Collecting.
    Collect,
    /// Angel creation.
    Angel,
    /// A quest reward.
    Quest,
    /// An event reward.
    Event,
    /// A level-up gift.
    LevelUpGift,
    /// A pending grant.
    Grant,
    /// A GM command.
    Gm,
    /// The Perin converter.
    Perin,
    /// A split stack.
    Split,
    /// A refund of a summon-monster item.
    CreateMonsterRefund,
    /// Anything else, with its own audit tag.
    Other(&'static str),
}

impl ItemOrigin {
    /// The snake_case source tag of the audit line.
    pub fn name(self) -> &'static str {
        match self {
            ItemOrigin::Drop => "drop",
            ItemOrigin::Pickup => "pickup",
            ItemOrigin::ShopBuy => "shop_buy",
            ItemOrigin::ChipShop => "chip_shop",
            ItemOrigin::VendorBuy => "vendor_buy",
            ItemOrigin::Trade => "trade",
            ItemOrigin::MailTake => "mail_take",
            ItemOrigin::BankGet => "bank_get",
            ItemOrigin::PocketMove => "pocket_move",
            ItemOrigin::Pack => "pack",
            ItemOrigin::GiftBox => "gift_box",
            ItemOrigin::Exchange => "exchange",
            ItemOrigin::Upgrade => "upgrade",
            ItemOrigin::Ultimate => "ultimate",
            ItemOrigin::Transy => "transy",
            ItemOrigin::PetFeed => "pet_feed",
            ItemOrigin::PetTransform => "pet_transform",
            ItemOrigin::Collect => "collect",
            ItemOrigin::Angel => "angel",
            ItemOrigin::Quest => "quest",
            ItemOrigin::Event => "event",
            ItemOrigin::LevelUpGift => "level_up_gift",
            ItemOrigin::Grant => "grant",
            ItemOrigin::Gm => "gm",
            ItemOrigin::Perin => "perin",
            ItemOrigin::Split => "split",
            ItemOrigin::CreateMonsterRefund => "create_monster_refund",
            ItemOrigin::Other(s) => s,
        }
    }

    /// `true` for the origins that move an existing item between containers or players: no `item.create`
    /// audit line (the move has its own event, 19 §21).
    pub fn is_move(self) -> bool {
        matches!(
            self,
            ItemOrigin::Pickup
                | ItemOrigin::Trade
                | ItemOrigin::MailTake
                | ItemOrigin::BankGet
                | ItemOrigin::PocketMove
                | ItemOrigin::VendorBuy
                | ItemOrigin::Split
        )
    }
}

/// Why an item is removed (the `item.destroy` audit reason; 19 §21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveReason {
    /// Used up by `DoUseItem` (audited only for premium items).
    Consume,
    /// Destroyed with `RemoveInvenItem`.
    Delete,
    /// A drop with `events.drop_item_remove` (KQ-16).
    DropRemove,
    /// Keep time ran out.
    Expire,
    /// An upgrade failed.
    UpgradeFail,
    /// The load stripped it (`RemoveItemIK3`).
    LoadStrip,
    /// An exchange script took it.
    Exchange,
    /// Sold to an NPC.
    Sell,
    /// Moved away (trade, mail, bank, vendor...): no `item.destroy` line.
    Moved,
    /// Anything else.
    Other(&'static str),
}

impl RemoveReason {
    /// The audit tag; `None` = no line.
    pub fn tag(self) -> Option<&'static str> {
        match self {
            RemoveReason::Consume => Some("consume"),
            RemoveReason::Delete => Some("delete"),
            RemoveReason::DropRemove => Some("drop_remove"),
            RemoveReason::Expire => Some("expire"),
            RemoveReason::UpgradeFail => Some("upgrade_fail"),
            RemoveReason::LoadStrip => Some("load_strip"),
            RemoveReason::Exchange => Some("exchange"),
            RemoveReason::Sell => Some("sell"),
            RemoveReason::Moved => None,
            RemoveReason::Other(s) => Some(s),
        }
    }
}

/// `UI_*` of `UpdateItem` (src:_Common/Mover.h:68-87).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum UiParam {
    /// `UI_NUM`: the count; <= 0 deletes.
    Num = 0,
    /// `UI_HP`: endurance / collector battery.
    Hp = 1,
    /// `UI_RN`: repair number.
    Rn = 2,
    /// `UI_AO`: ability option.
    Ao = 3,
    /// `UI_RAO`: element level.
    Rao = 4,
    /// `UI_IR`: element (or the text-quest flag).
    Ir = 5,
    /// `UI_PIERCING_SIZE`.
    PiercingSize = 6,
    /// `UI_PIERCING`: socket `LOWORD` = card `HIWORD`.
    Piercing = 7,
    /// `UI_COOLTIME`: like `Num`, the client starts the cooldown.
    CoolTime = 8,
    /// `UI_FLAG`: `MAKELONG(slot, flags)`.
    Flag = 9,
    /// `UI_RANDOMOPTITEMID`: the stat id (low byte).
    RandomOptItemId = 10,
    /// `UI_KEEPTIME`: minutes from now.
    KeepTime = 11,
    /// `UI_REPAIR_NUM`.
    RepairNum = 12,
    /// `UI_ULTIMATE_PIERCING_SIZE`.
    UltimatePiercingSize = 13,
    /// `UI_ULTIMATE_PIERCING`.
    UltimatePiercing = 14,
    /// `UI_PETVIS_SIZE`.
    PetVisSize = 15,
    /// `UI_PETVIS_ITEM`.
    PetVisItem = 16,
    /// `UI_PETVIS_ITEMSWAP`.
    PetVisItemSwap = 17,
    /// `UI_TRANSFORM_VISPET`.
    TransformVisPet = 18,
}

fn lo(v: u32) -> usize {
    (v & 0xFFFF) as usize
}

fn hi(v: u32) -> u32 {
    v >> 16
}

/// `MAKELONG(lo, hi)`.
pub fn make_long(lo: u32, hi: u32) -> u32 {
    (lo & 0xFFFF) | (hi << 16)
}

/// The mover-level functions.
pub mod core {
    use super::*;

    /// `CMover::CreateItem` (Plan/19 §4.1, exact order): an item without a prop is `NoProp`; an unset
    /// endurance (-1) becomes the prop's; collectors get a full battery; the container places it
    /// (`LackSpace` when it does not fit - the **caller** decides the feedback); on success the owner gets
    /// `CreateItem` and the audit line is queued. The item's serial is kept when set, else assigned.
    pub fn create_item(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        serials: &mut dyn SerialSource,
        mut item: Item,
        origin: ItemOrigin,
        fx: &mut Fx,
    ) -> Result<AddPlan, ItemError> {
        if item.item_id == 0 {
            return Err(ItemError::NoProp);
        }
        let prop = env.data().item(item.item_id).ok_or(ItemError::NoProp)?;
        if item.endurance == -1 {
            item.endurance = prop.endurance as i32;
        }
        if (item.item_id == II_GEN_TOO_COL_NORMAL || item.item_id == II_GEN_TOO_COL_EVENT)
            && item.collector_battery.is_none()
        {
            item.collector_battery = Some(prop.endurance.min(MAX_COLLECTOR_BATTERY));
        }
        let Some(p) = m.player_mut() else {
            return Err(ItemError::NotFound);
        };
        let plan = p
            .items
            .inventory
            .add(&item, env.data(), serials)
            .map_err(|e| match e {
                AddError::NoProp => ItemError::NoProp,
                AddError::Full => ItemError::LackSpace,
                AddError::Serial(s) => ItemError::Serial(s),
            })?;
        if plan.created.iter().any(|id| {
            p.items
                .inventory
                .get_at_id(*id)
                .is_some_and(|it| it.serial.0 == 0)
        }) {
            p.items.serial_pending = true;
        }
        let total: u32 = u32::from(item.count);
        let mut template = item.clone();
        template.serial = plan.template_serial;
        fx.out.to_self(pb::CreateItem {
            item_type: 0,
            item: Some(template.to_pb(0)),
            obj_ids: plan.touched.iter().map(|t| u32::from(t.0)).collect(),
            nums: plan.touched.iter().map(|t| i32::from(t.1)).collect(),
        });
        if !origin.is_move() && !quiet_stack(prop, total, origin) {
            audit_create(fx, &template, total, origin.name());
        }
        Ok(plan)
    }

    /// The legacy logging filter (19 §21): drops and pickups of FOOD / MAGIC / POTION / REFRESHER stacks of
    /// at most 9 are not logged.
    fn quiet_stack(
        prop: &flyff_data::props::item::ItemProp,
        count: u32,
        origin: ItemOrigin,
    ) -> bool {
        use flyff_common::defs::itemkind::{IK2_FOOD, IK2_MAGIC, IK2_POTION, IK2_REFRESHER};
        matches!(origin, ItemOrigin::Drop | ItemOrigin::Pickup)
            && count <= 9
            && matches!(
                prop.item_kind2,
                IK2_FOOD | IK2_MAGIC | IK2_POTION | IK2_REFRESHER
            )
    }

    /// `can_create`: would [`create_item`] succeed (`!is_full`)?
    pub fn can_create(m: &Mover, env: &ItemEnv<'_>, item: &Item) -> bool {
        let Some(p) = m.player() else { return false };
        let Some(prop) = env.data().item(item.item_id) else {
            return false;
        };
        !p.items
            .inventory
            .is_full(item, prop.pack_max.max(1), u32::from(item.count))
    }

    /// `RemoveItem(id, n)` = `UpdateItem(id, UI_NUM, count - n)`; the audit line carries the removed count
    /// and the reason.
    pub fn remove_item(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        id: StorageId,
        n: u16,
        reason: RemoveReason,
        fx: &mut Fx,
    ) {
        let Some(it) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) else {
            return;
        };
        let removed = n.min(it.count);
        if let Some(tag) = reason.tag() {
            fx.audit(
                ev::ITEM_DESTROY,
                json!({ "item": item_json(it), "count": removed, "reason": tag }),
            );
        }
        let new = i32::from(it.count) - i32::from(n);
        update_item(m, env, id, UiParam::Num, new as u32, 0, fx);
    }

    /// `RemoveItemA(item_id, n)` over the storage entries in ascending id order (equipped ones included);
    /// `None` removes every piece (`RemoveAllItem`). Returns the number removed.
    pub fn remove_item_by_id(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        item_id: u32,
        n: Option<u32>,
        reason: RemoveReason,
        fx: &mut Fx,
    ) -> u32 {
        let Some(p) = m.player() else { return 0 };
        let ids: Vec<(StorageId, u16)> = p
            .items
            .inventory
            .iter()
            .filter(|(_, it)| it.item_id == item_id)
            .map(|(id, it)| (id, it.count))
            .collect();
        let mut remaining = n;
        let mut removed = 0_u32;
        for (id, count) in ids {
            let take = match remaining {
                None => u32::from(count),
                Some(0) => break,
                Some(r) => r.min(u32::from(count)),
            };
            remove_item(m, env, id, take as u16, reason, fx);
            removed += take;
            if let Some(r) = remaining.as_mut() {
                *r -= take;
            }
        }
        removed
    }

    /// The equipment part an item is worn in.
    fn equip_part_of(m: &Mover, id: StorageId) -> Option<u32> {
        let inv = &m.player()?.items.inventory;
        if !inv.is_equip(id) {
            return None;
        }
        Some(u32::from(inv.slot_of(id)?) - u32::from(inv.index_num()))
    }

    /// What applying a parameter decided.
    enum Applied {
        /// The original returned early: change nothing, send nothing.
        Ignore,
        /// Applied; send this value.
        Done(u32),
        /// `UI_NUM` <= 0: delete the entry.
        Delete,
    }

    /// `UpdateItem(id, param, value, time)` (Plan/19 §4.1 table): applies the change to the item, then queues
    /// `UpdateItem{type: 0, obj_id, param, value, time}` for the owner. The early returns of the original
    /// (an AO or RAO above the maximum, a repair number of 100) send nothing; a parameter for a missing item
    /// sends nothing except `UI_FLAG`, whose message the client also uses for the equip info.
    pub fn update_item(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        id: StorageId,
        param: UiParam,
        value: u32,
        time: u32,
        fx: &mut Fx,
    ) {
        let max_pack = env.cfg().max_pack_item;
        let now = env.now.0;
        let worn_part = equip_part_of(m, id);
        let Some(p) = m.player_mut() else { return };
        let applied = match p.items.inventory.get_at_id_mut(id) {
            None if param == UiParam::Flag => Applied::Done(value),
            None => Applied::Ignore,
            Some(it) => {
                let kind2 = env.data().item(it.item_id).map(|pr| pr.item_kind2);
                apply_param(it, kind2, param, value, time, now, max_pack)
            }
        };
        let sent = match applied {
            Applied::Ignore => return,
            Applied::Done(v) => v,
            Applied::Delete => {
                remove_entry(m, env, id, worn_part, fx);
                value
            }
        };
        if worn_part.is_some() && applied_refreshes(param) {
            update_param(m, env);
        }
        fx.out.to_self(pb::UpdateItem {
            r#type: 0,
            obj_id: u32::from(id),
            param: param as i32,
            value: sent,
            time,
        });
    }

    /// The parameters after which the legacy called `UpdateParam()`.
    fn applied_refreshes(param: UiParam) -> bool {
        matches!(
            param,
            UiParam::Hp | UiParam::Rn | UiParam::Ao | UiParam::Rao | UiParam::Ir | UiParam::Flag
        )
    }

    /// The per-parameter rows of the `UpdateItem` switch.
    fn apply_param(
        it: &mut Item,
        kind2: Option<u32>,
        param: UiParam,
        value: u32,
        time: u32,
        now: i64,
        max_pack: u32,
    ) -> Applied {
        match param {
            UiParam::CoolTime | UiParam::Num => {
                if (value as i32) <= 0 {
                    return Applied::Delete;
                }
                let c = value.min(max_pack).min(u32::from(u16::MAX));
                it.count = c as u16;
                return Applied::Done(c);
            }
            UiParam::Hp => {
                if it.collector_battery.is_some() {
                    it.collector_battery = Some(value);
                } else {
                    it.endurance = value as i32;
                }
            }
            UiParam::RepairNum => it.repair_count = value as i32,
            UiParam::Rn => {
                if it.repair_number >= 100 {
                    return Applied::Ignore;
                }
                it.repair_number = value.min(100) as u8;
            }
            UiParam::Ao => {
                // 20 for gear and accessories, 5 for the normal collector
                let max: i32 = if it.is_collector(true) { 5 } else { 20 };
                if it.upgrade_level > max {
                    return Applied::Ignore;
                }
                it.upgrade_level = value.min(max as u32) as i32;
            }
            UiParam::Rao => {
                if it.element_level > 20 {
                    return Applied::Ignore;
                }
                it.element_level = value.min(20) as i32;
            }
            UiParam::Ir => {
                if kind2 == Some(IK2_TEXT) {
                    it.text_quest_started = value != 0;
                } else {
                    it.element = Element::from_u8(value as u8);
                }
            }
            UiParam::PiercingSize => {
                it.piercing
                    .cards
                    .resize((value as usize).min(MAX_PIERCING_WEAPON), 0);
            }
            UiParam::Piercing => {
                if let Some(slot) = it.piercing.cards.get_mut(lo(value)) {
                    *slot = hi(value);
                }
            }
            UiParam::UltimatePiercingSize => {
                it.piercing
                    .ultimate_gems
                    .resize((value as usize).min(MAX_PIERCING_ULTIMATE), 0);
            }
            UiParam::UltimatePiercing => {
                if let Some(slot) = it.piercing.ultimate_gems.get_mut(lo(value)) {
                    *slot = hi(value);
                }
            }
            UiParam::PetVisSize => {
                let n = (value as usize).min(MAX_VIS);
                it.piercing.cards.resize(n, 0);
                it.piercing.vis_keep_until.resize(n, 0);
            }
            UiParam::PetVisItem => {
                let idx = lo(value);
                if let Some(slot) = it.piercing.cards.get_mut(idx) {
                    *slot = hi(value);
                }
                if let Some(t) = it.piercing.vis_keep_until.get_mut(idx) {
                    *t = i64::from(time) * 60 + now;
                }
            }
            UiParam::PetVisItemSwap => {
                let (a, b) = (lo(value), hi(value) as usize);
                if a < it.piercing.cards.len() && b < it.piercing.cards.len() {
                    it.piercing.cards.swap(a, b);
                }
                if a < it.piercing.vis_keep_until.len() && b < it.piercing.vis_keep_until.len() {
                    it.piercing.vis_keep_until.swap(a, b);
                }
            }
            UiParam::TransformVisPet => {
                it.vis_transformed = value != 0;
                it.charged = true;
            }
            UiParam::RandomOptItemId => it.random_option.set_stat_id(value as u8),
            UiParam::KeepTime => it.keep_until = now + i64::from(value) * 60,
            UiParam::Flag => {}
        }
        Applied::Done(value)
    }

    /// `RemoveItemId`: unequips a worn item first (never fails and needs no bag space: the planner's
    /// `unequip_for_removal`), then empties the entry.
    fn remove_entry(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        id: StorageId,
        worn_part: Option<u32>,
        fx: &mut Fx,
    ) {
        if let Some(part) = worn_part {
            unequip_for_removal(m, env, part, &mut fx.out);
        }
        if let Some(p) = m.player_mut() {
            p.items.inventory.remove_at_id(id);
        }
    }

    /// `UpdateItemEx(id, UI_RANDOMOPTITEMID, word)`: replaces the whole packed word and queues
    /// `UpdateItemEx{obj_id, param: 10, value}` (the 64-bit form of `UI_RANDOMOPTITEMID`). A worn item's
    /// stats are re-applied.
    pub fn update_item_ex(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        id: StorageId,
        word: RandomOptWord,
        fx: &mut Fx,
    ) {
        let worn_part = equip_part_of(m, id);
        let Some(p) = m.player_mut() else { return };
        if let Some(it) = p.items.inventory.get_at_id_mut(id) {
            it.random_option = word;
        }
        if let Some(part) = worn_part {
            reapply_part(m, env, part, &mut fx.out);
        }
        fx.out.to_self(pb::UpdateItemEx {
            obj_id: u32::from(id),
            param: UiParam::RandomOptItemId as i32,
            value: word.to_i64(),
        });
    }

    /// `UpdateItem(id, UI_FLAG, ...)` the way the server uses it (19 §4.1): the new flag byte is set first,
    /// then `value = MAKELONG(slot index, flags)` goes out (the client applies `HIWORD` to the item and to the
    /// `EquipInfo` of `LOWORD - 168`). The equip info of a worn item follows.
    pub fn set_item_flags(m: &mut Mover, env: &ItemEnv<'_>, id: StorageId, flags: u8, fx: &mut Fx) {
        let worn_part = equip_part_of(m, id);
        let Some(p) = m.player_mut() else { return };
        let Some(slot) = p.items.inventory.slot_of(id) else {
            return;
        };
        if let Some(it) = p.items.inventory.get_at_id_mut(id) {
            it.flags.0 = flags;
        }
        if let Some(part) = worn_part
            && let Some(e) = m.mv.equip_info.get_mut(part as usize)
        {
            e.flag = flags;
        }
        update_item(
            m,
            env,
            id,
            UiParam::Flag,
            make_long(u32::from(slot), u32::from(flags)),
            0,
            fx,
        );
    }
}

/// The inventory-or-mail delivery of Plan/19 §4.1 lives with the mail service (WP-ECON-06).
pub use crate::econ::mail::{ItemDelivery, create_item_or_mail};

/// The serial source over the game's allocator (`Services.serials` + the persistence handle).
pub struct CtxSerials<'a> {
    alloc: &'a mut crate::persist::serial::SerialAllocator,
    src: GamePersist<'a>,
}

impl<'a> CtxSerials<'a> {
    /// A source over the allocator and the persistence handle / ticket registry of a `GameCtx`.
    pub fn new(
        alloc: &'a mut crate::persist::serial::SerialAllocator,
        persist: &'a flyff_db::PersistHandle,
        tickets: &'a mut crate::tickets::TicketRegistry,
    ) -> CtxSerials<'a> {
        CtxSerials {
            alloc,
            src: GamePersist { persist, tickets },
        }
    }
}

impl SerialSource for CtxSerials<'_> {
    fn next_serial(&mut self) -> Result<u64, SerialError> {
        self.alloc.next(&mut self.src)
    }
}

/// Runs `f` with the player's mover, the item environment, the game's serial source and a fresh [`Fx`], then
/// flushes the effects. `None` when `p` is not a live player.
fn with_player<R>(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    f: impl FnOnce(&mut Mover, &ItemEnv<'_>, &mut CtxSerials<'_>, &mut Fx) -> R,
) -> Option<R> {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    let r = {
        let mut serials = CtxSerials {
            alloc: &mut svc.serials,
            src: GamePersist {
                persist: ctx.persist,
                tickets: ctx.tickets,
            },
        };
        let m = ctx.worlds.player_mut(p.0)?;
        f(m, &env, &mut serials, &mut fx)
    };
    fx.flush(svc, ctx, p);
    Some(r)
}

/// `create_item(g, p, item, origin)` (Plan/19 §4.1): see [`core::create_item`]. `Err(LackSpace)` changes
/// nothing; the caller sends its own feedback.
pub fn create_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    item: Item,
    origin: ItemOrigin,
) -> Result<AddPlan, ItemError> {
    with_player(svc, ctx, p, |m, env, serials, fx| {
        core::create_item(m, env, serials, item, origin, fx)
    })
    .unwrap_or(Err(ItemError::NotFound))
}

/// `can_create(g, p, item)`: would the item fit?
pub fn can_create(svc: &Services, ctx: &GameCtx<'_>, p: PlayerRef, item: &Item) -> bool {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    ctx.player(p)
        .is_some_and(|m| core::can_create(m, &env, item))
}

/// `remove_item(g, p, obj_id, n, reason)`.
pub fn remove_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    obj_id: StorageId,
    n: u16,
    reason: RemoveReason,
) {
    with_player(svc, ctx, p, |m, env, _, fx| {
        core::remove_item(m, env, obj_id, n, reason, fx);
    });
}

/// `remove_item_by_id(g, p, item_id, n)` (`RemoveItemA`; `None` = all): returns the number removed.
pub fn remove_item_by_id(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    item_id: u32,
    n: Option<u32>,
    reason: RemoveReason,
) -> u32 {
    with_player(svc, ctx, p, |m, env, _, fx| {
        core::remove_item_by_id(m, env, item_id, n, reason, fx)
    })
    .unwrap_or(0)
}

/// `update_item(g, p, obj_id, param, value, time)`.
pub fn update_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    obj_id: StorageId,
    param: UiParam,
    value: u32,
    time: u32,
) {
    with_player(svc, ctx, p, |m, env, _, fx| {
        core::update_item(m, env, obj_id, param, value, time, fx);
    });
}

/// `update_item_ex(g, p, obj_id, word)` (`UI_RANDOMOPTITEMID`, 64 bit).
pub fn update_item_ex(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    obj_id: StorageId,
    word: RandomOptWord,
) {
    with_player(svc, ctx, p, |m, env, _, fx| {
        core::update_item_ex(m, env, obj_id, word, fx);
    });
}
