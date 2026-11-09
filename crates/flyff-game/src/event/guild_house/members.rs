//! The members of a guild house: the furniture DSTs, the guild seams, the join, the expiry bodies and the
//! `GuildHouseAllInfo` join step (Plan/22 §12.5, §12.6; `_Common/GuildHouse.cpp:584-678`, `DPCoreClient.cpp:1790-1810,
//! 1925-1961`).
//!
//! **DSTs.** Each installed entry is the ledger source `DstSource::GuildFurniture{seq}` with the three
//! `(dest_param, adj_param_val, chg_param_val)` slots of its prop (`NULL_ID` skipped, as `stats/equip.rs`); a removal is
//! exact (PL-080) and idempotent, an application is idempotent per source. The original's reset and apply around a
//! world change (`World.cpp:1638-1653`) is satisfied by construction: the ledger keeps the source across the change and
//! the target's `SpawnSelf` carries the aggregate.

use super::room::{check_destroy_room, go_out, is_inside, players_inside};
use super::{
    GhFurniture, GhOp, GuildHouse, WORLD, all_info, fntr_info_at, layer_of, notify, room_live,
    send_batch,
};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::outbox::Outbox;
use crate::stats::StatsCore;
use crate::stats::dst::DstSource;
use crate::stats::out::Out;
use crate::stats::source::{apply_source, remove_source};
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::{GuildId, ObjId, UnixTime};
use flyff_data::props::item::ItemProp;
use flyff_db::{AtomicBatch, DbOp};
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;

/// A ledger source with the DST slots it applies.
type DstJob = (DstSource, Vec<(u32, i32, i32)>);

/// The three DST slots of a furniture prop (`SetDestParam(0..2, prop)`): `NULL_ID` slots are skipped.
pub(crate) fn furniture_slots(prop: &ItemProp) -> Vec<(u32, i32, i32)> {
    (0..3)
        .filter_map(|i| {
            let dst = prop.dest_param[i];
            (dst != NULL_ID).then(|| {
                (
                    dst,
                    prop.adj_param_val[i],
                    i32::try_from(prop.chg_param_val[i]).unwrap_or(i32::MAX),
                )
            })
        })
        .collect()
}

/// The ledger source of an entry.
pub(crate) fn source_of(f: &GhFurniture) -> DstSource {
    DstSource::GuildFurniture { seq: f.seq }
}

/// Runs `f` on the avatar `obj` with the stats environment and sends what it queued.
fn with_avatar(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    obj: ObjId,
    f: impl FnOnce(&mut Mover, &StatsCore, &mut Out),
) {
    let core = svc.stats.handle();
    let mut out = Out::new();
    if let Some(m) = ctx.worlds.player_mut(obj) {
        f(m, &core, &mut out);
    }
    out.flush(ctx, obj);
}

/// The sources and slots of the installed entries of `house`.
fn jobs_of(ctx: &GameCtx<'_>, house: &GuildHouse) -> Vec<DstJob> {
    house
        .installed()
        .filter_map(|(_, f)| {
            ctx.data
                .item(f.item)
                .map(|p| (source_of(f), furniture_slots(p)))
        })
        .collect()
}

/// `SetApplyDST`: every installed entry's DSTs onto the avatar `obj` (with `send` the `SetDestParam` messages go out;
/// the join passes `false`, `SpawnSelf` carries the result).
pub(crate) fn apply_house(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    obj: ObjId,
    send: bool,
) {
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let jobs = jobs_of(ctx, house);
    with_avatar(svc, ctx, obj, |m, core, out| {
        for (src, slots) in &jobs {
            if !m.mv.dst.has(*src) {
                apply_source(m, core, *src, slots, send, out);
            }
        }
    });
}

/// `ResetApplyDST`: everything the house applied to the avatar `obj` goes again.
pub(crate) fn reset_house(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    obj: ObjId,
    send: bool,
) {
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let sources: Vec<DstSource> = house.installed().map(|(_, f)| source_of(f)).collect();
    with_avatar(svc, ctx, obj, |m, core, out| {
        for src in &sources {
            remove_source(m, core, *src, send, out);
        }
    });
}

/// `SetDSTFunriture`: one entry's DSTs onto every online member of `guild`.
pub(crate) fn apply_entry_to_members(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    f: &GhFurniture,
) {
    let Some(prop) = ctx.data.item(f.item) else {
        return;
    };
    let (src, slots) = (source_of(f), furniture_slots(prop));
    for obj in svc.social.guild.online_objs(&svc.presence, guild) {
        with_avatar(svc, ctx, obj, |m, core, out| {
            if !m.mv.dst.has(src) {
                apply_source(m, core, src, &slots, true, out);
            }
        });
    }
}

