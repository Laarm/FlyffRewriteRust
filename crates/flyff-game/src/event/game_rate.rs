//! `event::game_rate`: the `GameRate` message (Plan/22 §4.5; WP-EVENT-04 sends the shop kinds, WP-EVENT-05 the join
//! set).
//!
//! `GameRate{rate, kind}` carries one rate of the server to the client (shop prices, the skill-point constants, and
//! for game masters the drop/EXP rates). The kinds are the legacy `GAMERATE_*` ids of `_Network/MsgHdr.h:1514-1529`.
//!
//! * **Join** ([`GameRateStep`], after `SmModeAll`; legacy `CUser::AddGameSetting`, src:WORLDSERVER/User.cpp:6533-6558):
//!   `SHOPCOST`, the three skill-point constants, `SHOP_BUY`, `SHOP_SELL` (6 messages); a game master (authority
//!   `GAMEMASTER` or higher) additionally `ITEMDROP`, `GOLDDROP`, `MONSTEREXP`, `MONSTERHIT`, `REBIRTH` 1.0, `HITPOINT`
//!   1.0, `AGGRESSIVE` (13 in total). Unset `[rates]` are 1.0. [`send_join_set`] repeats the set for one player (`/gs`).
//! * **Change** ([`broadcast`]): one rate to everybody who is in a world (`CUserMng::AddGameRate`): the Event.lua shop
//!   factors when they change (done by the P9 job) and doc 23's `rates.*` consumer.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::event::EventFactors;
use crate::live::LiveConfig;
use crate::outbox::{Audience, Outbox};
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::ObjId;
use flyff_common::gm::Authority;
use flyff_data::GameData;
use flyff_proto::pb;
use smallvec::smallvec;

/// `GAME_RATE_SHOPCOST`: the shop price multiplier (`rates.shop_cost`).
pub const SHOPCOST: u8 = 0x00;
/// `GAME_RATE_ITEMDROP`.
pub const ITEMDROP: u8 = 0x01;
/// `GAME_RATE_GOLDDROP`.
pub const GOLDDROP: u8 = 0x02;
/// `GAME_RATE_MONSTEREXP`.
pub const MONSTEREXP: u8 = 0x03;
/// `GAME_RATE_MONSTERHIT`.
pub const MONSTERHIT: u8 = 0x04;
/// `GAME_RATE_X3` (unused).
pub const X3: u8 = 0x05;
/// `GAME_RATE_REBIRTH`.
pub const REBIRTH: u8 = 0x06;
/// `GAME_RATE_HITPOINT`.
pub const HITPOINT: u8 = 0x07;
/// `GAME_RATE_AGGRESSIVE`.
pub const AGGRESSIVE: u8 = 0x08;
/// `GAME_RATE_RESPAWN` (unused).
pub const RESPAWN: u8 = 0x09;
/// `GAME_RATE_SKILL_VAGSP`: skill-point constant of the vagrant.
pub const SKILL_VAGSP: u8 = 0x10;
/// `GAME_RATE_SKILL_EXPERTSP`.
pub const SKILL_EXPERTSP: u8 = 0x11;
/// `GAME_RATE_SKILL_PROSP`.
pub const SKILL_PROSP: u8 = 0x12;
/// `GAME_RATE_SHOP_BUY`: the Event.lua shop buy factor.
pub const SHOP_BUY: u8 = 0x13;
/// `GAME_RATE_SHOP_SELL`: the Event.lua shop sell factor.
pub const SHOP_SELL: u8 = 0x14;

/// The message for one rate.
pub fn message(kind: u8, rate: f32) -> pb::GameRate {
    pb::GameRate {
        rate,
        kind: u32::from(kind),
    }
}

/// `CUserMng::AddGameRate`: sends one rate to everybody who is in a world (a GM rate change, a change of the
/// Event.lua shop factors; doc 23 calls this for `rates.*` changes).
pub fn broadcast(ctx: &mut GameCtx<'_>, kind: u8, rate: f32) {
    ctx.broadcast(None, ObjId(0), Audience::AllOnline, message(kind, rate));
}

/// The rates of the join set in the legacy order: `(kind, rate)`. `gm` adds the seven game-master rates.
pub fn join_rates(
    live: &LiveConfig,
    data: &GameData,
    events: &dyn EventFactors,
    gm: bool,
) -> Vec<(u8, f32)> {
    let r = &live.rates;
    let mut v = vec![
        (SHOPCOST, r.shop_cost.unwrap_or(1.0)),
        (SKILL_VAGSP, data.constant.vag_sp as f32),
        (SKILL_EXPERTSP, data.constant.expert_sp as f32),
        (SKILL_PROSP, data.constant.pro_sp as f32),
        (SHOP_BUY, events.shop_buy()),
        (SHOP_SELL, events.shop_sell()),
    ];
    if gm {
        v.extend([
            (ITEMDROP, r.item_drop.unwrap_or(1.0)),
            (GOLDDROP, r.gold_drop.unwrap_or(1.0)),
            (MONSTEREXP, r.monster_exp.unwrap_or(1.0)),
            (MONSTERHIT, r.monster_hit.unwrap_or(1.0)),
            // the back-end system rates are not configurable (F-014)
            (REBIRTH, 1.0),
            (HITPOINT, 1.0),
            (AGGRESSIVE, r.monster_aggressive),
        ]);
    }
    v
}

/// `true` when `authority` receives the game-master rates (`IsAuthHigher(AUTH_GAMEMASTER)`).
fn is_gm(authority: Authority) -> bool {
    authority.at_least(Authority::GAMEMASTER)
}

/// `GameRate` x 6 (+7 for game masters) of the join batch (`JoinStep::GameRate`, 11 §2.5).
pub struct GameRateStep;

impl JoinContributor for GameRateStep {
    fn step(&self) -> JoinStep {
        JoinStep::GameRate
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let gm = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .is_some_and(|p| is_gm(p.ident.authority));
        for (kind, rate) in join_rates(cx.live, cx.data, cx.svc.event.factors(), gm) {
            out.to_session(cx.session, 0, message(kind, rate));
        }
    }
}

/// `/gs` (`CUser::AddGameSetting`): the join set again, to `p` only.
pub fn send_join_set(s: &Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let gm = ctx
        .player(p)
        .and_then(|m| m.player())
        .is_some_and(|pl| is_gm(pl.ident.authority));
    for (kind, rate) in join_rates(ctx.live, ctx.data, s.event.factors(), gm) {
        ctx.broadcast(
            Some(s),
            ObjId(0),
            Audience::Players(smallvec![p.0]),
            message(kind, rate),
        );
    }
}
