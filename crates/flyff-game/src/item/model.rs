//! The runtime item (Plan/19 §2.1-§2.5, WP-ITEM-01): `Item` mirrors the persisted `ItemRecord` one to one
//! (the overloaded legacy fields are split into honest ones) and converts to and from it and to the wire
//! `ItemElem`.
//!
//! Which overloaded legacy field applies is decided by the conversions: the wire `hit_point` is the sealed
//! character id, else the collector battery, else the endurance; the wire `random_opt_item_id` is the couple
//! partner id when a ring carries one, else the packed word; the wire `item_resist` is 1 for a started text
//! quest item, else the element.

use flyff_common::ItemSerial;
use flyff_common::defs::item::{II_GEN_TOO_COL_EVENT, II_GEN_TOO_COL_NORMAL, II_SYS_SYS_SCR_PERIN};
use flyff_common::defs::itemkind::{
    IK2_ARMOR, IK2_ARMORETC, IK2_WARP, IK2_WEAPON_DIRECT, IK2_WEAPON_MAGIC, IK3_CLOAK, IK3_EGG,
    IK3_PET, IK3_SUIT,
};
use flyff_common::defs::neuz::PARTS_RIDE;
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_data::types::ItemId;
use flyff_db::{ItemRecord, PetRecord};
use flyff_proto::pb;
use flyff_rules::item::random_option::{RandomOptWord, random_option_kind};
use flyff_rules::item::{
    IP_FLAG_BINDS, IP_FLAG_UNDESTRUCTABLE, ITEM_FLAG_BINDS, ITEM_FLAG_EXPIRED,
};

/// `NULL_ID` as the `dwItemJob` / `dwParts` of an unrestricted item.
const NULL_ID: u32 = u32::MAX;

/// Sockets on a weapon or shield (`MAX_PIERCING_WEAPON`).
pub const MAX_PIERCING_WEAPON: usize = 10;
/// Sockets on a suit (`MAX_PIERCING_SUIT`).
pub const MAX_PIERCING_SUIT: usize = 4;
/// Ultimate gem sockets (`MAX_PIERCING_ULTIMATE`).
pub const MAX_PIERCING_ULTIMATE: usize = 5;
/// Vis slots of a vis pet (`MAX_VIS`).
pub const MAX_VIS: usize = 9;
/// The vis slots a fresh vis pet starts with (`PETVIS_DEFAULT_VIS_SLOT_SIZE`).
pub const PETVIS_DEFAULT_VIS_SLOT_SIZE: usize = 2;
/// The capacity of a collector battery (`nMaxBattery`).
pub const MAX_COLLECTOR_BATTERY: u32 = 1800;

/// The item flag byte (`m_byFlag`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemFlags(pub u8);

impl ItemFlags {
    /// `expired`.
    pub const EXPIRED: u8 = ITEM_FLAG_EXPIRED;
    /// `binds`.
    pub const BINDS: u8 = ITEM_FLAG_BINDS;
    /// `isusing` (never set by the server).
    pub const ISUSING: u8 = 0x04;

    /// `true` when every bit of `bit` is set.
    pub fn has(self, bit: u8) -> bool {
        self.0 & bit == bit
    }

    /// Sets or clears `bit`.
    pub fn set(&mut self, bit: u8, on: bool) {
        if on {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }
}

/// The element of an upgraded item (`SAI79::ePropType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Element {
    /// `NO_PROP`.
    #[default]
    None = 0,
    /// Fire.
    Fire = 1,
    /// Water.
    Water = 2,
    /// Electricity.
    Electricity = 3,
    /// Wind.
    Wind = 4,
    /// Earth.
    Earth = 5,
}

impl Element {
    /// The element of a stored byte (an unknown value reads as none).
    pub fn from_u8(v: u8) -> Element {
        match v {
            1 => Element::Fire,
            2 => Element::Water,
            3 => Element::Electricity,
            4 => Element::Wind,
            5 => Element::Earth,
            _ => Element::None,
        }
    }
}

/// The sockets of an item (Plan/19 §2.3, ref/13 §18.6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PiercingSet {
    /// Normal sockets: the length is the socket count, 0 = an empty socket.
    pub cards: Vec<u32>,
    /// Ultimate gem sockets (ability item ids).
    pub ultimate_gems: Vec<u32>,
    /// Vis slots: the absolute expiry of each slot (vis pets only; same length as `cards` there).
    pub vis_keep_until: Vec<i64>,
}