/// `ResetDSTFunriture`: one entry's DSTs off every online member of `guild`.
pub(crate) fn reset_entry_for_members(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    src: DstSource,
) {
    for obj in svc.social.guild.online_objs(&svc.presence, guild) {
        with_avatar(svc, ctx, obj, |m, core, out| {
            remove_source(m, core, src, true, out);
        });
    }
}

// ---- the guild seams ----------------------------------------------------------------------------------------------------

/// `GuildSeams::on_member_joined` (`DPCoreClient.cpp:1925-1928`): the new member gets `GuildHouseAllInfo` (always,
/// `house` absent without one) and the DSTs of the installed furniture.
pub fn on_member_joined(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId, obj: ObjId) {
    let msg = all_info(svc, ctx, Some(guild));
    crate::social::guild::wire::to_one(ctx, obj, msg);
    apply_house(svc, ctx, guild, obj, true);
}

/// `GuildSeams::on_member_left` (`DPCoreClient.cpp:1790-1810, 1955-1961`), right BEFORE the member's mirror is
/// cleared: the rest points are 0, the furniture DSTs go, a member inside his house is sent out, and with `disband` a
/// member of a guild with a house gets `GuildHouseRemove` (`OnRemoveGuildHouse`).
pub fn on_member_left(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    obj: ObjId,
    disband: bool,
) {
    if let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) {
        p.misc.rest_points = 0;
    }
    if !svc.event.guild_houses.has_house(guild) {
        return;
    }
    reset_house(svc, ctx, guild, obj, true);
    if is_inside(ctx, obj, guild) {
        go_out(ctx, guild, obj);
    }
    if disband {
        crate::social::guild::wire::to_one(ctx, obj, pb::GuildHouseRemove {});
    }
}

/// `GuildSeams::on_disband`, after the loop over the members: the house leaves the service and its room is
/// invalidated (the players were sent out by [`on_member_left`]; FIX: the original left them in a layer whose house no
/// longer existed). The rows go with the guild's cascade (`DeleteGuild`): no op here.
pub fn on_guild_disbanded(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId) {
    let Some(house) = svc.event.guild_houses.remove(guild) else {
        return;
    };
    if let Some(layer) = layer_of(guild) {
        ctx.worlds.invalidate(WORLD, layer);
    }
    tracing::info!(
        guild = guild.0,
        furniture = house.furniture.len(),
        "guild house: the guild was dissolved, the house is gone"
    );
}

// ---- the join -----------------------------------------------------------------------------------------------------------

/// Before the join batch, after the guild mirrors (22 §12.2, `DPDatabaseClient.cpp:846-851, 1294-1299`): a character
/// without a guild has no rest points; the DSTs of the installed furniture are applied silently (`SpawnSelf` carries
/// them); a player placed inside his own house gets the offline rest points; a member placed elsewhere while the
/// admission created a room lets it go again.
pub fn on_join(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let Some(pl) = ctx.worlds.player(p.0).and_then(|m| m.player()) else {
        return;
    };
    let guild = pl.social.guild;
    let Some(guild) = guild else {
        if let Some(pl) = ctx.worlds.player_mut(p.0).and_then(|m| m.player_mut()) {
            pl.misc.rest_points = 0;
        }
        return;
    };
    if !svc.event.guild_houses.has_house(guild) {
        return;
    }
    apply_house(svc, ctx, guild, p.0, false);
    if is_inside(ctx, p.0, guild) {
        super::rest::login_bonus(ctx, p);
    } else {
        check_destroy_room(svc, ctx, guild, None);
    }
}

/// `JoinStep::GuildHouseAllInfo`: `GuildHouseAllInfo{bank_enabled, house}` only for a member of a guild that owns a
/// house (the idle join batch is unchanged; the client resets its guild-house state at every join).
#[derive(Clone, Copy, Debug, Default)]
pub struct GuildHouseAllInfoStep;

