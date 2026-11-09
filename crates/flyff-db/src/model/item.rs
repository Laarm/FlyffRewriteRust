//! `ItemRecord`, `PetRecord` and `StoredItem` (Plan/13 §6.2): one `CItemElem` as JSON.
//!
//! The record keeps the legacy fields but gives overloaded ones honest names (`collector_battery`,
//! `sealed_character_id`, `couple_partner_id`, `text_quest_started`); the protocol keeps the legacy
//! semantics, so [`ItemRecord::legacy_hit_point`], [`ItemRecord::legacy_random_option`] and
//! [`ItemRecord::legacy_element`] re-create the overloaded wire values. Fields at their default are omitted
//! from the JSON; `count` and `serial` are always written.

use super::doc::{default, skip, v1};
use serde::{Deserialize, Serialize};

/// `ItemRecord.flags` bit: expired (`m_byFlag & 0x01`).
pub const ITEM_FLAG_EXPIRED: u8 = 0x01;
/// `ItemRecord.flags` bit: binds / soul-linked (`0x02`).
pub const ITEM_FLAG_BINDS: u8 = 0x02;
/// `ItemRecord.flags` bit: is being used (`0x04`, unused by the legacy server).
pub const ITEM_FLAG_ISUSING: u8 = 0x04;
/// Largest stack the loader accepts (`MAX_PACK_ITEM`, Plan/13 §6.2 `count`).
pub const MAX_ITEM_COUNT: u16 = 9999;
/// Longest `ItemRecord.text` in bytes (`m_szItemText[32]`).
pub const MAX_ITEM_TEXT_BYTES: usize = 31;

/// One item instance (`CItemElem`) as stored in containers, mails and grants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRecord {
    /// Storage index in its container (`m_dwObjId`); the id packets use. `0` in a [`StoredItem`].
    #[serde(default)]
    pub obj_id: u16,
    /// `propItem` id, never 0.
    pub item_id: u32,
    /// Stack count (`m_nItemNum`), 1..=9999.
    #[serde(default = "default::one_u16")]
    pub count: u16,
    /// Unique serial from `meta.next_item_serial` blocks; 0 = "assign one" (templates in grants).
    #[serde(default)]
    pub serial: u64,
    /// `m_szItemText` (at most 31 bytes UTF-8).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// `m_byFlag`: [`ITEM_FLAG_EXPIRED`] | [`ITEM_FLAG_BINDS`] | [`ITEM_FLAG_ISUSING`].
    #[serde(default, skip_serializing_if = "skip::is_zero_u8")]
    pub flags: u8,
    /// `m_nHitPoint` of ordinary items (dead feature, still on the wire); default -1.
    #[serde(
        default = "default::minus_one",
        skip_serializing_if = "skip::is_minus_one"
    )]
    pub endurance: i32,
    /// `m_nHitPoint` re-use: collector battery 0..1800.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collector_battery: Option<u32>,
    /// `m_nHitPoint` re-use of `II_SYS_SYS_SCR_SEALCHARACTER`: the sealed character id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_character_id: Option<u32>,
    /// `m_nRepair` (dead; mail calls it `nMaxHitPoint`).
    #[serde(default, skip_serializing_if = "skip::is_zero_i32")]
    pub repair_count: i32,
    /// `m_nRepairNumber` (dead).
    #[serde(default, skip_serializing_if = "skip::is_zero_u8")]
    pub repair_number: u8,
    /// `m_nAbilityOption` ("+N").
    #[serde(default, skip_serializing_if = "skip::is_zero_i32")]
    pub upgrade_level: i32,
    /// `m_bItemResist` (SAI79): 0 none, 1 fire, 2 water, 3 electricity, 4 wind, 5 earth.
    #[serde(default, skip_serializing_if = "skip::is_zero_u8")]
    pub element: u8,
    /// `m_nResistAbilityOption`.
    #[serde(default, skip_serializing_if = "skip::is_zero_i32")]
    pub element_level: i32,
    /// `m_nResistSMItemId`: the commercial element stone.
    #[serde(default, skip_serializing_if = "skip::is_zero_u32")]
    pub element_stone_id: u32,
    /// `m_idGuild`: owner of a guild cloak, 0 none.
    #[serde(default, skip_serializing_if = "skip::is_zero_u32")]
    pub guild_id: u32,
    /// `m_bCharged`: cash item.
    #[serde(default, skip_serializing_if = "skip::is_false")]
    pub charged: bool,
    /// `m_dwKeepTime`: absolute unix seconds, 0 = permanent.
    #[serde(default, skip_serializing_if = "skip::is_zero_i64")]
    pub keep_until: i64,
    /// `m_iRandomOptItemId` bit layout (ref/13 §5.3).
    #[serde(default, skip_serializing_if = "skip::is_zero_u64")]
    pub random_option: u64,
    /// `m_iRandomOptItemId` re-use by couple rings: the partner's character id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub couple_partner_id: Option<u32>,
    /// `m_n64NewRandomOption`: the pending protected-awakening result. FIX (PL-079): persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_random_option: Option<u64>,
    /// `m_bItemResist` re-use for `IK2_TEXT` items: the quest was started.
    #[serde(default, skip_serializing_if = "skip::is_false")]
    pub text_quest_started: bool,
    /// `m_bTranformVisPet`.
    #[serde(default, skip_serializing_if = "skip::is_false")]
    pub vis_transformed: bool,
    /// Normal sockets: length = socket count, 0 = empty socket.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub piercing_cards: Vec<u32>,
    /// Ultimate-weapon gem sockets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ultimate_gems: Vec<u32>,
    /// Vis slots: absolute expiry per slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vis_keep_until: Vec<i64>,
    /// System pet data of an egg item (`CPet`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pet: Option<PetRecord>,
}