impl PiercingSet {
    /// `true` when there are no sockets of any kind.
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty() && self.ultimate_gems.is_empty() && self.vis_keep_until.is_empty()
    }
}

/// The level of a system pet (`PL_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PetLevel {
    /// `PL_EGG`.
    Egg = 0,
    /// `PL_D`.
    D = 1,
    /// `PL_C`.
    C = 2,
    /// `PL_B`.
    B = 3,
    /// `PL_A`.
    A = 4,
    /// `PL_S`.
    S = 5,
}

impl PetLevel {
    /// The level of a stored byte; anything above 5 reads as `S`.
    pub fn from_u8(v: u8) -> PetLevel {
        match v {
            0 => PetLevel::Egg,
            1 => PetLevel::D,
            2 => PetLevel::C,
            3 => PetLevel::B,
            4 => PetLevel::A,
            _ => PetLevel::S,
        }
    }
}

/// The system pet inside an egg item (Plan/19 §2.4, `CPet`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetState {
    /// 0 tiger STR, 1 lion STA, 2 rabbit DEX, 3 fox INT.
    pub kind: u8,
    /// The pet level.
    pub level: PetLevel,
    /// Experience.
    pub exp: u32,
    /// Energy.
    pub energy: u16,
    /// Life.
    pub life: u16,
    /// The stat roll per level D..S at index `level - 1`; 0 = not rolled.
    pub avail_levels: [u8; 6],
    /// The name (at most 16 bytes); empty = unnamed.
    pub name: String,
}

impl From<&PetRecord> for PetState {
    fn from(r: &PetRecord) -> PetState {
        PetState {
            kind: r.kind,
            level: PetLevel::from_u8(r.level),
            exp: r.exp,
            energy: r.energy,
            life: r.life,
            avail_levels: r.avail_levels,
            name: r.name.clone(),
        }
    }
}

impl From<&PetState> for PetRecord {
    fn from(p: &PetState) -> PetRecord {
        PetRecord {
            kind: p.kind,
            level: p.level as u8,
            exp: p.exp,
            energy: p.energy,
            life: p.life,
            avail_levels: p.avail_levels,
            name: p.name.clone(),
        }
    }
}

/// One item instance (`CItemElem`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// `propItem` id, never 0.
    pub item_id: ItemId,
    /// The unique serial (19 §2.7).
    pub serial: ItemSerial,
    /// `m_nItemNum`, `1..=pack_max`.
    pub count: u16,
    /// `m_szItemText`, at most 31 bytes.
    pub text: String,
    /// `m_byFlag`.
    pub flags: ItemFlags,
    /// `m_nHitPoint` of an ordinary item (the dead endurance feature, still on the wire); -1 = unset.
    pub endurance: i32,
    /// `m_nHitPoint` re-use of the collectors.
    pub collector_battery: Option<u32>,
    /// `m_nHitPoint` re-use of `II_SYS_SYS_SCR_SEALCHARACTER`.
    pub sealed_character_id: Option<u32>,
    /// `m_nRepair` (dead).
    pub repair_count: i32,
    /// `m_nRepairNumber` (dead).
    pub repair_number: u8,
    /// `m_nAbilityOption`, "+N".
    pub upgrade_level: i32,
    /// `m_bItemResist`.
    pub element: Element,
    /// `m_nResistAbilityOption`.
    pub element_level: i32,
    /// `m_nResistSMItemId`: the applied commercial element stone.
    pub element_stone_id: u32,
    /// `m_idGuild`.
    pub guild_id: u32,
    /// `m_bCharged`.
    pub charged: bool,
    /// `m_dwKeepTime`: absolute unix seconds, 0 = permanent.
    pub keep_until: i64,
    /// `m_iRandomOptItemId`.
    pub random_option: RandomOptWord,
    /// `m_iRandomOptItemId` re-use of the couple rings.
    pub couple_partner_id: Option<u32>,
    /// `m_n64NewRandomOption`: the pending protected-awakening result (persisted, PL-079).
    pub pending_random_option: Option<RandomOptWord>,
    /// `m_bItemResist` re-use of `IK2_TEXT`: the quest was started.
    pub text_quest_started: bool,
    /// `m_bTranformVisPet`.
    pub vis_transformed: bool,
    /// The sockets.
    pub piercing: PiercingSet,
    /// The system pet of an egg.
    pub pet: Option<Box<PetState>>,
    /// Runtime only: the count offered in a trade or registered in a vendor (`m_nExtra`).
    pub reserved: u16,
    /// Runtime only: the vendor unit price (`m_nCost`), meaningful while `reserved`.
    pub vendor_price: i32,
}