impl JoinContributor for GuildHouseAllInfoStep {
    fn step(&self) -> JoinStep {
        JoinStep::GuildHouseAllInfo
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(guild) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
            .and_then(|c| cx.svc.social.guild.guild_of(c))
        else {
            return;
        };
        let Some(house) = cx.svc.event.guild_houses.house(guild) else {
            return;
        };
        out.to_session(
            cx.session,
            0,
            pb::GuildHouseAllInfo {
                bank_enabled: cx
                    .live
                    .event_state(flyff_common::config::EventId::GuildInventory)
                    != 0,
                house: Some(super::house_info(cx.worlds, house)),
            },
        );
    }
}

// ---- the expiry bodies of P11 ---------------------------------------------------------------------------------------------

/// Deletes the object of an entry (the layer may be gone already).
fn delete_object(ctx: &mut GameCtx<'_>, f: &GhFurniture) {
    if let Some(o) = f.obj {
        ctx.worlds.delete(o);
    }
}

/// LISTDROP of the FIRST entry whose keep time ran out (`GetFirstExpiredFurnitureIndex`, then `ListDropFurniture`): an
/// installed entry is taken down first (object, DSTs), the entry leaves the list, `DeleteGuildHouseFurniture{seq}` and
/// `GuildHousePacketNtf{LISTDROP, index, info}` to the members. One entry per house per run.
pub(crate) fn drop_first_expired(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    now: UnixTime,
) {
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let Some(index) = house
        .furniture
        .iter()
        .position(|f| f.keep_until.0 > 0 && f.keep_until.0 <= now.0)
    else {
        return;
    };
    let live = room_live(ctx.worlds, guild);
    let Some(entry) = house.furniture.get(index).cloned() else {
        return;
    };
    let info = fntr_info_at(&entry, live);
    // ---- the commit block ----
    if entry.installed {
        delete_object(ctx, &entry);
        reset_entry_for_members(svc, ctx, guild, source_of(&entry));
    }
    if let Some(h) = svc.event.guild_houses.house_mut(guild) {
        h.furniture.remove(index);
    }
    ctx.persist
        .op(DbOp::DeleteGuildHouseFurniture { seq: entry.seq });
    notify(
        svc,
        ctx,
        guild,
        GhOp::ListDrop,
        i32::try_from(index).unwrap_or(i32::MAX),
        info,
    );
    tracing::info!(
        guild = guild.0,
        item = entry.item,
        seq = entry.seq,
        "guild house: a furniture entry ran out"
    );
}

/// EXPIRED (`ExpiredGuildHouse`): when the upkeep ran out, the players inside are sent out, every installed entry is
/// taken down (objects, DSTs; the keep times keep running), `upkeep_until = 0`, ONE `GuildHouseExpire` batch, the room
/// is invalidated and the members get `GuildHousePacketNtf{EXPIRED, -1, ..}`. The house stays; UPKEEP reactivates it.
pub(crate) fn expire_house(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    now: UnixTime,
) {
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    if !(house.upkeep_until.0 > 0 && house.upkeep_until.0 <= now.0) {
        return;
    }
    let installed: Vec<GhFurniture> = house.installed().map(|(_, f)| f.clone()).collect();
    // ---- the commit block ----
    if let Some(layer) = layer_of(guild) {
        for p in players_inside(ctx.worlds, layer) {
            go_out(ctx, guild, p);
        }
    }
    for f in &installed {
        delete_object(ctx, f);
        reset_entry_for_members(svc, ctx, guild, source_of(f));
    }
    let Some(house) = svc.event.guild_houses.house_mut(guild) else {
        return;
    };
    house.upkeep_until = UnixTime(0);
    for f in house.furniture.iter_mut().filter(|f| f.installed) {
        f.installed = false;
        f.obj = None;
    }
    let record = house.record();
    let uninstalled: Vec<_> = installed
        .iter()
        .map(|f| {
            let mut r = f.record(guild);
            r.installed = false;
            r
        })
        .collect();
    send_batch(
        ctx,
        "GuildHouseExpire",
        AtomicBatch::guild_house_expire(record, uninstalled),
    );
    if let Some(layer) = layer_of(guild) {
        ctx.worlds.invalidate(WORLD, layer);
    }
    notify(
        svc,
        ctx,
        guild,
        GhOp::Expired,
        -1,
        pb::GhFntrInfo {
            item_id: super::NO_OBJ,
            ctrl_obj_id: super::NO_OBJ,
            pos: Some(crate::movement::intents::pb_vec(
                flyff_common::math::Vec3::ZERO,
            )),
            ..pb::GhFntrInfo::default()
        },
    );
    tracing::info!(
        guild = guild.0,
        uninstalled = installed.len(),
        "guild house: the upkeep ran out, the house expired"
    );
}
