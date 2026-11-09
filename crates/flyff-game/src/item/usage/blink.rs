//! Blink wings, couple rings, the escape command and the return-village selection (Plan/19 §5.7, WP-ITEM-10 and
//! the escape of WP-ITEM-21; `CMover::DoUseItemBlinkWing` / `DoUseItemWarp` / `DoUseItemVirtual`,
//! `MoverSkill.cpp:1751-1895, 3833-3935`; `CDPSrvr::OnDoEscape` / `OnReturnScroll`).
//!
//! # Blink wings (`IK2_BLINKWING`)
//!
//! Refused (silently unless noted) while sitting or in the base-motion state, in Kebaras (when used from the
//! inventory), the guild-war world, the 1:1 guild combat (`TID_GAME_GUILDCOMBAT1TO1_NOBLINKWING`), the secret
//! room (`TID_GAME_SECRETROOM_USEBRINKITEM`), a rainbow race (`TID_GAME_RAINBOWRACE_NOTELEPORT`), the mini-room
//! and below the level limit (`TID_GAME_USINGNOTLEVEL`). The wing is consumed **before** the teleport. A town
//! wing leads to the revival point of the world ([`Followup::BlinkTown`]; the escape wing forces the
//! teleport), any other wing to the world and spot of its prop (`weapon_type`, `item_atk_order`), a chaotic
//! player to the chaotic revival point instead. The refusals of the cast time (`TID_GAME_NOTSIT_BLINK`) are
//! the cast's (`usage::cast`).
//!
//! # Couple rings (`IK2_WARP`)
//!
//! Never consumed. A blank ring is carved with the character id of its user (`UpdateItemEx(RANDOMOPTITEMID)`;
//! the `II_GEN_WARP_COUPLERING` also gets a 15 day keep time) and says `TID_GAME_COUPLERING_CARVE`; one's own
//! ring says `TID_GAME_COUPLERING_ERR01`; a ring of another player teleports the user to the carver when the
//! rules of [`run_couple_ring`] allow it.
//!
//! # Escape and the return village
//!
//! `DoEscape` is the virtual use of `II_CHR_SYS_SCR_ESCAPEBLINKWING` (the cast time first, then `SM_ESCAPE` for
//! its `circle_time` seconds and a forced town blink); while `SM_ESCAPE` runs it says `TID_GAME_STILLNOTUSE`.
//! No item is consumed. `ReturnScrollReq{0..2}` stores the village of the next return scroll and answers
//! `ReturnScrollNtf`; `-1` (the trip back, WP-ITEM-20) is not handled here.

use super::{RowCx, RowResult, UseFacts, cast};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::ops::{RemoveReason, UiParam, core as ops};
use crate::mover::Mover;
use crate::stats::sm::sm_active;
use flyff_common::defs::item::{
    II_CHR_SYS_SCR_ESCAPEBLINKWING, II_GEN_WARP_COUPLERING, II_GEN_WARP_COUPLERING01,
    II_GEN_WARP_WEDDING_BAND,
};
use flyff_common::defs::itemkind::IK3_TOWNBLINKWING;
use flyff_common::defs::text::{
    TID_DIAG_0061, TID_GAME_COUPLERING_CARVE, TID_GAME_COUPLERING_ERR01, TID_GAME_COUPLERING_ERR02,
    TID_GAME_COUPLERING_ERR03, TID_GAME_GUILDCOMBAT1TO1_NOBLINKWING,
    TID_GAME_RAINBOWRACE_NOTELEPORT, TID_GAME_SECRETROOM_USEBRINKITEM, TID_GAME_STILLNOTUSE,
    TID_GAME_USINGNOTLEVEL,
};
use flyff_common::defs::world::{
    WI_WORLD_GUILDWAR, WI_WORLD_GUILDWAR1TO1_0, WI_WORLD_GUILDWAR1TO1_L, WI_WORLD_KEBARAS,
    WI_WORLD_MINIROOM, WI_WORLD_NONE, WI_WORLD_QUIZ,
};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GameMs, LayerId, ObjId, WorldId};
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::consts::{NULL_ID, SM_ESCAPE, STATE_BASEMOTION_MODE};

