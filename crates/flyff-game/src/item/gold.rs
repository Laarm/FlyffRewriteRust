//! Gold arithmetic and the Perin converter (Plan/19 §4.3-§4.4, WP-ITEM-03; VR-011, PL-075, KQ-15;
//! src:_Common/Mover.cpp:584-638, WORLDSERVER/User.cpp:2184-2236).
//!
//! Gold is an `i64` in all arithmetic and clamped to `[0, GOLD_MAX]` when stored (`AddGold` with
//! `__PERIN_BUY_BUG`), while every "at least 2 100 000 000" business check of the original is kept literally.
//! Gold changes go to the owner only (PL-075, `fidelity.gold_update_self_only`).

use super::container::SerialSource;
use super::env::ItemEnv;
use super::fx::Fx;
use super::model::Item;
use super::ops::{ItemOrigin, RemoveReason, core as ops};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::pool::emit_gold;
use flyff_common::defs::item::II_SYS_SYS_SCR_PERIN;
use flyff_common::defs::text::{
    TID_GAME_MAKEPERIN, TID_GAME_MAKEPERIN_FAILURE, TID_GAME_REAPMONEY,
};

/// `INT_MAX`: the clamp of `AddGold`.
pub const GOLD_MAX: i64 = 2_147_483_647;
/// The threshold of shop sell, quick sell and the Perin converter (21 Perins = 2 100 000 000).
pub const GOLD_BUSINESS_CAP: i64 = 2_100_000_000;
/// `PERIN_VALUE`: the penya a Perin is worth (`Resource/define.h:263`).
pub const PERIN_VALUE: i64 = 100_000_000;
/// Using a Perin is refused at or above this wallet.
pub const PERIN_USE_CAP: i64 = 2_000_000_000;
/// The most gold one mail can carry.
pub const MAIL_GOLD_CAP: i64 = 2_000_000_000;
/// How many Perins the converter makes at once.
pub const PERIN_CONVERT_COUNT: u16 = 21;

/// `CanAdd(current, plus)`: a positive `plus` that does not push the wallet past `GOLD_MAX` (the original's
/// `int` overflow test).
pub fn can_add(current: i64, plus: i64) -> bool {
    plus > 0 && current + plus <= GOLD_MAX
}

/// `GetNumberFormatEx`: the number with thousands commas (`2,100,000,000`).
pub fn format_number(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    let first = digits.len() % 3;
    for (i, c) in digits.chars().enumerate() {
        if i != 0 && (i + 3 - first).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The mover-level functions.
pub mod core {
    use super::*;

    /// `AddGold(delta, send)`: `gold = clamp(gold + delta, 0, GOLD_MAX)`; 0 is a no-op; with `send` a
    /// `SetPointParam{DST_GOLD}` goes to the owner (or to the near set when
    /// `fidelity.gold_update_self_only` is off).
    pub fn add_gold(m: &mut Mover, env: &ItemEnv<'_>, delta: i64, send: bool, fx: &mut Fx) {
        if delta == 0 {
            return;
        }
        let Some(p) = m.player_mut() else { return };
        let total = (i64::from(p.items.gold) + delta).clamp(0, GOLD_MAX);
        p.items.gold = total as i32;
        if send {
            emit_gold(m, env.stats(), &mut fx.out);
        }
    }

    /// The wallet as an `i64`.
    pub fn gold(m: &Mover) -> i64 {
        m.player().map_or(0, |p| i64::from(p.items.gold))
    }

    /// `GetPerinNum`: the usable Perins in the inventory.
    pub fn perin_count(m: &Mover) -> u32 {
        m.player().map_or(0, |p| {
            p.items
                .inventory
                .iter()
                .filter(|(_, it)| it.is_perin() && it.is_usable())
                .map(|(_, it)| u32::from(it.count))
                .sum()
        })
    }

    /// `PickupGoldCore(gold)` (src:_Common/MoverEquip.cpp:2395-2420): the wallet grows (message to the owner),
    /// the converter and the gold line run (`AddGoldText`), and amounts of 500 and more are logged
    /// (`item.pickup`).
    pub fn pickup_gold_core(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        serials: &mut dyn SerialSource,
        amount: i64,
        from_monster: bool,
        fx: &mut Fx,
    ) {
        add_gold(m, env, amount, true, fx);
        gold_text(m, env, serials, amount, fx);
        if amount >= 500 {
            fx.audit(
                flyff_db::audit::ev::ITEM_PICKUP,
                serde_json::json!({ "gold": amount, "from_monster": from_monster }),
            );
        }
    }

    /// `AddGoldText(plus)` (`CUser::AddGoldText`): with `features.auto_perin_converter` and a wallet above
    /// 2 100 000 000 the four cases of 19 §4.4 (the `perins == 1` case does nothing - KEEP-QUIRK), then the
    /// `TID_GAME_REAPMONEY` line with the gained and the total amount.
    pub fn gold_text(
        m: &mut Mover,
        env: &ItemEnv<'_>,
        serials: &mut dyn SerialSource,
        plus: i64,
        fx: &mut Fx,
    ) {
        if env.cfg().auto_perin_converter && gold(m) > GOLD_BUSINESS_CAP {
            let perins = perin_count(m);
            let empty = m.player().map_or(0, |p| p.items.inventory.empty_count());
            let convert = |m: &mut Mover,
                           serials: &mut dyn SerialSource,
                           fx: &mut Fx,
                           count: u32,
                           remove: u32| {
                if remove > 0 {
                    ops::remove_item_by_id(
                        m,
                        env,
                        II_SYS_SYS_SCR_PERIN,
                        Some(remove),
                        RemoveReason::Other("perin_convert"),
                        fx,
                    );
                }
                let mut perin = Item::new(II_SYS_SYS_SCR_PERIN, count as u16);
                perin.charged = true;
                // the result is not checked before the gold goes (KEEP-QUIRK, unreachable below 9 978 Perins)
                let _ = ops::create_item(m, env, serials, perin, ItemOrigin::Perin, fx);
                add_gold(m, env, -GOLD_BUSINESS_CAP, true, fx);
                fx.text(TID_GAME_MAKEPERIN);
                fx.mark_urgent();
            };
            if empty == 0 && perins > 0 {
                convert(
                    m,
                    serials,
                    fx,
                    perins + u32::from(PERIN_CONVERT_COUNT),
                    perins,
                );
            } else if empty > 0 && (perins == 0 || perins > 1) {
                convert(m, serials, fx, u32::from(PERIN_CONVERT_COUNT), 0);
            } else if empty == 0 && perins == 0 {
                fx.text(TID_GAME_MAKEPERIN_FAILURE);
            }
        }
        let total = gold(m);
        fx.defined(
            TID_GAME_REAPMONEY,
            vec![format_number(plus), format_number(total)],
        );
    }
}

/// `add_gold(g, p, delta, send)` (Plan/19 §4.3).
pub fn add_gold(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef, delta: i64, send: bool) {
    let core_env = svc.items.handle();
    let env = core_env.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(p.0) {
        core::add_gold(m, &env, delta, send, &mut fx);
    }
    fx.flush(svc, ctx, p);
}

pub mod total;