impl Item {
    /// A new stack of `item_id` with the legacy defaults (`endurance` -1, to be set from the prop by
    /// `create_item`). The serial is assigned by the caller.
    pub fn new(item_id: ItemId, count: u16) -> Item {
        Item {
            item_id,
            serial: ItemSerial(0),
            count,
            text: String::new(),
            flags: ItemFlags::default(),
            endurance: -1,
            collector_battery: None,
            sealed_character_id: None,
            repair_count: 0,
            repair_number: 0,
            upgrade_level: 0,
            element: Element::None,
            element_level: 0,
            element_stone_id: 0,
            guild_id: 0,
            charged: false,
            keep_until: 0,
            random_option: RandomOptWord(0),
            couple_partner_id: None,
            pending_random_option: None,
            text_quest_started: false,
            vis_transformed: false,
            piercing: PiercingSet::default(),
            pet: None,
            reserved: 0,
            vendor_price: 0,
        }
    }

    /// The runtime item of a persisted record (`obj_id` is the container's business; `reserved` and
    /// `vendor_price` start at 0).
    pub fn from_record(r: &ItemRecord) -> Item {
        Item {
            item_id: r.item_id,
            serial: ItemSerial(r.serial),
            count: r.count,
            text: r.text.clone(),
            flags: ItemFlags(r.flags),
            endurance: r.endurance,
            collector_battery: r.collector_battery,
            sealed_character_id: r.sealed_character_id,
            repair_count: r.repair_count,
            repair_number: r.repair_number,
            upgrade_level: r.upgrade_level,
            element: Element::from_u8(r.element),
            element_level: r.element_level,
            element_stone_id: r.element_stone_id,
            guild_id: r.guild_id,
            charged: r.charged,
            keep_until: r.keep_until,
            random_option: RandomOptWord(r.random_option),
            couple_partner_id: r.couple_partner_id,
            pending_random_option: r.pending_random_option.map(RandomOptWord),
            text_quest_started: r.text_quest_started,
            vis_transformed: r.vis_transformed,
            piercing: PiercingSet {
                cards: r.piercing_cards.clone(),
                ultimate_gems: r.ultimate_gems.clone(),
                vis_keep_until: r.vis_keep_until.clone(),
            },
            pet: r.pet.as_ref().map(|p| Box::new(PetState::from(p))),
            reserved: 0,
            vendor_price: 0,
        }
    }

    /// The persisted record at storage index `obj_id`; the runtime-only fields are dropped (a reserved
    /// item is saved as an ordinary item, 13 §17.3 rule 4).
    pub fn to_record(&self, obj_id: u16) -> ItemRecord {
        ItemRecord {
            obj_id,
            item_id: self.item_id,
            count: self.count,
            serial: self.serial.0,
            text: self.text.clone(),
            flags: self.flags.0,
            endurance: self.endurance,
            collector_battery: self.collector_battery,
            sealed_character_id: self.sealed_character_id,
            repair_count: self.repair_count,
            repair_number: self.repair_number,
            upgrade_level: self.upgrade_level,
            element: self.element as u8,
            element_level: self.element_level,
            element_stone_id: self.element_stone_id,
            guild_id: self.guild_id,
            charged: self.charged,
            keep_until: self.keep_until,
            random_option: self.random_option.0,
            couple_partner_id: self.couple_partner_id,
            pending_random_option: self.pending_random_option.map(|w| w.0),
            text_quest_started: self.text_quest_started,
            vis_transformed: self.vis_transformed,
            piercing_cards: self.piercing.cards.clone(),
            ultimate_gems: self.piercing.ultimate_gems.clone(),
            vis_keep_until: self.piercing.vis_keep_until.clone(),
            pet: self.pet.as_deref().map(PetRecord::from),
        }
    }

