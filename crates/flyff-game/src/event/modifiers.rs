//! `EventModifiers`: the cached aggregates of the running events, and [`EventFactors`], the read-only
//! query surface the progression, drop, combat and shop code consumes (Plan/22 §4.3, §4.4; ref/15 §4.2 step 4:
//! the legacy `CEventLuaProxy`).
//!
//! Gameplay never calls Lua for these numbers. The Event.lua pass ([`super::lua_events`]) recomputes the
//! struct when the set of running events changes (a state change, a reload, the first pass at start) and every
//! consumer reads the cached values. With the shipped `Event.lua` ("Level Up Rewards!", permanent) they are the
//! repack's rates: EXP x20, item drop x10, piece drop x10, gold x250, +1 ATK and +1 DEF.
//!
//! [`EventFactors`] has a neutral default for every query (1.0 / 0), so a consumer written against the trait
//! works unchanged before the Event service exists ([`NoEvents`]) and in tests; [`EventModifiers`] is the real
//! implementation. The consumers multiply the factors in the legacy order, in `f32` (22 §4.4):
//!
//! | Effect | Composition |
//! |---|---|
//! | EXP | item buffs ... x cheer x [`generic_exp`](EventFactors::generic_exp) x [`lua_exp`](EventFactors::lua_exp) x `rates.monster_exp` x (1 + DST_EXPERIENCE/100) x [`lord_exp`](EventFactors::lord_exp) x 1.1 x 1.05 x weather |
//! | item drop rate | `rates.item_drop` x prop `m_fItemDrop_Rate` x [`generic_item_drop`](EventFactors::generic_item_drop) x [`lua_item_drop`](EventFactors::lua_item_drop) |
//! | piece-item drop rate | [`lua_piece_drop`](EventFactors::lua_piece_drop) x [`lord_item`](EventFactors::lord_item) x 1.1 x 1.05 |
//! | gold drop | `gold = (int)(gold as f32 x` [`lua_gold`](EventFactors::lua_gold) `)` |
//! | player ATK / DEF | `+` [`lua_attack`](EventFactors::lua_attack) / [`lua_defense`](EventFactors::lua_defense) after the DST parameter |
//! | shop buy / sell | `(int)(factor x cost)` |

use flyff_data::types::ItemId;

/// The factor queries (see the module documentation). Every method defaults to the neutral value.
pub trait EventFactors {
    /// `EventLua.GetExpFactor()`: the product of the EXP factors of the running events.
    fn lua_exp(&self) -> f32 {
        1.0
    }
    /// `EventLua.GetItemDropRate()`.
    fn lua_item_drop(&self) -> f32 {
        1.0
    }
    /// `EventLua.GetPieceItemDropRate()`.
    fn lua_piece_drop(&self) -> f32 {
        1.0
    }
    /// `EventLua.GetGoldDropFactor()`.
    fn lua_gold(&self) -> f32 {
        1.0
    }
    /// `EventLua.GetAttackPower()`: the flat ATK bonus of players (`1 + sum`, KQ-11; 0 without Event.lua).
    fn lua_attack(&self) -> i32 {
        0
    }
    /// `EventLua.GetDefensePower()`: the flat DEF bonus of players.
    fn lua_defense(&self) -> i32 {
        0
    }
    /// `EventLua.GetCheerExpFactor()` (used by the `II_CHEERUP` buff).
    fn lua_cheer_exp(&self) -> f32 {
        1.0
    }
    /// `EventLua.GetWeatherEventExpFactor()`: missing in the shipped `EventFunc.lua`, so 1.0 (KQ-11).
    fn lua_weather_exp(&self) -> f32 {
        1.0
    }
    /// `EventLua.GetShopBuyFactor()`: missing in the shipped `EventFunc.lua`, so 1.0 (KQ-11).
    fn shop_buy(&self) -> f32 {
        1.0
    }
    /// `EventLua.GetShopSellFactor()`: missing in the shipped `EventFunc.lua`, so 1.0 (KQ-11).
    fn shop_sell(&self) -> f32 {
        1.0
    }
    /// The `propEvent.inc` generic events' EXP factor (doc 22 §6, WP-EVENT-07).
    fn generic_exp(&self) -> f32 {
        1.0
    }
    /// The `propEvent.inc` generic events' item drop factor.
    fn generic_item_drop(&self) -> f32 {
        1.0
    }
    /// The lord events' EXP factor, `1 + sum fE` (doc 22 §9.5).
    fn lord_exp(&self) -> f32 {
        1.0
    }
    /// The lord events' item factor, `1 + sum fI`.
    fn lord_item(&self) -> f32 {
        1.0
    }
    /// The weather effect is active (the weather EXP factor then applies).
    fn weather_effect(&self) -> bool {
        false
    }
}

