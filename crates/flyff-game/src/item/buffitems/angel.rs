//! Angel buff items (Plan/19 §5.12; ref/07c §6; KQ-17; `MoverSkill.cpp:1428-1466`, `DPSrvr.cpp:9496-9565`).
//!
//! * **Using an `IK3_ANGEL_BUFF` item** (`IK2_BUFF`): after `IsDoUseBuff` (`exclusive`; the item needs
//!   `events.pk`, a PK value above 0, the PK-minus quest and no finished angel in the bag - result 5 is the
//!   silent refusal of a server without PK, KQ-17) the buff is applied, the angel EXP is reset to 0, the angel
//!   level becomes `level + 1` and `Angel{info, exp, level, complete: false}` goes to the owner.
//! * **`AngelBuff`** (C2S): with the buff running and `angel_exp >= exp1(angel level) / 100 * n` (`n` is the
//!   buff item's first parameter, 100 outside `1..=100`; integer divisions, the `EXPINTEGER` of the original) the
//!   buff item turns into the matching 100 % angel (`II_SYS_SYS_QUE_ANGEL_{RED,BLUE,GREEN}` -> `..100`, any other
//!   -> `WHITE100`): created in the bag, then the buff is removed and the owner reads `TID_EVE_REAPITEM`; a full
//!   bag answers `TID_GAME_NOT_INVEN_ANGEL` and changes nothing. An angel level beyond the EXP table refuses
//!   (the original read past the array).
//! * [`send_info`] builds `Angel` for the EXP accrual of doc 16 (`stats::progression::add_angel_exp` sends the
//!   same message).

use super::with_player;
use crate::buff::core::{BuffCx, RemoveFlags, find_ik3, remove_buffs};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::SerialSource;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::model::Item;
use crate::item::ops::{ItemError, ItemOrigin, core as ops};
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_common::defs::item::*;
use flyff_common::defs::itemkind::IK3_ANGEL_BUFF;
use flyff_common::defs::text::{TID_EVE_REAPITEM, TID_GAME_NOT_INVEN_ANGEL};
use flyff_proto::pb;
use flyff_rules::item::buffitem::angel_goal;

/// `ANGEL_INFO`: the only state the client handles.
pub const ANGEL_INFO: u32 = 1;

/// `AddAngelInfo(bComplete)`: the angel EXP and level to the owner.
pub fn send_info(m: &Mover, complete: bool, fx: &mut Fx) {
    if let Some(p) = m.player() {
        fx.out.to_self(pb::Angel {
            state: ANGEL_INFO,
            angel_exp: p.progression.angel_exp,
            angel_level: p.progression.angel_level,
            complete,
        });
    }
}

/// The use of an angel buff item: a new angel starts (`m_nAngelExp = 0`, `m_nAngelLevel = level + 1`).
pub fn on_summon(m: &mut Mover, fx: &mut Fx) {
    let level = m.mv.level;
    if let Some(p) = m.player_mut() {
        p.progression.angel_exp = 0;
        p.progression.angel_level = level.wrapping_add(1);
    }
    send_info(m, false, fx);
}

/// How an [`angel_buff`] request ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngelOutcome {
    /// No angel buff is running, or its item has no prop.
    NoBuff,
    /// The angel EXP has not reached the goal.
    NotEnough,
    /// The bag is full: `TID_GAME_NOT_INVEN_ANGEL`.
    NoSpace,
    /// The 100 % angel item was created.
    Created(u32),
}

/// The 100 % angel item of the colour of the buff item `buff_item`.
pub fn finished_angel(buff_item: u32) -> u32 {
    match buff_item {
        II_SYS_SYS_QUE_ANGEL_RED => II_SYS_SYS_QUE_ANGEL_RED100,
        II_SYS_SYS_QUE_ANGEL_BLUE => II_SYS_SYS_QUE_ANGEL_BLUE100,
        II_SYS_SYS_QUE_ANGEL_GREEN => II_SYS_SYS_QUE_ANGEL_GREEN100,
        _ => II_SYS_SYS_QUE_ANGEL_WHITE100,
    }
}

/// `OnAngleBuff` over one mover.
pub fn angel_buff(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    now: GameMs,
    serials: &mut dyn SerialSource,
    fx: &mut Fx,
) -> AngelOutcome {
    let data = env.data();
    let Some(key) = find_ik3(m, data, IK3_ANGEL_BUFF) else {
        return AngelOutcome::NoBuff;
    };
    let Some(prop) = data.item(u32::from(key.id)) else {
        return AngelOutcome::NoBuff;
    };
    let percent = prop.adj_param_val[0];
    let Some(p) = m.player() else {
        return AngelOutcome::NoBuff;
    };
    let goal = env
        .stats()
        .rules
        .exp_row(p.progression.angel_level)
        .map(|r| angel_goal(r.exp1, percent));
    let Some(goal) = goal else {
        return AngelOutcome::NotEnough;
    };
    if p.progression.angel_exp < goal {
        return AngelOutcome::NotEnough;
    }
    let angel = finished_angel(prop.id);
    match ops::create_item(m, env, serials, Item::new(angel, 1), ItemOrigin::Angel, fx) {
        Ok(_) => {
            let cx = BuffCx::new(*env, now);
            remove_buffs(m, &cx, RemoveFlags::IK3, IK3_ANGEL_BUFF, fx);
            let name = data.item(angel).map_or(String::new(), |p| p.name.clone());
            fx.defined(TID_EVE_REAPITEM, vec![format!("\"{name}\"")]);
            fx.mark_urgent();
            AngelOutcome::Created(angel)
        }
        Err(ItemError::LackSpace) => {
            fx.text(TID_GAME_NOT_INVEN_ANGEL);
            AngelOutcome::NoSpace
        }
        Err(_) => AngelOutcome::NoBuff,
    }
}

/// `AngelBuff` (C2S, no payload).
pub fn on_angel_buff(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, _: pb::AngelBuff) {
    let now = ctx.clock.game_ms;
    with_player(svc, ctx, who, |m, env, serials, fx| {
        angel_buff(m, env, now, serials, fx)
    });
}
