//! The guild-war senders (Plan/21 §6, 11 §4.2): every message goes out in `CharacterId` order, the declaring guild
//! before the accepting one, and is encoded once per audience.

use super::model::GuildWar;
use crate::ctx::{GameCtx, Services};
use crate::outbox::Audience;
use crate::social::guild::wire::{to_all, to_members, to_one};
use flyff_common::{GuildId, GuildWarId, ObjId};
use flyff_proto::pb;

/// `DeclGuildWarNtf` to ONE avatar: the master of the challenged guild.
pub fn declaration(ctx: &mut GameCtx<'_>, to: ObjId, decl_guild: GuildId, master_name: String) {
    to_one(
        ctx,
        to,
        pb::DeclGuildWarNtf {
            id_decl_guild: decl_guild.0,
            master_name,
        },
    );
}

/// `QueryTruceNtf` to ONE avatar: the master of the enemy guild.
pub fn truce_request(ctx: &mut GameCtx<'_>, to: ObjId) {
    to_one(ctx, to, pb::QueryTruceNtf {});
}

/// `AcptGuildWarNtf{id_war, id_decl, id_acpt}` to every player in a world.
pub fn started(svc: &Services, ctx: &mut GameCtx<'_>, war: &GuildWar) {
    to_all(
        svc,
        ctx,
        pb::AcptGuildWarNtf {
            id_war: war.id.0,
            id_decl: war.decl.guild.0,
            id_acpt: war.acpt.guild.0,
        },
    );
}

/// `WarEnd{id_war, points_decl, points_acpt, result}` to every player in a world.
pub fn ended(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    war: GuildWarId,
    points_decl: i32,
    points_acpt: i32,
    result: u8,
) {
    to_all(
        svc,
        ctx,
        pb::WarEnd {
            id_war: war.0,
            points_decl,
            points_acpt,
            result: i32::from(result),
        },
    );
}

/// `SetWar{id_war}` for the avatar `obj` to its viewers and itself (`obj_id` = the member; 0 = no war).
pub fn set_war_near(svc: &Services, ctx: &mut GameCtx<'_>, obj: ObjId, war: u32) {
    ctx.broadcast(
        Some(svc),
        obj,
        Audience::NearInclSelf,
        pb::SetWar { id_war: war },
    );
}

/// Sends `msg` to the online members of the declaring guild, then to those of the accepting guild (the original
/// walked both rosters in this order); the same message twice means two `Audience::Guild` fan-outs.
pub fn to_both_guilds<M>(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    decl: GuildId,
    acpt: GuildId,
    msg: M,
) where
    M: Into<pb::server_msg::Msg> + Clone,
{
    to_members(svc, ctx, decl, msg.clone());
    to_members(svc, ctx, acpt, msg);
}