use crate::world::ReplaceKind;

/// The villages of `ReturnScrollReq{0..2}` (`szPos[]` of `OnReturnScroll`): the keys of the revival points.
pub const RETURN_VILLAGES: [&str; 3] = ["flaris", "saintmorning", "darkon"];

/// The keep time of a carved `II_GEN_WARP_COUPLERING` in minutes (15 days).
pub const COUPLE_RING_KEEP_MINUTES: u32 = 21_600;

/// `true` for the worlds of the 1:1 guild combat.
fn is_gc1to1_world(world: u32) -> bool {
    (WI_WORLD_GUILDWAR1TO1_0..=WI_WORLD_GUILDWAR1TO1_L).contains(&world)
}

/// The use of one item from the inventory: `count - 1` and `UpdateItem(UI_NUM)`, the audit line of a premium
/// item that is used up. (`pItemElem->UseItem(); UpdateItem(..., UI_NUM, ...)`.)
pub fn consume_one(m: &mut Mover, env: &ItemEnv<'_>, id: StorageId, fx: &mut Fx) {
    let Some(item) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) else {
        return;
    };
    let new = item.count.saturating_sub(1);
    if new == 0 && item.is_charged() {
        fx.audit(
            flyff_db::audit::ev::ITEM_DESTROY,
            serde_json::json!({
                "item": crate::item::fx::item_json(item),
                "count": 1,
                "reason": RemoveReason::Consume.tag(),
            }),
        );
    }
    ops::update_item(m, env, id, UiParam::Num, u32::from(new), 0, fx);
}

// ---- blink wings -------------------------------------------------------------------------------------------------

/// `DoUseItemBlinkWing(prop, item, bUse)`: `consume` is the wing in the inventory (`bUse == TRUE`), `None` for
/// the virtual use of the escape command. Returns `true` when the wing worked.
fn blink_core(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    facts: &UseFacts,
    consume: Option<StorageId>,
    fx: &mut Fx,
) -> bool {
    if m.player().is_none() || m.sim.act.flags.sit() {
        return false;
    }
    if m.mv.state_mode & STATE_BASEMOTION_MODE != 0 {
        return false;
    }
    let world = m.core.world.0;
    if consume.is_some() && world == WI_WORLD_KEBARAS {
        return false;
    }
    if world == WI_WORLD_GUILDWAR {
        return false;
    }
    if facts.gc1to1 {
        fx.text(TID_GAME_GUILDCOMBAT1TO1_NOBLINKWING);
        return false;
    }
    if facts.secret_room {
        fx.text(TID_GAME_SECRETROOM_USEBRINKITEM);
        return false;
    }
    if facts.rainbow_entrant {
        fx.text(TID_GAME_RAINBOWRACE_NOTELEPORT);
        return false;
    }
    if world == WI_WORLD_MINIROOM {
        return false;
    }
    if prop.limit_level1 != NULL_ID && i64::from(prop.limit_level1) > i64::from(m.mv.level) {
        fx.defined(TID_GAME_USINGNOTLEVEL, vec![format!("\"{}\"", prop.name)]);
        return false;
    }
    let me = m.core.id;
    if prop.item_kind3 == IK3_TOWNBLINKWING {
        if let Some(id) = consume {
            if !m.player().is_some_and(|p| {
                p.items
                    .inventory
                    .get_at_id(id)
                    .is_some_and(|it| it.is_usable())
            }) {
                return false;
            }
            consume_one(m, env, id, fx);
        }
        fx.then(Followup::BlinkTown {
            mover: me,
            force: prop.id == II_CHR_SYS_SCR_ESCAPEBLINKWING,
        });
        return true;
    }
    // a wing with a destination of its own: x / y / z / angle in `item_atk_order`, the world in `weapon_type`
    if prop.weapon_type == WI_WORLD_NONE || prop.weapon_type == u32::MAX {
        return false;
    }
    let pos = Vec3::new(
        prop.item_atk_order[0] as f32,
        prop.item_atk_order[1] as f32,
        prop.item_atk_order[2] as f32,
    );
    let angle = flyff_sim::num::set_angle_quantized(prop.item_atk_order[3] as f32);
    m.sim.angle = angle;
    m.core.angle = angle;
    if let Some(id) = consume {
        if !m.player().is_some_and(|p| {
            p.items
                .inventory
                .get_at_id(id)
                .is_some_and(|it| it.is_usable())
        }) {
            return false;
        }
        consume_one(m, env, id, fx);
    }
    fx.then(Followup::BlinkFixed {
        mover: me,
        world: WorldId(prop.weapon_type),
        pos,
        chaotic: crate::combat::state::is_chaotic(m),
        key: prop.text_file.clone(),
    });
    true
}