/// No events at all: every factor is neutral. The default consumer of [`EventFactors`] until the Event
/// service feeds the real values, and the answer while `Event.lua` is `Missing` or `Faulted`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoEvents;

impl EventFactors for NoEvents {}

impl<T: EventFactors + ?Sized> EventFactors for &T {
    fn lua_exp(&self) -> f32 {
        (**self).lua_exp()
    }
    fn lua_item_drop(&self) -> f32 {
        (**self).lua_item_drop()
    }
    fn lua_piece_drop(&self) -> f32 {
        (**self).lua_piece_drop()
    }
    fn lua_gold(&self) -> f32 {
        (**self).lua_gold()
    }
    fn lua_attack(&self) -> i32 {
        (**self).lua_attack()
    }
    fn lua_defense(&self) -> i32 {
        (**self).lua_defense()
    }
    fn lua_cheer_exp(&self) -> f32 {
        (**self).lua_cheer_exp()
    }
    fn lua_weather_exp(&self) -> f32 {
        (**self).lua_weather_exp()
    }
    fn shop_buy(&self) -> f32 {
        (**self).shop_buy()
    }
    fn shop_sell(&self) -> f32 {
        (**self).shop_sell()
    }
    fn generic_exp(&self) -> f32 {
        (**self).generic_exp()
    }
    fn generic_item_drop(&self) -> f32 {
        (**self).generic_item_drop()
    }
    fn lord_exp(&self) -> f32 {
        (**self).lord_exp()
    }
    fn lord_item(&self) -> f32 {
        (**self).lord_item()
    }
    fn weather_effect(&self) -> bool {
        (**self).weather_effect()
    }
}

/// The cached aggregates (Plan/22 §4.3). `f32` is the C++ `(float)ToNumber`, `i32` the `(int)` truncation.
#[derive(Clone, PartialEq, Debug)]
pub struct EventModifiers {
    /// `GetExpFactor`.
    pub lua_exp: f32,
    /// `GetItemDropRate`.
    pub lua_item_drop: f32,
    /// `GetPieceItemDropRate`.
    pub lua_piece_drop: f32,
    /// `GetGoldDropFactor`.
    pub lua_gold: f32,
    /// `GetAttackPower`: `1 + sum` when Event.lua is loaded, 0 when the family is `Faulted` or `Missing`.
    pub lua_attack: i32,
    /// `GetDefensePower`.
    pub lua_defense: i32,
    /// `GetCheerExpFactor`.
    pub lua_cheer_exp: f32,
    /// `GetWeatherEventExpFactor`; the function is missing in the shipped script, so 1.0.
    pub lua_weather_exp: f32,
    /// `GetShopBuyFactor`; missing, so 1.0.
    pub shop_buy: f32,
    /// `GetShopSellFactor`; missing, so 1.0.
    pub shop_sell: f32,
    /// `GetCouponEvent` in ms: the first non-zero value of the running events, 0 = off.
    pub coupon_ms: u32,
    /// `GetKeepConnectTime` in ms: the first non-zero value, 0 = off.
    pub keep_connect_ms: u32,
    /// `GetKeepConnectItem`: every keep-connect item of every running event (an unknown item id is dropped).
    pub keep_connect_items: Vec<(ItemId, i32)>,
    /// The `propEvent.inc` generic events (WP-EVENT-07 fills these).
    pub generic_exp: f32,
    /// See [`EventModifiers::generic_exp`].
    pub generic_item_drop: f32,
    /// The lord events, `1 + sum fE` (WP-EVENT-10/11 fill these).
    pub lord_exp: f32,
    /// `1 + sum fI`.
    pub lord_item: f32,
    /// The weather effect (WP-EVENT-22).
    pub weather_effect: bool,
}

