//! Shout (Plan/21 §10.3; F-265, KQ-12; ref/24 §7.2).

use super::broadcast_filtered;
use super::filter::{ChatChannel, delivers};
use crate::admin::chat_tap::{self, ChatChannel as TapChannel};
use crate::chat::ingress::{has_item_buff, is_shout_full};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use flyff_common::defs::item::II_SYS_SYS_LS_SHOUT;
use flyff_proto::pb;

/// `shout_color`: `0xFF00FF00` with the lord-skill shout buff (`II_SYS_SYS_LS_SHOUT`, equals 22's
/// `lord.shout_color`), else `0xFFFF99CC`.
pub const fn shout_color(has_lord_shout_buff: bool) -> u32 {
    if has_lord_shout_buff {
        0xFF00_FF00
    } else {
        0xFFFF_99CC
    }
}

/// `route::shout` (21 §10.3): `Shout{obj_id, name, text, color}` (header obj id 0) to the audience of
/// `CUserMng::AddShout`: the same world and layer within `gameplay.shout_range` (3-D, inclusive: 255.0 is heard,
/// 255.1 is not) or, with an active `SM_SHOUT15|30|001` timer, every player in a world; the speaker is included.
/// `text` is already cleaned by `shout_pipeline`.
pub fn shout(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, text: &str) {
    let Some(m) = ctx.player(who) else {
        return;
    };
    let Some(p) = m.player() else {
        return;
    };
    let color = shout_color(has_item_buff(m, II_SYS_SYS_LS_SHOUT));
    let range = if is_shout_full(m) {
        0.0
    } else {
        ctx.cfg.rules.gameplay.shout_range
    };
    let msg = pb::Shout {
        obj_id: who.0.0,
        name: p.ident.name.clone(),
        text: text.to_owned(),
        color,
    };
    let aud = Audience::Shout {
        world: m.core.world,
        layer: m.core.layer,
        center: m.core.pos,
        range,
    };
    let speaker = who.0;
    broadcast_filtered(
        svc,
        ctx,
        flyff_common::ObjId::NULL,
        aud,
        msg,
        |_, listener, worlds| delivers(svc, worlds, listener, speaker, ChatChannel::Shout),
    );
    // the admin console's chat tap (25 §4.9)
    chat_tap::record(svc, ctx, who, TapChannel::Shout, None, text);
}
