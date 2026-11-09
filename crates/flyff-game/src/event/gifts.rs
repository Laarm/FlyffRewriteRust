//! `event::gifts`: the Event.lua level-up gifts (Plan/22 §4.6, WP-EVENT-05; legacy `CEventLua::SetLevelUpGift`).
//!
//! Doc 16 calls [`on_level_up`] once for every level a character reaches **for the first time** (not for a level it
//! regains after a death penalty: `death_level < level`), after the skill points and before the messages of the
//! level-up. The gifts come from the `Event` family (`GetLevelUpGift(level, account)`, running events only); for each
//! one in script order:
//!
//! 1. an unknown item is skipped with a warning (the wrappers do that),
//! 2. doc 19's [`create_item_or_mail`] puts the item into the inventory (`ItemOrigin::LevelUpGift`, audited as
//!    `item.create`); a full inventory sends it as a system mail instead (the legacy
//!    `SendQueryPostMail(id, 0, item, 0, "", "")`: no title, no text; audited as `mail.send`),
//! 3. `DefinedText(TID_GAME_EVENT_LEVELUP_GIFT, "\"<item name>\"")` tells the player, on both paths.
//!
//! **Account string.** The legacy script is called with the login name of the account (`m_playAccount`), which the
//! game thread does not hold; the decimal account id stands in for it. The shipped gifts are addressed to `"all"`,
//! so nothing changes; a pattern aimed at login names never matches.

use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::create_item_or_mail;
use crate::item::model::{Item, ItemFlags};
use crate::item::ops::ItemOrigin;
use flyff_common::defs::text::TID_GAME_EVENT_LEVELUP_GIFT;
use flyff_data::types::ItemId;

/// `event::on_level_up` (22 §2.2, §4.6): the gifts for reaching `level`.
pub fn on_level_up(s: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef, level: u32) {
    let Some(account) = ctx
        .player(p)
        .and_then(|m| m.player())
        .map(|pl| pl.ident.account_id.0.to_string())
    else {
        return;
    };
    let gifts = s
        .event
        .lua_events
        .level_up_gifts(ctx.lua, ctx.data, level, &account);
    for g in gifts {
        let name = ctx
            .data
            .item(g.item)
            .map(|prop| prop.name.clone())
            .unwrap_or_default();
        let item = gift_item(g.item, g.count, g.flag);
        let delivered = match create_item_or_mail(
            s,
            ctx,
            p,
            item,
            ItemOrigin::LevelUpGift,
            None,
            None,
        ) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(item = g.item, error = %e, "level-up gift: the item could not be delivered");
                false
            }
        };
        if delivered {
            notice::defined(
                ctx,
                p,
                TID_GAME_EVENT_LEVELUP_GIFT,
                vec![format!("\"{name}\"")],
            );
        }
    }
}

/// The item of one gift: `count` (at least 1) of `item` with the script's `byFlag`.
fn gift_item(item: ItemId, count: i32, flag: u8) -> Item {
    let count = u16::try_from(count.clamp(1, i32::from(u16::MAX))).unwrap_or(1);
    let mut it = Item::new(item, count);
    it.flags = ItemFlags(flag);
    it
}