impl EventModifiers {
    /// No event: factors 1.0, ATK/DEF 0, no times, no items.
    pub const NEUTRAL: EventModifiers = EventModifiers {
        lua_exp: 1.0,
        lua_item_drop: 1.0,
        lua_piece_drop: 1.0,
        lua_gold: 1.0,
        lua_attack: 0,
        lua_defense: 0,
        lua_cheer_exp: 1.0,
        lua_weather_exp: 1.0,
        shop_buy: 1.0,
        shop_sell: 1.0,
        coupon_ms: 0,
        keep_connect_ms: 0,
        keep_connect_items: Vec::new(),
        generic_exp: 1.0,
        generic_item_drop: 1.0,
        lord_exp: 1.0,
        lord_item: 1.0,
        weather_effect: false,
    };
}

impl EventModifiers {
    /// The same values with the Event.lua part (22 §4.3 first block) back at neutral: what a recompute starts from, so
    /// the generic, lord and weather parts that other services fill survive it.
    #[must_use]
    pub fn with_lua_neutral(&self) -> EventModifiers {
        EventModifiers {
            lua_exp: 1.0,
            lua_item_drop: 1.0,
            lua_piece_drop: 1.0,
            lua_gold: 1.0,
            lua_attack: 0,
            lua_defense: 0,
            lua_cheer_exp: 1.0,
            lua_weather_exp: 1.0,
            shop_buy: 1.0,
            shop_sell: 1.0,
            coupon_ms: 0,
            keep_connect_ms: 0,
            keep_connect_items: Vec::new(),
            ..self.clone()
        }
    }
}

impl EventModifiers {
    /// The aggregates as JSON (the admin API's "list modifiers", 22 §4.13): the field names of 22 §4.3.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "lua_exp": self.lua_exp,
            "lua_item_drop": self.lua_item_drop,
            "lua_piece_drop": self.lua_piece_drop,
            "lua_gold": self.lua_gold,
            "lua_attack": self.lua_attack,
            "lua_defense": self.lua_defense,
            "lua_cheer_exp": self.lua_cheer_exp,
            "lua_weather_exp": self.lua_weather_exp,
            "shop_buy": self.shop_buy,
            "shop_sell": self.shop_sell,
            "coupon_ms": self.coupon_ms,
            "keep_connect_ms": self.keep_connect_ms,
            "keep_connect_items": self
                .keep_connect_items
                .iter()
                .map(|(item, count)| serde_json::json!({ "item": item, "count": count }))
                .collect::<Vec<_>>(),
            "generic_exp": self.generic_exp,
            "generic_item_drop": self.generic_item_drop,
            "lord_exp": self.lord_exp,
            "lord_item": self.lord_item,
            "weather_effect": self.weather_effect,
        })
    }
}

impl Default for EventModifiers {
    fn default() -> Self {
        EventModifiers::NEUTRAL
    }
}

impl EventFactors for EventModifiers {
    fn lua_exp(&self) -> f32 {
        self.lua_exp
    }
    fn lua_item_drop(&self) -> f32 {
        self.lua_item_drop
    }
    fn lua_piece_drop(&self) -> f32 {
        self.lua_piece_drop
    }
    fn lua_gold(&self) -> f32 {
        self.lua_gold
    }
    fn lua_attack(&self) -> i32 {
        self.lua_attack
    }
    fn lua_defense(&self) -> i32 {
        self.lua_defense
    }
    fn lua_cheer_exp(&self) -> f32 {
        self.lua_cheer_exp
    }
    fn lua_weather_exp(&self) -> f32 {
        self.lua_weather_exp
    }
    fn shop_buy(&self) -> f32 {
        self.shop_buy
    }
    fn shop_sell(&self) -> f32 {
        self.shop_sell
    }
    fn generic_exp(&self) -> f32 {
        self.generic_exp
    }
    fn generic_item_drop(&self) -> f32 {
        self.generic_item_drop
    }
    fn lord_exp(&self) -> f32 {
        self.lord_exp
    }
    fn lord_item(&self) -> f32 {
        self.lord_item
    }
    fn weather_effect(&self) -> bool {
        self.weather_effect
    }
}
