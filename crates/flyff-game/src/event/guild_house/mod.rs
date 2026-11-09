//! `event::guild_house`: the small guild house and the rest points (Plan/22 §12, F-291, F-135; WP-EVENT-14,
//! WP-EVENT-15).
//!
//! | file | what |
//! |---|---|
//! | `mod.rs` | [`GuildHouseService`], [`GuildHouse`], [`GhFurniture`], [`GhOp`], the constants, the read API, the wire builders and the P11 [`expiry_step`] |
//! | `requests.rs` | the four client messages, buy, enter, go out, `GuildHousePacketReq` (SETUP, REMOVE, RESET, UPKEEP), the item rows and their follow-up |
//! | `room.rs` | the layer `(212, guild id)`: create, REFRESH, `check_destroy_room`, login admission, logout, layer change |
//! | `members.rs` | the furniture DSTs of the members, the guild seams, [`on_join`], [`GuildHouseAllInfoStep`] |
//! | `rest.rs` | the rest points (WP-EVENT-15) |
//! | `gm.rs` | the calls of `/content gh` (pure: nothing prints, nothing checks authority) |
//!
//! **State.** Everything is memory owned by the game thread; the database holds the same facts in `guild_houses` and
//! `guild_house_furniture` (`flyff-db` "Content command API"). The service is built from `StartupState.guild_houses`
//! by [`GuildHouseService::load`] in `Game::new` (no batch at boot). A change validates first, then ONE block changes
//! the memory, builds the op or the `AtomicBatch` (with a fresh snapshot where penya or an item moves, VR-018), sends
//! the messages and writes the audit line. A batch that answers `Failed` is a bug: the memory is kept and a CRITICAL
//! line names the batch (21 §2.4, README ruling 18; Plan/22 E5 removed the refund paths and `GUILDHOUSE_LATER`).
//!
//! **The room.** The layer of the house is `(212, LayerId(guild id))`; the world's layer is the truth (the Plan's
//! `layer_open` is not stored). Its objects are the installed entries (`GhFurniture::obj`, runtime only): a furniture
//! entry is a ctrl, a teleporter an NPC mover, papering has no object. A wire `ctrl_obj_id` is the entry's object only
//! while the layer exists and is live; a stale id of a released or invalidated room never reaches a client.
//!
//! **Time.** Every instant is `ctx.clock` (seconds via [`crate::social::party::now_secs`], the rest tick in game ms);
//! tests move the wall clock with a `WallClockCell`.

pub mod gm;
mod members;
mod requests;
pub mod rest;
mod room;

pub use members::{
    GuildHouseAllInfoStep, on_guild_disbanded, on_join, on_member_joined, on_member_left,
};
pub use requests::{on_buy, on_enter, on_go_out, on_item_followup, on_packet_req, use_row};
pub use room::{login_admission, on_layer_changed, on_logout};

use crate::ctx::{GameCtx, Services};
use crate::outbox::Audience;
use crate::social::party::now_secs;
use crate::tickets::TicketResult;
use crate::world::{LayerState, Worlds};
use flyff_common::defs::world::WI_GUILDHOUSE_SMALL;
use flyff_common::math::Vec3;
use flyff_common::{GuildId, LayerId, ObjId, UnixTime, WorldId};
use flyff_db::{GuildHouseFurnitureRecord, GuildHouseRecord, StartupState};
use flyff_proto::pb;
use std::collections::BTreeMap;

// ---- constants (`_Common/GuildHouse.h:25-38`, the small house only) -------------------------------------------------

