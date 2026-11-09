//! The quest helper's NPC locator (Plan/20 §9.9, Plan/15 §13.3, WP-NPC-15; `DPSrvr.cpp:12397-12416`):
//! `QuestHelperReqNpcPos{character_key}` answers the place of a character in the player's world.

use crate::chat::ingress::notice;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::defs::text::TID_GAME_QUESTINFO_FAIL;
use flyff_proto::pb;

/// The longest key the request reads (`ReadString(szCharKey, 64)`: 63 characters).
pub const MAX_KEY_LEN: usize = 63;

/// `QuestHelperReqNpcPos` (the click on the end-character line of the quest window):
///
/// * a key longer than 63 bytes is dropped;
/// * a key that is no character at all is ignored (nothing is sent);
/// * a placed character in the player's world answers `QuestHelperNpcPos{pos}`; a character in another world, or
///   one that was never placed, answers `DefinedText{TID_GAME_QUESTINFO_FAIL}`.
///
/// There is no layer check (original).
pub fn on_quest_helper_req_npc_pos(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::QuestHelperReqNpcPos,
) {
    if m.character_key.len() > MAX_KEY_LEN || ctx.data.npc(&m.character_key).is_none() {
        return;
    }
    let Some(world) = ctx.worlds.core(who.0).map(|c| c.world) else {
        return;
    };
    match ctx.worlds.npc_locator.get(&m.character_key) {
        Some((w, pos)) if w == world => send_to(
            ctx,
            who,
            pb::QuestHelperNpcPos {
                pos: Some(pb::Vec3 {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                }),
            },
        ),
        _ => notice::defined(ctx, who, TID_GAME_QUESTINFO_FAIL, Vec::new()),
    }
}
