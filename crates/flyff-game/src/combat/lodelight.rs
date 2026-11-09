//! `SetLodelight` (Plan/17 §9.6, WP-COMBAT-11; ref/07a §10.4; src:WORLDSERVER/DPSrvr.cpp:1226-1236): the
//! player stores the place it stands on as its marking position at a `MMI_MARKING` NPC. The position is the
//! one the marking and return scrolls read; it is saved with the character (`characters.marking_world_id`,
//! `marking_x/y/z`, 13 §5.4).

use crate::chat::ingress::notice::defined_text;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::defs::neuz::MMI_MARKING;
use flyff_common::defs::text::TID_GAME_LODELIGHT;
use flyff_proto::pb;

/// What a `SetLodelight` request did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lodelight {
    /// The request is ignored: no marking NPC within `gameplay.npc_menu_range` (VR-012: same world and layer,
    /// 3-D distance), or the sender is no live player. Nothing is answered.
    Ignored,
    /// The marking position was set.
    Marked,
}

/// `OnSetLodelight`: `NpcChecker::close_npc( MMI_MARKING )`, then `SetMarkingPos()` (`m_vMarkingPos = pos`,
/// `m_idMarkingWorld = world`) and `DefinedText{TID_GAME_LODELIGHT}` to the player.
pub fn on_set_lodelight(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::SetLodelight,
) -> Lodelight {
    let Some((world, layer, pos)) = ctx.worlds.core(who.0).map(|c| (c.world, c.layer, c.pos))
    else {
        return Lodelight::Ignored;
    };
    if ctx
        .worlds
        .close_npc(MMI_MARKING, world, layer, pos)
        .is_none()
    {
        return Lodelight::Ignored;
    }
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
        return Lodelight::Ignored;
    };
    p.misc.marking_world = world;
    p.misc.marking_pos = pos;
    if let Some(sid) = ctx.session_of(who) {
        ctx.outbox
            .to_session(sid, 0, defined_text(TID_GAME_LODELIGHT, Vec::new()));
    }
    // the mark is part of the next snapshot; the player does not wait for the periodic one
    svc.saves.mark_urgent(who.0);
    Lodelight::Marked
}