    /// The legacy `m_nHitPoint` the wire carries: the sealed character id, else the collector battery, else
    /// the endurance (13 §6.2 precedence).
    pub fn wire_hit_point(&self) -> i32 {
        if let Some(id) = self.sealed_character_id {
            id as i32
        } else if let Some(battery) = self.collector_battery {
            battery as i32
        } else {
            self.endurance
        }
    }

    /// The legacy `m_iRandomOptItemId` the wire carries: the couple partner id when present, else the word.
    pub fn wire_random_option(&self) -> i64 {
        match self.couple_partner_id {
            Some(partner) => i64::from(partner),
            None => self.random_option.to_i64(),
        }
    }

    /// The legacy `m_bItemResist` the wire carries: 1 for a started text-quest item, else the element.
    pub fn wire_element(&self) -> u8 {
        if self.text_quest_started {
            1
        } else {
            self.element as u8
        }
    }

    /// `CItemElem::Serialize` (ref/08 §7.7.1) for the item at storage index `obj_id`.
    pub fn to_pb(&self, obj_id: u16) -> pb::ItemElem {
        let piercing = if self.piercing.is_empty() {
            None
        } else {
            Some(pb::Piercing {
                card_item_ids: self.piercing.cards.clone(),
                ultimate_gem_ids: self.piercing.ultimate_gems.clone(),
                vis_expire_at: self.piercing.vis_keep_until.clone(),
            })
        };
        pb::ItemElem {
            obj_id: u32::from(obj_id),
            item_id: self.item_id,
            serial: self.serial.0,
            item_text: self.text.clone(),
            num: i32::from(self.count),
            repair_number: u32::from(self.repair_number),
            hit_point: self.wire_hit_point(),
            repair: self.repair_count,
            flag: u32::from(self.flags.0),
            ability_option: self.upgrade_level,
            id_guild: self.guild_id,
            item_resist: u32::from(self.wire_element()),
            resist_ability_option: self.element_level,
            resist_sm_item_id: i32::try_from(self.element_stone_id).unwrap_or(i32::MAX),
            piercing,
            charged: self.charged,
            random_opt_item_id: self.wire_random_option(),
            keep_time_at: self.keep_until,
            pet: self.pet.as_deref().map(|p| pb::PetData {
                kind: u32::from(p.kind),
                level: p.level as u32,
                exp: p.exp,
                energy: u32::from(p.energy),
                life: u32::from(p.life),
                avail_level: p.avail_levels.iter().map(|&v| u32::from(v)).collect(),
                name: p.name.clone(),
            }),
            transform_vis_pet: self.vis_transformed,
        }
    }

    // ---- predicates (Plan/19 §2.1; src:_Common/Item.cpp:323-421, 739-829) --------------------------------

    /// `IsUsableItem`: nothing of it is offered in a trade or a vendor.
    pub fn is_usable(&self) -> bool {
        self.reserved == 0
    }

    /// `IsCharged`.
    pub fn is_charged(&self) -> bool {
        self.charged || self.element_stone_id != 0
    }

    /// `IsBinds`: any of a timed item (except warp rings), the prop's `BINDS` flag, the item's `binds` flag, a
    /// Blessing / EatPet random option with at least one line, or a level-down.
    /// `only_armor_awakes` is `features.only_armor_awakes` (it decides which items have a random-option kind).
    pub fn is_binds(&self, p: &ItemProp, only_armor_awakes: bool) -> bool {
        if self.keep_until != 0 && p.item_kind2 != IK2_WARP {
            return true;
        }
        if p.flag & IP_FLAG_BINDS != 0 || self.flags.has(ItemFlags::BINDS) {
            return true;
        }
        if self.random_option.line_count() > 0 {
            let egg_c = self.pet.as_ref().is_some_and(|x| x.level >= PetLevel::C);
            if matches!(
                random_option_kind(p.parts, p.item_kind3, egg_c, only_armor_awakes),
                Some(
                    flyff_rules::item::RandomOptKind::Blessing
                        | flyff_rules::item::RandomOptKind::EatPet
                )
            ) {
                return true;
            }
        }
        self.random_option.level_down().levels() < 0
    }

    /// `IsFlag(expired)`.
    pub fn is_expired(&self) -> bool {
        self.flags.has(ItemFlags::EXPIRED)
    }