/// `IK2_BLINKWING` row. A wing that worked did everything itself (consumption included).
pub fn use_blink_wing(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    if blink_core(m, env, prop, ux.facts, Some(ux.id), fx) {
        RowResult::Handled
    } else {
        RowResult::Refused
    }
}

/// The town blink (a [`Followup::BlinkTown`]): the revival point of the world, else the nearest, else the same
/// spot on the default layer; layer 0 in every case.
pub fn run_town(ctx: &mut GameCtx<'_>, mover: ObjId, force: bool) {
    let Some(m) = ctx.worlds.mover(mover) else {
        return;
    };
    let (world, pos, chaotic) = (
        m.core.world,
        m.core.pos,
        crate::combat::state::is_chaotic(m),
    );
    let settings = ctx.worlds.world(world).map(|w| w.statics.settings.clone());
    let dest = settings.and_then(|s| ctx.worlds.revival.select(world, &s, pos, chaotic));
    let (w, p) = dest.map_or((world, pos), |d| (d.world, d.pos));
    let kind = if force {
        ReplaceKind::Force
    } else {
        ReplaceKind::Normal
    };
    if let Err(e) = ctx.worlds.replace(mover, w, p, LayerId(0), kind) {
        tracing::debug!(mover = mover.0, "town blink refused: {e}");
    }
}

/// The blink to a fixed destination (a [`Followup::BlinkFixed`]); a chaotic player goes to the chaotic revival
/// point of `key` in `world`, else to the nearest chaotic one, and nowhere when there is none.
pub fn run_fixed(
    ctx: &mut GameCtx<'_>,
    mover: ObjId,
    world: WorldId,
    pos: Vec3,
    chaotic: bool,
    key: &str,
) {
    let (w, p) = if chaotic {
        let here = ctx.worlds.mover(mover).map_or(pos, |m| m.core.pos);
        let found = ctx
            .worlds
            .revival
            .revival_pos_chao(world, key)
            .or_else(|| ctx.worlds.revival.near_revival_pos_chao(world, here));
        match found {
            Some(r) => (r.world, r.pos),
            None => return,
        }
    } else {
        (world, pos)
    };
    if let Err(e) = ctx
        .worlds
        .replace(mover, w, p, LayerId(0), ReplaceKind::Normal)
    {
        tracing::debug!(mover = mover.0, "blink refused: {e}");
    }
}

// ---- couple rings ------------------------------------------------------------------------------------------------

