//! Buff-dependent item effects (Plan/19 §5.3-§5.5, §12.2-§12.4, §12.7, §13.2; WP-ITEM-20; ref/13 §10.2-§10.3;
//! `MoverSkill.cpp:1424-1727, 1897-3035`): the items whose use is a buff, depends on one, or is refused by one.
//!
//! | module | content |
//! |---|---|
//! | [`exclusive`] | `IsDoUseBuff`: the exclusivity of item buffs, the angel rules (KQ-17) and the queries `has_item` / `has_item2` |
//! | [`ik2`] | the `IK2` rows `BUFF`, `BUFF_TOGIFT`, `BUFF2`, `AIRFUEL`, `GMTEXT`, `SKILL` and the generic timed buff |
//! | [`system`] | the `IK2_SYSTEM` rows: protection/smelting/awakening-protection/`GET` scrolls, balloons, `HOLY`, `SELPAR`, the collector battery |
//! | [`ampes`] | the EXP scrolls with their level bands and tiers |
//! | [`pets`] | the pet tonics, the pet feed pocket, `FeedPocketInactive` |
//! | [`return_scroll`] | the return scroll and the trip back (`ReturnScrollReq{-1}`) |
//! | [`angel`] | angel buff items, `AngelBuff`, `Angel` |
//! | [`later`] | what a row defers to the services (`Followup::BuffItem`) |
//!
//! **How the rows plug in.** [`usage::dispatch_ik2`](crate::item::usage::dispatch_ik2) asks
//! [`dispatch_ik2`] first and [`usage::system::use_system`](crate::item::usage::system::use_system) asks
//! [`dispatch_system`] after its level gate; both answer `None` for what is not theirs. The rows work on one
//! `&mut Mover`: the item buff itself (`DoApplySkill(this, prop, NULL)`) is applied by the services when the
//! `Fx` is flushed ([`Later::ApplyItem`], which calls [`crate::buff::apply_item`]), after the messages of the
//! use (the consumption `UpdateItem` precedes the `SetSkillState`).
//!
//! **Who reads the buffs.** The legacy upgrade path (`item::upgrade`) reads and removes `SMELPROT`, `SMELTING`,
//! `SUPERSMELTING`, `SMELPROT3/4`, `SMELTING2` and `AWAKESAFE`; the drop passes read `GET01/02`; the pet processing
//! (`item::pets::system`) reads the tonics, `SPETGOOD` and the feed pockets; the collecting pass reads the three
//! battery buffs; doc 16 reads the EXP scroll tiers. This module only creates them, with the exclusivity of the
//! original.

pub mod ampes;
pub mod angel;
pub mod exclusive;
pub mod ik2;
pub mod later;
pub mod pets;
pub mod return_scroll;
pub mod system;

pub use later::Later;

pub(crate) use super::pockets::with_player;

use super::env::ItemEnv;
use super::fx::Fx;
use super::usage::system::Sys;
use super::usage::{RowCx, RowResult};
use crate::ctx::{GameCtx, Services};
use crate::mover::Mover;
use flyff_data::props::item::ItemProp;

/// The `IK2` rows of this module (`None` = not one of them).
pub fn dispatch_ik2(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> Option<RowResult> {
    ik2::dispatch(m, env, prop, ux, fx)
}

/// The `IK2_SYSTEM` rows of this module (`None` = not one of them).
pub fn dispatch_system(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> Option<Sys> {
    system::dispatch(m, env, prop, ux, fx)
}

/// The job of one tick (called by [`crate::item::tick::process`]): keeps `PlayerItems::active_feed_pocket` in
/// step with the feed pocket buff.
pub fn tick(_svc: &mut Services, ctx: &mut GameCtx<'_>) {
    pets::tick(ctx);
}