/// `WI_GUILDHOUSE_SMALL`: the world of every small guild house.
pub const WORLD: WorldId = WorldId(WI_GUILDHOUSE_SMALL);
/// `BUY_MIN_MEMBERSIZE`: the guild needs this many members (offline ones count).
pub const BUY_MIN_MEMBERSIZE: usize = 12;
/// `BUY_MIN_LEVEL`: the guild level needed.
pub const BUY_MIN_LEVEL: i32 = 3;
/// `BUY_PENYA`: the price.
pub const BUY_PENYA: i64 = 100_000_000;
/// `MAX_LISTUP_NUM`: the longest furniture list.
pub const MAX_LISTUP_NUM: usize = 200;
/// `UPKEEP_DAY_TIME`: seconds per upkeep day.
pub const UPKEEP_DAY_TIME: i64 = 86_400;
/// `MAX_UPKEEP_DAY`: the upkeep may never reach this many days.
pub const MAX_UPKEEP_DAY: i64 = 35;
/// `UPKEEP_DAY_PENYA_SMALL`: penya per upkeep day.
pub const UPKEEP_DAY_PENYA_SMALL: i64 = 2_400_000;
/// The upkeep a fresh house starts with (7 days) and the keep time of its teleporter.
pub const BUY_UPKEEP_DAYS: i64 = 7;
/// `MAX_REST_POINT`: the rest points are clamped to `[0, MAX_REST_POINT]`.
pub const MAX_REST_POINT: i32 = 1_000_000;
/// `REST_POINT_INC`: the points of one tick inside the house (before the `DST_RESTPOINT_RATE` scaling).
pub const REST_POINT_INC: i32 = 10_000;
/// `REST_POINT_LOGOUT_INC`: the points per tick of absence at a login inside the house.
pub const REST_POINT_LOGOUT_INC: i32 = 2_500;
/// `REST_POINT_TICK`: the seconds of one tick.
pub const REST_POINT_TICK_S: i64 = 240;
/// The wire `NULL_ID`: no object, no item.
pub const NO_OBJ: u32 = u32::MAX;

/// Where the teleporter NPC stands (`GetTeleporterPos`).
pub const TELEPORTER_POS: Vec3 = Vec3::new(80.0, 100.0, 75.0);
/// The enter area: `x` is drawn from `[78, 83)` first, then `z` from `[61, 66)` (`GetEnterPos`).
pub const ENTER_X: (i32, i32) = (78, 83);
/// See [`ENTER_X`].
pub const ENTER_Z: (i32, i32) = (61, 66);
/// The height of the enter point.
pub const ENTER_Y: f32 = 100.0;
/// Where the comeback scroll's mark and the go-out fallback point (Madrigal).
pub const COMEBACK_POS: Vec3 = Vec3::new(6968.0, 0.0, 3328.8);

/// The packet operations of `GuildHousePacketReq` / `GuildHousePacketNtf` (`GUILDHOUSE_PCKTTYPE_*`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhOp {
    /// A furniture item was listed (server to client only).
    ListUp = 0,
    /// An entry left the list (expiry; server to client only).
    ListDrop = 1,
    /// Install an entry.
    Setup = 2,
    /// Dismantle an entry.
    Remove = 3,
    /// Move an installed entry.
    Reset = 4,
    /// The object id and the remaining time of an installed entry (server to client only).
    Refresh = 5,
    /// Pay upkeep days.
    Upkeep = 6,
    /// The house expired (server to client only).
    Expired = 7,
}

impl GhOp {
    /// The operation of a wire value.
    pub fn from_i32(v: i32) -> Option<GhOp> {
        Some(match v {
            0 => GhOp::ListUp,
            1 => GhOp::ListDrop,
            2 => GhOp::Setup,
            3 => GhOp::Remove,
            4 => GhOp::Reset,
            5 => GhOp::Refresh,
            6 => GhOp::Upkeep,
            7 => GhOp::Expired,
            _ => return None,
        })
    }
}

// ---- the model ------------------------------------------------------------------------------------------------------