/// `DoUseItemWarp`: the `IK2_WARP` row (the three couple rings; any other warp item does nothing).
pub fn use_warp(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    if !matches!(
        prop.id,
        II_GEN_WARP_COUPLERING | II_GEN_WARP_WEDDING_BAND | II_GEN_WARP_COUPLERING01
    ) {
        return RowResult::Refused;
    }
    if m.sim.act.flags.fly() {
        return RowResult::Refused;
    }
    let Some(p) = m.player() else {
        return RowResult::Refused;
    };
    let me = p.ident.character_id.0;
    let name = p.ident.name.clone();
    let Some(partner) = p
        .items
        .inventory
        .get_at_id(ux.id)
        .map(|it| it.couple_partner_id.unwrap_or(0))
    else {
        return RowResult::Refused;
    };
    if partner == me {
        fx.defined(TID_GAME_COUPLERING_ERR01, vec![name]);
        return RowResult::Refused;
    }
    if partner > 0 {
        fx.then(Followup::CoupleRing {
            player: m.core.id,
            partner,
        });
        return RowResult::Handled;
    }
    // carve the ring with the user's id
    if let Some(it) = m
        .player_mut()
        .and_then(|p| p.items.inventory.get_at_id_mut(ux.id))
    {
        it.couple_partner_id = Some(me);
    }
    fx.out.to_self(pb::UpdateItemEx {
        obj_id: u32::from(ux.id),
        param: UiParam::RandomOptItemId as i32,
        value: i64::from(me),
    });
    if prop.id == II_GEN_WARP_COUPLERING {
        ops::update_item(
            m,
            env,
            ux.id,
            UiParam::KeepTime,
            COUPLE_RING_KEEP_MINUTES,
            0,
            fx,
        );
    }
    fx.defined(TID_GAME_COUPLERING_CARVE, vec![name]);
    RowResult::Handled
}

/// The teleport of a ring carved by `partner` (a [`Followup::CoupleRing`], `SendTeleportPlayer` of the original
/// folded into one step): the carver must be online (`TID_DIAG_0061 [name]`), not flying
/// (`TID_GAME_COUPLERING_ERR03`), neither of the two in the guild-war world or a 1:1 guild-combat world
/// (silent), and both in the same world and layer, outside the quiz world (else `TID_GAME_COUPLERING_ERR02
/// [name]`). A carver unknown to the directory does nothing.
pub fn run_couple_ring(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, partner: u32) {
    let Some(entry) = svc.presence.get(CharacterId(partner)) else {
        return;
    };
    let partner_name = entry.name.clone();
    let online = entry.online;
    let mut fx = Fx::new();
    let me = ctx
        .worlds
        .mover(who.0)
        .map(|m| (m.core.world, m.core.layer, m.core.pos));
    let other = online.and_then(|o| {
        ctx.worlds.mover(o.obj).map(|m| {
            (
                o.obj,
                m.core.world,
                m.core.layer,
                m.core.pos,
                m.sim.act.flags.fly(),
            )
        })
    });
    let (Some((my_world, my_layer, _)), Some(other)) = (me, other) else {
        if me.is_some() {
            fx.defined(TID_DIAG_0061, vec![partner_name]);
            fx.flush(svc, ctx, who);
        }
        return;
    };
    let (_, their_world, their_layer, their_pos, their_fly) = other;
    if their_fly {
        fx.text(TID_GAME_COUPLERING_ERR03);
    } else if their_world.0 == WI_WORLD_GUILDWAR
        || my_world.0 == WI_WORLD_GUILDWAR
        || is_gc1to1_world(my_world.0)
        || is_gc1to1_world(their_world.0)
    {
        // silent
    } else if crate::event::rainbow::is_entry_obj(svc, ctx, who.0)
        || svc.event.rainbow.is_entry(CharacterId(partner))
    {
        // ---- rainbow race (doc 22 §14, task rainbow-race): `MoverSkill.cpp:3889-3894`, either side races ----
        fx.text(TID_GAME_RAINBOWRACE_NOTELEPORT);
    } else if my_world == their_world && my_layer == their_layer && my_world.0 != WI_WORLD_QUIZ {
        fx.then(Followup::Teleport {
            mover: who.0,
            world: their_world,
            layer: their_layer,
            pos: their_pos,
            kind: ReplaceKind::Normal,
        });
    } else {
        fx.defined(TID_GAME_COUPLERING_ERR02, vec![partner_name]);
    }
    fx.flush(svc, ctx, who);
}

