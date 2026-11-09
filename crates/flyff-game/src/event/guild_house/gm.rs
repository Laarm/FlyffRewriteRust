//! The GM and test calls of the guild house (Plan/22 §12, README ruling 42): the names `gm-content-control`'s
//! `/content gh give|expire|furniture <guild>`, `/content gh <guild>` and the `gh` state line call, and that the tests
//! and the browser flows use. They are pure calls: nothing here prints and nothing checks authority; the rows 215 and 216
//! (`/bgh`, `/ghu`) stay DROP.
//!
//! * [`grant_house`]: the `GuildHouseBuy` commit without the NPC, master, level, member and penya checks and without
//!   payment (ONE `GuildHouseGrant` batch);
//! * [`expire_upkeep`]: the upkeep deadline moves to now; NO immediate pass: the next P11 run (at most 5 s) expires the
//!   house ("advance = the deadline to now", the uniform seam of the content-control verbs);
//! * [`expire_furniture`]: every started keep countdown moves to now; P11 drops one entry per house per run as ever;
//! * [`set_upkeep`]: an exact deadline (tests);
//! * [`house_line`], [`state_line`]: the status lines.

use super::requests::{announce_purchase, new_house, upkeep_info};
use super::{GhOp, GuildHouse, send_batch};
use crate::ctx::{GameCtx, Services};
use crate::social::party::now_secs;
use flyff_common::{GuildId, UnixTime};
use flyff_db::audit::content as audit;
use flyff_db::repo::meta::counter;
use flyff_db::{AtomicBatch, AtomicKind, DbOp};
use serde_json::json;

/// Why a GM call was refused (`gm-content-control` prints `"gh: <reason>"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GhGmError {
    /// No guild has this id.
    #[error("no such guild")]
    NoGuild,
    /// The guild owns a house already.
    #[error("already has a house")]
    HasHouse,
    /// The guild owns no house.
    #[error("has no house")]
    NoHouse,
}

/// `/content gh give <guild>`: the guild gets a house as if its master had bought it (upkeep 7 days, the installed
/// teleporter), at no cost and without the requirements; every online member gets `GuildHouseAllInfo`, `PURCHASE` and
/// the DSTs; audit `guild_house.buy` with `source: "gm"`.
pub fn grant_house(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
) -> Result<(), GhGmError> {
    if svc.social.guild.get(guild).is_none() {
        return Err(GhGmError::NoGuild);
    }
    if svc.event.guild_houses.has_house(guild) {
        return Err(GhGmError::HasHouse);
    }
    // ---- the commit block ----
    let now = now_secs(ctx);
    let (house, teleporter) = new_house(svc, guild, now);
    let batch = AtomicBatch::of_ops(
        AtomicKind::Other("GuildHouseGrant"),
        vec![
            DbOp::UpsertGuildHouse(house.record()),
            DbOp::UpsertGuildHouseFurniture(teleporter.record(guild)),
            DbOp::RaiseCounter {
                key: counter::NEXT_GUILD_FURNITURE_SEQ,
                at_least: i64::from(teleporter.seq) + 1,
            },
        ],
    );
    let upkeep = house.upkeep_until.0;
    svc.event.guild_houses.insert(house);
    send_batch(ctx, "GuildHouseGrant", batch);
    announce_purchase(svc, ctx, guild);
    let mut ev = audit::guild_house_buy(guild, flyff_common::CharacterId(0), 0, upkeep);
    if let Some(o) = ev.data.as_object_mut() {
        o.insert("source".to_owned(), json!("gm"));
    }
    ctx.audit.emit(ev);
    tracing::info!(guild = guild.0, "guild house: granted by a GM call");
    Ok(())
}

/// `/content gh expire <guild>`: `upkeep_until = now`, ONE `UpsertGuildHouse`, no immediate pass; the next P11 run
/// expires the house. A house that is expired already stays as it is.
pub fn expire_upkeep(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
) -> Result<(), GhGmError> {
    let now = now_secs(ctx);
    let house = svc
        .event
        .guild_houses
        .house_mut(guild)
        .ok_or(GhGmError::NoHouse)?;
    if house.is_expired() {
        return Ok(());
    }
    house.upkeep_until = now;
    ctx.persist.op(DbOp::UpsertGuildHouse(house.record()));
    Ok(())
}

/// `/content gh furniture <guild>`: the keep time of every entry whose countdown started (`keep_until > 0`) becomes
/// now, in ONE batch; returns how many entries it touched. P11 then drops one per house per run.
pub fn expire_furniture(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId) -> usize {
    let now = now_secs(ctx);
    let Some(house) = svc.event.guild_houses.house_mut(guild) else {
        return 0;
    };
    let mut ops = Vec::new();
    for f in house.furniture.iter_mut().filter(|f| f.keep_until.0 > 0) {
        f.keep_until = now;
        ops.push(DbOp::UpsertGuildHouseFurniture(f.record(guild)));
    }
    let n = ops.len();
    if n > 0 {
        send_batch(
            ctx,
            "GuildHouseFurnitureExpire",
            AtomicBatch::of_ops(AtomicKind::Other("GuildHouseFurnitureExpire"), ops),
        );
    }
    n
}

/// An exact upkeep deadline (`UnixTime(0)` = expired): ONE `UpsertGuildHouse` and `GuildHousePacketNtf{UPKEEP, -1}`
/// to the members.
pub fn set_upkeep(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    until: UnixTime,
) -> Result<(), GhGmError> {
    let house = svc
        .event
        .guild_houses
        .house_mut(guild)
        .ok_or(GhGmError::NoHouse)?;
    house.upkeep_until = until;
    ctx.persist.op(DbOp::UpsertGuildHouse(house.record()));
    super::notify(svc, ctx, guild, GhOp::Upkeep, -1, upkeep_info(0, until.0));
    Ok(())
}

fn room_open(ctx: &GameCtx<'_>, h: &GuildHouse) -> bool {
    super::room_exists(ctx.worlds, h.guild)
}

/// `gh <guild name>: upkeep <n> s left|expired, <listed> listed, <installed> installed, room open|closed`
/// (`gh <name>: no house` without one).
pub fn house_line(svc: &Services, ctx: &GameCtx<'_>, guild: GuildId) -> String {
    let name = svc
        .social
        .guild
        .get(guild)
        .map_or_else(|| format!("#{}", guild.0), |g| g.name.clone());
    let Some(h) = svc.event.guild_houses.house(guild) else {
        return format!("gh {name}: no house");
    };
    let now = now_secs(ctx).0;
    let upkeep = if h.upkeep_until.0 > 0 {
        format!("upkeep {} s left", (h.upkeep_until.0 - now).max(0))
    } else {
        "upkeep expired".to_owned()
    };
    format!(
        "gh {name}: {upkeep}, {} listed, {} installed, room {}",
        h.furniture.len(),
        h.installed().count(),
        if room_open(ctx, h) { "open" } else { "closed" }
    )
}

/// `gh: Active, <houses> houses, <expired> expired, <rooms> rooms open` (the guild house has no state machine; P11
/// always runs).
pub fn state_line(svc: &Services, ctx: &GameCtx<'_>) -> String {
    let houses = svc.event.guild_houses.len();
    let expired = svc
        .event
        .guild_houses
        .houses()
        .filter(|h| h.is_expired())
        .count();
    let rooms = svc
        .event
        .guild_houses
        .houses()
        .filter(|h| room_open(ctx, h))
        .count();
    format!("gh: Active, {houses} houses, {expired} expired, {rooms} rooms open")
}
