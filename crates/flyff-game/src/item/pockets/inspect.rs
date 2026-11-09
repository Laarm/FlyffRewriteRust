//! `QueryEquipReq` (Plan/19 §5.13.5; ref/07b §9.2; `DPSrvr.cpp:7121-7141`, `User.cpp:2697-2730`): the equipment
//! of another player.
//!
//! **VR-051 (FIX):** the original took any object id of a player anywhere in the world; the target must be a
//! player in the actor's view set (the actor itself counts). A target with `EQUIP_DENIAL_MODE` refuses a
//! requester below `AUTH_GAMEMASTER` with `TID_DIAG_0088` (`IsAuthHigher` is `>=`). The answer
//! `QueryEquipNtf` goes to the requester only, with the target as the header object: for every worn part the
//! packed random-option word, the sockets, the element and its level.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::mover::Mover;
use flyff_common::ObjId;
use flyff_common::defs::text::TID_DIAG_0088;
use flyff_common::gm::Authority;
use flyff_proto::pb;
use flyff_rules::consts::{EQUIP_DENIAL_MODE, MAX_HUMAN_PARTS};

/// The inspectable data of every worn part of `target`, in part order (`CUser::AddQueryEquip`).
pub fn equip_entries(target: &Mover) -> Vec<pb::QueryEquipEntry> {
    let Some(p) = target.player() else {
        return Vec::new();
    };
    let inv = &p.items.inventory;
    (0..MAX_HUMAN_PARTS as u32)
        .filter_map(|part| {
            let it = inv.get_equip(part)?;
            let piercing = if it.piercing.is_empty() {
                None
            } else {
                Some(pb::Piercing {
                    card_item_ids: it.piercing.cards.clone(),
                    ultimate_gem_ids: it.piercing.ultimate_gems.clone(),
                    vis_expire_at: it.piercing.vis_keep_until.clone(),
                })
            };
            Some(pb::QueryEquipEntry {
                part: part as i32,
                random_opt_item_id: it.wire_random_option(),
                piercing,
                item_resist: u32::from(it.wire_element()),
                resist_ability_option: it.element_level,
            })
        })
        .collect()
}

/// Why a request was not answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectRefusal {
    /// Not a player, or not in the actor's view set (VR-051): silent.
    NotVisible,
    /// The target refuses and the actor is below GAMEMASTER: `TID_DIAG_0088`.
    Denied,
}

/// The decision of `OnQueryEquip` over the two movers and the actor's view set.
pub fn check(actor: &Mover, target: &Mover, actor_sees_target: bool) -> Result<(), InspectRefusal> {
    if target.player().is_none() || !(actor.id() == target.id() || actor_sees_target) {
        return Err(InspectRefusal::NotVisible);
    }
    let gm = actor
        .player()
        .is_some_and(|p| p.ident.authority.at_least(Authority::GAMEMASTER));
    if target.mv.mode & EQUIP_DENIAL_MODE != 0 && !gm {
        return Err(InspectRefusal::Denied);
    }
    Ok(())
}

/// `QueryEquipReq` (C2S).
pub fn on_query_equip(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::QueryEquipReq,
) {
    let target = ObjId(msg.target);
    let seen = ctx
        .worlds
        .core(who.0)
        .is_some_and(|c| c.viewers.contains(&target));
    let verdict = match (ctx.worlds.player(who.0), ctx.worlds.player(target)) {
        (Some(a), Some(t)) => check(a, t, seen).map(|()| equip_entries(t)),
        _ => return,
    };
    match verdict {
        Ok(entries) => {
            if let Some(sid) = ctx.session_of(who) {
                ctx.outbox
                    .to_session(sid, target.0, pb::QueryEquipNtf { entries });
            }
        }
        Err(InspectRefusal::NotVisible) => {}
        Err(InspectRefusal::Denied) => {
            let mut fx = Fx::new();
            fx.text(TID_DIAG_0088);
            fx.flush(svc, ctx, who);
        }
    }
}
