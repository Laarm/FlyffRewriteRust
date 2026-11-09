//! The rules around the rooms and the hunting ground (Plan/22 §11.4, corrected to the C++ where the plan text is wrong).
//!
//! * [`revival_override`] (`DeathHooks::secret_room_revival`, `DPSrvr.cpp:1172-1179`): a death in a room (152..=167)
//!   revives in MADRIGAL at the revival point of the side of the player's LINE-UP, else the `NODATA` point (6968, 0,
//!   3328) - not "in the room" (the plan text was wrong),
//! * [`logout_position`] (`LogoutHooks::secret_room_position`, `User.cpp:4172-4190`): a logout inside a room saves at the
//!   revival point of the ROOM's side (deviation g: the legacy used the line-up side and `NODATA` for a non-member, a
//!   game master; identical for every line-up member, and the logout hook has no services),
//! * [`on_teleport_to_npc`] (`SecretRoomTeleportToNpc`, VR-062: only inside 151..=167): back to Madrigal at the
//!   line-up side's revival point, layer 0, no other check (legacy),
//! * [`on_teleport_to_dungeon`] (`SecretRoomTeleportToDungeon`, menu 100): the guild that owns the player's area (the
//!   CURRENT tax owner) enters the hunting ground 151; the Lodelight mark is overwritten with the place he leaves,
//! * [`on_tax_changed`] (`OnTaxInfo` at the weekly change, `DPDatabaseClient.cpp:3634-3675`): the caption
//!   `TID_GAME_TAX_CHANGENEXT` when flag 24 is on, and everybody in 151 (any layer) goes back to his marking place,
//! * [`invite_locked`] (`IsInviteAbleGuild`, `DPSrvr.cpp:9998-10040`): a guild with a tender may not invite while the
//!   manager is not `Close`.

use super::tender::{area_of, npc_ok, say};
use super::{HUNTING_GROUND, SrMngState, enabled, is_room_world, room_side};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use crate::world::ReplaceKind;
use crate::world::continent::{CONT_NODATA, Continents};
use crate::world::revival::MADRIGAL_FLARIS_CENTER;
use flyff_common::defs::neuz::MMI_SECRET_ENTRANCE_1;
use flyff_common::defs::text::{TID_GAME_SECRETROOM_NOENTRANCE_1, TID_GAME_TAX_CHANGENEXT};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::{GuildId, LayerId, ObjId, WorldId};