impl Default for ItemRecord {
    /// A one-piece stack of "item 0" with the legacy defaults (`endurance` -1); callers set `item_id`.
    fn default() -> Self {
        Self {
            obj_id: 0,
            item_id: 0,
            count: 1,
            serial: 0,
            text: String::new(),
            flags: 0,
            endurance: -1,
            collector_battery: None,
            sealed_character_id: None,
            repair_count: 0,
            repair_number: 0,
            upgrade_level: 0,
            element: 0,
            element_level: 0,
            element_stone_id: 0,
            guild_id: 0,
            charged: false,
            keep_until: 0,
            random_option: 0,
            couple_partner_id: None,
            pending_random_option: None,
            text_quest_started: false,
            vis_transformed: false,
            piercing_cards: Vec::new(),
            ultimate_gems: Vec::new(),
            vis_keep_until: Vec::new(),
            pet: None,
        }
    }
}

impl ItemRecord {
    /// A plain item: `count` pieces of `item_id` with `serial` at storage index `obj_id`.
    pub fn new(obj_id: u16, item_id: u32, count: u16, serial: u64) -> Self {
        Self {
            obj_id,
            item_id,
            count,
            serial,
            ..Self::default()
        }
    }

    /// The legacy `m_nHitPoint` the wire carries (13 §6.2 precedence: `sealed_character_id`, then
    /// `collector_battery`, then `endurance`; the first `Some` wins).
    pub fn legacy_hit_point(&self) -> i32 {
        if let Some(id) = self.sealed_character_id {
            id as i32
        } else if let Some(battery) = self.collector_battery {
            battery as i32
        } else {
            self.endurance
        }
    }

    /// The legacy `m_iRandomOptItemId` the wire carries: the couple partner id when present, else the
    /// packed random options.
    pub fn legacy_random_option(&self) -> u64 {
        match self.couple_partner_id {
            Some(partner) => u64::from(partner),
            None => self.random_option,
        }
    }

    /// The legacy `m_bItemResist` the wire carries: 1 for a started `IK2_TEXT` quest item, else the element.
    pub fn legacy_element(&self) -> u8 {
        if self.text_quest_started {
            1
        } else {
            self.element
        }
    }

    /// `true` when [`ITEM_FLAG_EXPIRED`] is set.
    pub fn is_expired_flag(&self) -> bool {
        self.flags & ITEM_FLAG_EXPIRED != 0
    }
}

/// System pet data of an egg item (`CPet`, ref/13 §5.4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PetRecord {
    /// Pet kind (`m_nKind`).
    #[serde(default)]
    pub kind: u8,
    /// `PL_EGG` 0 ... `PL_S` 5.
    #[serde(default)]
    pub level: u8,
    /// Experience.
    #[serde(default)]
    pub exp: u32,
    /// Energy.
    #[serde(default)]
    pub energy: u16,
    /// Life.
    #[serde(default)]
    pub life: u16,
    /// Stat-gain roll per level D..S at index `level - 1`; 0 = not rolled.
    #[serde(default)]
    pub avail_levels: [u8; 6],
    /// Name, at most 16 bytes UTF-8; empty = unnamed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// An item stored on its own (mail attachments, pending grants): `{"schema_version":1, ...item fields}`.
/// `obj_id` is meaningless there and always written as 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredItem {
    /// Document version (13 §6.1).
    #[serde(default = "v1")]
    pub schema_version: u32,
    /// The item fields, flattened into the same object.
    #[serde(flatten)]
    pub item: ItemRecord,
}

impl StoredItem {
    /// Wraps `item` for standalone storage (`obj_id` is reset to 0).
    pub fn new(mut item: ItemRecord) -> Self {
        item.obj_id = 0;
        Self {
            schema_version: <Self as super::JsonDoc>::SCHEMA_VERSION,
            item,
        }
    }
}

impl super::JsonDoc for StoredItem {
    const KIND: &'static str = "stored_item";
    const SCHEMA_VERSION: u32 = 1;
}