/// One entry of the furniture list (`GH_Fntr_Info`): its index in the list is the client's index.
#[derive(Clone, Debug, PartialEq)]
pub struct GhFurniture {
    /// The sequence number (`meta.next_guild_furniture_seq`), the key of the row.
    pub seq: u32,
    /// The item id.
    pub item: u32,
    /// `true` while the piece stands in the house.
    pub installed: bool,
    /// The position in the house.
    pub pos: Vec3,
    /// The rotation.
    pub angle: f32,
    /// The end of the keep time; `UnixTime(0)` = the countdown has not started (it starts at the first setup).
    pub keep_until: UnixTime,
    /// When the piece was listed.
    pub listed_at: UnixTime,
    /// When the piece was last installed.
    pub installed_at: Option<UnixTime>,
    /// The object of the installed piece in the room (runtime only; papering has none).
    pub obj: Option<ObjId>,
}

impl GhFurniture {
    /// A freshly listed piece: not installed, no keep time.
    pub fn listed(seq: u32, item: u32, now: UnixTime) -> GhFurniture {
        GhFurniture {
            seq,
            item,
            installed: false,
            pos: Vec3::ZERO,
            angle: 0.0,
            keep_until: UnixTime(0),
            listed_at: now,
            installed_at: None,
            obj: None,
        }
    }

    fn from_record(r: &GuildHouseFurnitureRecord) -> GhFurniture {
        GhFurniture {
            seq: r.seq,
            item: r.item_id,
            installed: r.installed,
            pos: Vec3::new(r.pos[0], r.pos[1], r.pos[2]),
            angle: r.angle,
            keep_until: r.keep_until,
            listed_at: r.listed_at,
            installed_at: r.installed_at,
            obj: None,
        }
    }

    /// The row of the entry in the database.
    pub fn record(&self, guild: GuildId) -> GuildHouseFurnitureRecord {
        GuildHouseFurnitureRecord {
            seq: self.seq,
            guild_id: guild,
            item_id: self.item,
            installed: self.installed,
            pos: [self.pos.x, self.pos.y, self.pos.z],
            angle: self.angle,
            keep_until: self.keep_until,
            listed_at: self.listed_at,
            installed_at: self.installed_at,
        }
    }
}

/// A guild house (`CGuildHouseSmall`).
#[derive(Clone, Debug, PartialEq)]
pub struct GuildHouse {
    /// The owning guild; also the layer of the room.
    pub guild: GuildId,
    /// `WI_GUILDHOUSE_SMALL`.
    pub world: WorldId,
    /// The end of the paid upkeep; `UnixTime(0)` = expired.
    pub upkeep_until: UnixTime,
    /// When the house was bought (written back unchanged).
    pub created_at: UnixTime,
    /// The furniture list; the position is the client's index.
    pub furniture: Vec<GhFurniture>,
}

impl GuildHouse {
    /// The row of the house in the database.
    pub fn record(&self) -> GuildHouseRecord {
        GuildHouseRecord {
            guild_id: self.guild,
            world_id: self.world,
            upkeep_until: self.upkeep_until,
            created_at: self.created_at,
        }
    }

    /// The pieces that are installed.
    pub fn installed(&self) -> impl Iterator<Item = (usize, &GhFurniture)> {
        self.furniture
            .iter()
            .enumerate()
            .filter(|(_, f)| f.installed)
    }

    /// `true` while the upkeep has run out.
    pub fn is_expired(&self) -> bool {
        self.upkeep_until.0 <= 0
    }
}

/// The guild houses (`Services.event.guild_houses`, `CGuildHouseMng`).
#[derive(Debug, Default)]
pub struct GuildHouseService {
    houses: BTreeMap<GuildId, GuildHouse>,
    next_seq: u32,
}