/// The side whose line-up holds the player, else `CONT_NODATA` (`CSecretRoomMng::GetRevivalPos`).
pub fn lineup_side_of(s: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> u8 {
    crate::social::guild::character_of(ctx, who)
        .and_then(|c| {
            s.social
                .guild
                .guild_of(c)
                .and_then(|g| s.event.secret_room.lineup_side(g, c))
        })
        .unwrap_or(CONT_NODATA)
}

/// `DeathHooks::secret_room_revival` (`RevivalOverrideHook`): `Some((Madrigal, revival point))` for a player in a room.
pub fn revival_override(
    s: &Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
) -> Option<(WorldId, Vec3)> {
    let world = ctx.worlds.core(who.0)?.world;
    if !is_room_world(world) {
        return None;
    }
    Some((
        WorldId(WI_WORLD_MADRIGAL),
        Continents::revival_pos(lineup_side_of(s, ctx, who)),
    ))
}

/// `LogoutHooks::secret_room_position`: the revival point of the side of the room the player stands in.
pub fn logout_position(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Vec3> {
    let world = ctx.worlds.core(who.0)?.world;
    is_room_world(world).then(|| Continents::revival_pos(room_side(world)))
}

/// `SecretRoomTeleportToNpc`: only in 151..=167 (VR-062, silent elsewhere); a normal replace to Madrigal at the
/// revival point of the player's line-up side (`NODATA` without one), layer 0.
pub fn on_teleport_to_npc(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(world) = ctx.worlds.core(who.0).map(|c| c.world) else {
        return;
    };
    if !is_room_world(world) && world != HUNTING_GROUND {
        return;
    }
    let pos = Continents::revival_pos(lineup_side_of(s, ctx, who));
    if let Err(e) = ctx.worlds.replace(
        who.0,
        WorldId(WI_WORLD_MADRIGAL),
        pos,
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::debug!(player = who.0.0, error = %e, "SecretRoomTeleportToNpc: the replace was refused");
    }
}

/// `SecretRoomTeleportToDungeon` (menu 100, `OnTeleportSecretRoomDungeon`): needs the NPC (the one in the shipped data
/// stands in Darkon); the player's guild must be the CURRENT tax owner of his area (`NODATA`: no owner), else
/// `TID_GAME_SECRETROOM_NOENTRANCE_1`. Then the place he leaves becomes his marking position, his angle 180, and a
/// normal replace puts him into the hunting ground at `(295 + j, 102, 530 + j')`, layer 0 (`j = xRandom(4) - 2`).
pub fn on_teleport_to_dungeon(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !npc_ok(ctx, who, MMI_SECRET_ENTRANCE_1) {
        return;
    }
    let area = area_of(ctx, who);
    let guild =
        crate::social::guild::character_of(ctx, who).and_then(|c| s.social.guild.guild_of(c));
    let owner = s.event.tax.owner(area);
    if guild.is_none_or(|g| owner != Some(g.0)) {
        say(ctx, who, TID_GAME_SECRETROOM_NOENTRANCE_1);
        return;
    }
    let Some((world, pos)) = ctx.worlds.core(who.0).map(|c| (c.world, c.pos)) else {
        return;
    };
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.misc.marking_world = world;
        p.misc.marking_pos = pos;
    }
    s.saves.mark_urgent(who.0);
    let (jx, jz) = super::war::jitter(ctx);
    super::war::set_angle_180(ctx, who.0);
    let pos = Vec3::new((295 + jx) as f32, 102.0, (530 + jz) as f32);
    if let Err(e) = ctx
        .worlds
        .replace(who.0, HUNTING_GROUND, pos, LayerId(0), ReplaceKind::Normal)
    {
        tracing::debug!(player = who.0.0, error = %e, "SecretRoomTeleportToDungeon: the replace was refused");
    }
}

/// The players standing in `world`, in any layer (linked or waiting to be added).
fn players_in_world(ctx: &GameCtx<'_>, world: WorldId) -> Vec<ObjId> {
    let Some(w) = ctx.worlds.world(world) else {
        return Vec::new();
    };
    w.layers
        .values()
        .flat_map(|l| l.members.iter().chain(l.queues.add.iter()).copied())
        .filter(|&o| ctx.worlds.store().is_player(o))
        .collect()
}

/// `OnTaxInfo` with `bToAllClient` (the weekly change, also `/tan` and the catch-up): the caption
/// `TID_GAME_TAX_CHANGENEXT` to everybody when flag 24 is on, then every player in the hunting ground (any layer) goes
/// back to his marking place, layer 0 (FIX: without a Madrigal mark he goes to the Flaris centre instead of staying).
pub fn on_tax_changed(s: &mut Services, ctx: &mut GameCtx<'_>) {
    if enabled(ctx) {
        let line = ctx.data.texts.get(TID_GAME_TAX_CHANGENEXT).to_owned();
        notice::caption(s, ctx, Audience::AllOnline, false, WorldId(0), &line);
    }
    for obj in players_in_world(ctx, HUNTING_GROUND) {
        let mark = ctx
            .worlds
            .mover(obj)
            .and_then(|m| m.player())
            .map(|p| (p.misc.marking_world, p.misc.marking_pos));
        let (world, pos) = match mark {
            Some((w, p)) if w.0 != 0 => (w, p),
            _ => (WorldId(WI_WORLD_MADRIGAL), MADRIGAL_FLARIS_CENTER),
        };
        if let Err(e) = ctx
            .worlds
            .replace(obj, world, pos, LayerId(0), ReplaceKind::Normal)
        {
            tracing::debug!(player = obj.0, error = %e, "secret room: the eviction from the hunting ground was refused");
        }
    }
}

/// `GuildSeams::secret_room_invite_locked`: the manager is not `Close` and the guild has a tender on either side.
pub fn invite_locked(s: &Services, guild: GuildId) -> bool {
    s.event.secret_room.state != SrMngState::Close && s.event.secret_room.has_tender(guild)
}