    /// `IsExpiring`: the keep time has passed.
    pub fn is_expiring(&self, now: i64) -> bool {
        self.keep_until != 0 && now > self.keep_until
    }

    /// `IsUndestructable` (a prop flag).
    pub fn is_undestructable(p: &ItemProp) -> bool {
        p.flag & IP_FLAG_UNDESTRUCTABLE != 0
    }

    /// A guild cloak: a cloak that carries a guild id.
    pub fn is_guild_cloak(&self, p: &ItemProp) -> bool {
        p.item_kind3 == IK3_CLOAK && self.guild_id != 0
    }

    /// A ride of the vagrant job (deleted when unequipped, not tradeable).
    pub fn is_vagrant_ride(p: &ItemProp) -> bool {
        p.parts == PARTS_RIDE && p.item_job == flyff_common::defs::job::JOB_VAGRANT
    }

    /// `IsEgg`: an egg item whose pet (if any) is still an egg.
    pub fn is_egg(&self, p: &ItemProp) -> bool {
        p.item_kind3 == IK3_EGG && self.pet.as_ref().is_none_or(|x| x.level == PetLevel::Egg)
    }

    /// `IsPet`: `IK3_EGG`.
    pub fn is_pet(p: &ItemProp) -> bool {
        p.item_kind3 == IK3_EGG
    }

    /// `IsEatPet`: `IK3_PET`.
    pub fn is_eat_pet(p: &ItemProp) -> bool {
        p.item_kind3 == IK3_PET
    }

    /// `IsVisPet`: an eat pet of the vis kind, or one converted to it.
    pub fn is_vis_pet(&self, p: &ItemProp) -> bool {
        p.is_vis_pet() || self.vis_transformed
    }

    /// `IsCollector(refinable)`: the normal collector, or the event collector when `!refinable`.
    pub fn is_collector(&self, refinable: bool) -> bool {
        self.item_id == II_GEN_TOO_COL_NORMAL
            || (!refinable && self.item_id == II_GEN_TOO_COL_EVENT)
    }

    /// `IsAccessory`: the item is in `accessory.inc`.
    pub fn is_accessory(&self, data: &GameData) -> bool {
        data.accessory.items.iter().any(|a| a.item == self.item_id)
    }

    /// `IsPerin`.
    pub fn is_perin(&self) -> bool {
        self.item_id == II_SYS_SYS_SCR_PERIN
    }

    /// `IsDiceRefineryAble`: general refine is allowed.
    pub fn is_dice_refinery_able(p: &ItemProp) -> bool {
        matches!(
            p.item_kind2,
            IK2_ARMOR | IK2_ARMORETC | IK2_WEAPON_DIRECT | IK2_WEAPON_MAGIC
        )
    }

    /// `IsEleRefineryAble`: element upgrade is allowed.
    pub fn is_ele_refinery_able(p: &ItemProp) -> bool {
        p.item_kind3 == IK3_SUIT || matches!(p.item_kind2, IK2_WEAPON_MAGIC | IK2_WEAPON_DIRECT)
    }

    /// `ExeTarget == EXT_ITEM`: the item needs another item as its target.
    pub fn is_need_target(p: &ItemProp) -> bool {
        p.exe_target == flyff_common::defs::define::EXT_ITEM
    }

    /// `GetAttrOption`: `ability_option | element << 16 | element_level << 24` (the `EquipInfo.option`).
    pub fn attr_option(&self) -> i32 {
        self.upgrade_level | ((self.element as i32) << 16) | (self.element_level << 24)
    }

    /// `GetLimitLevel`: `NULL_ID` without a limit, else `max(0, limit_level1 + level_down)`.
    pub fn limit_level(&self, p: &ItemProp) -> u32 {
        flyff_rules::equip_rules::limit_level(p, self.random_option)
    }

    /// `true` when the item can be stacked with `other` (`IsFull` / `Add` / `Swap` compatibility): same id,
    /// flags and charged state. AO, element, options, keep time and serial are ignored (KEEP-QUIRK, KQ-15).
    pub fn stacks_with(&self, other: &Item) -> bool {
        self.item_id == other.item_id && self.flags == other.flags && self.charged == other.charged
    }

    /// `true` when the prop makes the item unusable as equipment (`dwParts == NULL_ID`).
    pub fn is_equipable(p: &ItemProp) -> bool {
        p.parts != NULL_ID
    }
}