impl GuildHouseService {
    /// The service of the stored rows: every house with its furniture in `seq` order, the sequence counter above the
    /// stored one (`counters.next_guild_furniture_seq`, and never at or below a stored seq).
    pub fn load(startup: &StartupState) -> GuildHouseService {
        let mut next_seq = startup.counters.next_guild_furniture_seq.max(1);
        let mut houses = BTreeMap::new();
        for load in &startup.guild_houses {
            let guild = load.house.guild_id;
            let mut furniture: Vec<GhFurniture> = load
                .furniture
                .iter()
                .map(GhFurniture::from_record)
                .collect();
            furniture.sort_by_key(|f| f.seq);
            for f in &furniture {
                next_seq = next_seq.max(f.seq.saturating_add(1));
            }
            houses.insert(
                guild,
                GuildHouse {
                    guild,
                    world: WORLD,
                    upkeep_until: load.house.upkeep_until,
                    created_at: load.house.created_at,
                    furniture,
                },
            );
        }
        GuildHouseService { houses, next_seq }
    }

    /// The house of `guild`.
    pub fn house(&self, guild: GuildId) -> Option<&GuildHouse> {
        self.houses.get(&guild)
    }

    /// `true` when `guild` owns a house (an expired one included).
    pub fn has_house(&self, guild: GuildId) -> bool {
        self.houses.contains_key(&guild)
    }

    /// Every house in guild-id order.
    pub fn houses(&self) -> impl Iterator<Item = &GuildHouse> {
        self.houses.values()
    }

    /// How many houses exist.
    pub fn len(&self) -> usize {
        self.houses.len()
    }

    /// `true` when no guild owns a house.
    pub fn is_empty(&self) -> bool {
        self.houses.is_empty()
    }

    /// The next furniture sequence number (the counter row is raised by the batch that writes it).
    pub fn next_seq(&self) -> u32 {
        self.next_seq
    }

    pub(crate) fn house_mut(&mut self, guild: GuildId) -> Option<&mut GuildHouse> {
        self.houses.get_mut(&guild)
    }

    pub(crate) fn insert(&mut self, house: GuildHouse) {
        self.houses.insert(house.guild, house);
    }

    pub(crate) fn remove(&mut self, guild: GuildId) -> Option<GuildHouse> {
        self.houses.remove(&guild)
    }

    pub(crate) fn alloc_seq(&mut self) -> u32 {
        let seq = self.next_seq.max(1);
        self.next_seq = seq.saturating_add(1);
        seq
    }
}

/// `CGuildHouseMng::IsGuildHouse`: the worlds 212, 213 and 214.
pub fn is_guild_house_world(world: WorldId) -> bool {
    (212..=214).contains(&world.0)
}

// ---- wire builders --------------------------------------------------------------------------------------------------

/// The layer of the room of `guild`: `None` when the id does not fit the layer id (an id above `i32::MAX` gets no
/// room).
pub fn layer_of(guild: GuildId) -> Option<LayerId> {
    i32::try_from(guild.0).ok().filter(|v| *v > 0).map(LayerId)
}

/// The guild of a room layer.
pub fn guild_of_layer(layer: LayerId) -> Option<GuildId> {
    u32::try_from(layer.0).ok().filter(|v| *v > 0).map(GuildId)
}

/// `true` while the room of `guild` exists and is live.
pub fn room_live(worlds: &Worlds, guild: GuildId) -> bool {
    layer_of(guild)
        .and_then(|l| worlds.layer(WORLD, l))
        .is_some_and(|l| l.state == LayerState::Live)
}

/// `true` while the room of `guild` exists (live or waiting for its garbage collection).
pub fn room_exists(worlds: &Worlds, guild: GuildId) -> bool {
    layer_of(guild)
        .and_then(|l| worlds.layer(WORLD, l))
        .is_some()
}

/// The wire entry of `f`, built from the server entry: the object id only when `live` and the entry is installed.
pub(crate) fn fntr_info_at(f: &GhFurniture, live: bool) -> pb::GhFntrInfo {
    pb::GhFntrInfo {
        item_id: f.item,
        setup: f.installed,
        pos: Some(crate::movement::intents::pb_vec(f.pos)),
        angle: f.angle,
        ctrl_obj_id: match (live, f.installed, f.obj) {
            (true, true, Some(o)) => o.0,
            _ => NO_OBJ,
        },
        expire_at: f.keep_until.0,
    }
}