// ---- the escape command and the return village ---------------------------------------------------------------------

/// `DoUseItemVirtual(prop_id)` for the item prop `prop`: the cast time first (the first call starts the cast and
/// answers `false`), then the effect. Only the escape wing is a virtual item of this build (the return scroll's
/// trip is WP-ITEM-20's). Returns `true` when the effect happened.
pub fn use_virtual(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    now: GameMs,
    facts: &UseFacts,
    fx: &mut Fx,
) -> bool {
    if !matches!(
        cast::ready_check_virtual(m, env, prop, now, fx),
        cast::CastStep::Proceed
    ) {
        return false;
    }
    if prop.id == flyff_common::defs::item::II_SYS_SYS_SCR_RETURN {
        // the trip back of the return scroll (WP-ITEM-20)
        return crate::item::buffitems::return_scroll::trip(m, env, now, fx);
    }
    if prop.id != II_CHR_SYS_SCR_ESCAPEBLINKWING {
        return false;
    }
    if prop.circle_time != NULL_ID {
        if sm_active(m, SM_ESCAPE) {
            return false;
        }
        crate::stats::sm::set(m, SM_ESCAPE, prop.circle_time, env.now.0, &mut fx.out);
    }
    blink_core(m, env, prop, facts, None, fx)
}

/// Runs the virtual use of item prop `prop_id` for `who` and flushes (the escape command, and its cast
/// re-issued by the item tick).
pub fn run_virtual(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, prop_id: u32) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let now = ctx.clock.game_ms;
    let Some(prop) = env.data().item(prop_id) else {
        return;
    };
    let mut fx = Fx::new();
    // doc 21 §8 (task gc11): the escape wing of a participant of the 1:1 guild combat is refused
    let gc1to1 = crate::social::gc1to1::is_participant(svc, ctx, who.0);
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        let mut facts = UseFacts::of(m);
        facts.gc1to1 = gc1to1;
        use_virtual(m, &env, prop, now, &facts, &mut fx);
    }
    fx.flush(svc, ctx, who);
}

/// `DoEscape` (C2S, no payload; `OnDoEscape`): the virtual use of the escape wing while `SM_ESCAPE` is not
/// active, else `TID_GAME_STILLNOTUSE`. Nothing is consumed.
pub fn on_do_escape(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let core = svc.items.handle();
    if core.data.item(II_CHR_SYS_SCR_ESCAPEBLINKWING).is_none() {
        return;
    }
    let active = ctx
        .worlds
        .player(who.0)
        .is_some_and(|m| sm_active(m, SM_ESCAPE));
    if active {
        let mut fx = Fx::new();
        fx.text(TID_GAME_STILLNOTUSE);
        fx.flush(svc, ctx, who);
        return;
    }
    run_virtual(svc, ctx, who, II_CHR_SYS_SCR_ESCAPEBLINKWING);
}

/// `ReturnScrollReq{select}` (VR-004: `-1..=2`): `0..=2` stores the village of the next return scroll and
/// answers `ReturnScrollNtf`; `-1` is the trip back of WP-ITEM-20 and does nothing here.
pub fn on_return_scroll(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ReturnScrollReq,
) {
    if msg.select == -1 {
        // the trip back to the stored position (WP-ITEM-20)
        crate::item::buffitems::return_scroll::on_trip_request(svc, ctx, who);
        return;
    }
    if !(0..=2).contains(&msg.select) {
        return;
    }
    let mut fx = Fx::new();
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.misc.return_village = Some(msg.select as u8);
        fx.out.to_self_subject(pb::ReturnScrollNtf {});
    }
    fx.flush(svc, ctx, who);
}