/// The wire entry of `f` of the house of `guild` (the object id follows the live room).
pub(crate) fn fntr_info(worlds: &Worlds, guild: GuildId, f: &GhFurniture) -> pb::GhFntrInfo {
    fntr_info_at(f, room_live(worlds, guild))
}

/// The body of `SerializeAllInfo` for `house`.
pub fn house_info(worlds: &Worlds, house: &GuildHouse) -> pb::GuildHouseInfo {
    let live = room_live(worlds, house.guild);
    pb::GuildHouseInfo {
        id_guild: house.guild.0,
        world_id: house.world.0,
        upkeep_at: house.upkeep_until.0,
        furniture: house
            .furniture
            .iter()
            .map(|f| fntr_info_at(f, live))
            .collect(),
    }
}

/// `GuildHouseAllInfo` for the members of `guild` (`house` absent without one).
pub(crate) fn all_info(
    svc: &Services,
    ctx: &GameCtx<'_>,
    guild: Option<GuildId>,
) -> pb::GuildHouseAllInfo {
    pb::GuildHouseAllInfo {
        bank_enabled: guild_bank_on(ctx),
        house: guild
            .and_then(|g| svc.event.guild_houses.house(g))
            .map(|h| house_info(ctx.worlds, h)),
    }
}

/// `events.guild_bank` (flag 2, `ENABLE_GUILD_INVENTORY`).
pub(crate) fn guild_bank_on(ctx: &GameCtx<'_>) -> bool {
    ctx.live
        .event_state(flyff_common::config::EventId::GuildInventory)
        != 0
}

/// `GuildHousePacketNtf{op, index, info}` to every online member of `guild` (wire `obj_id` 0).
pub(crate) fn notify(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    op: GhOp,
    index: i32,
    info: pb::GhFntrInfo,
) {
    ctx.broadcast(
        Some(svc),
        ObjId::NULL,
        Audience::Guild(guild),
        pb::GuildHousePacketNtf {
            op: op as i32,
            index,
            info: Some(info),
        },
    );
}

/// The ticket continuation of a guild-house batch: a refusal is a bug, the memory is kept (21 §2.4).
pub(crate) fn log_result(
    name: &'static str,
) -> impl FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) + Send + 'static {
    move |_svc, _ctx, result| match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => {
            tracing::error!(
                error = %e,
                batch = name,
                "CRITICAL: a guild-house batch failed: the memory is kept (a bug, 21 §2.4)"
            );
        }
        other => tracing::error!(
            ?other,
            batch = name,
            "a guild-house batch completed unexpectedly"
        ),
    }
}

/// An actor-less batch (expiry, a GM call) with a ticket that logs the answer.
pub(crate) fn send_batch(ctx: &mut GameCtx<'_>, name: &'static str, batch: flyff_db::AtomicBatch) {
    let ticket = ctx.tickets.issue(crate::tickets::Pending::Service(
        crate::tickets::ServiceTicket::new(log_result(name)),
    ));
    ctx.persist.atomic(batch.with_ticket(ticket));
}

// ---- P11: expiry ----------------------------------------------------------------------------------------------------

/// P11 `event::guild_house::expiry_step` (22 §12.6, every 75 ticks): per house in guild order, the FIRST furniture
/// entry whose keep time ran out leaves the list (LISTDROP), then a house whose upkeep ran out expires (EXPIRED)
/// (`CGuildHouseMng::ProcessExpired`, two requests of the database in that order).
pub fn expiry_step(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if svc.event.guild_houses.is_empty() {
        return;
    }
    let now = now_secs(ctx);
    let guilds: Vec<GuildId> = svc.event.guild_houses.houses().map(|h| h.guild).collect();
    for guild in guilds {
        members::drop_first_expired(svc, ctx, guild, now);
        members::expire_house(svc, ctx, guild, now);
    }
}
