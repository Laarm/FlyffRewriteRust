//! World commands of the console (Plan/25 §3.12, §7.1-§7.2, §11.5):
//!
//! * `TeleportCharacter`, `SpawnMonsters`, `RespawnRegion`, `DropItem`: **WP-ADMIN-12** (the move is `Worlds::replace`
//!   as `/te` does it, the spawn is `/cn`'s builder, the respawner is 15 §15, the ground item is 19 §6);
//! * `SetWeather`: **WP-ADMIN-30**, phase-gated on WP-GM-30 (the weather service): until that work package fills
//!   the body, the command answers `not_implemented` naming WP-GM-30 and the console shows "needs WP-GM-30";
//! * `SetSpawnScale`: **WP-ADMIN-31** (MAY).
//!
//! Every body validates before it changes anything (see [`super::common`]). A teleport answers one
//! [`TargetResult`] per character with `info = {from, to}` (the audit of 25 §10.3 and the
//! "teleport back" undo of 25 §2.5 read it); a spawn answers [`AdminOutput::Spawned`] with the new object ids.

use super::common::{Place, check_group, check_rank, fail, failed, online_player, report};
use super::{TargetId, TargetResult, not_implemented};
use crate::admin::types::AdminErrorCode;
use crate::ai::spawn::{MonsterSpawn, build};
use crate::chat::ingress::audit as chat_audit;
use crate::ctx::{GameCtx, Services};
use crate::gm::cmd::spawn::{MAX_SPAWN, spawnable};
use crate::input::{AdminActor, AdminOutput};
use crate::item::GroundItemMeta;
use crate::sched::{TimerEvent, TimerScope, timers};
use crate::world::continent::{CONT_FLARIS, Continents};
use crate::world::{ReplaceError, ReplaceKind};
use flyff_common::config::EventId;
use flyff_common::defs::itemkind::IK1_GOLD;
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{CharacterId, LayerId, ObjId, WorldId};
use flyff_db::StoredItem;
use serde_json::json;
use std::sync::Arc;

/// The largest scatter radius of a spawn (metres; the picker of 25 §3.11 offers 0-30).
pub const MAX_SPREAD_M: f32 = 30.0;
/// The longest lifetime of a spawn (30 days).
pub const MAX_LIFETIME_S: u32 = 30 * 24 * 3600;

/// Where a teleport goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TeleportMode {
    /// To the `world`, `layer` and `pos` of the command (a point or a quick location).
    #[default]
    Point,
    /// "Return to town" (25 §3.12): each character goes to the revival point of its own continent
    /// ([`unstuck_destination`]); `world`, `layer` and `pos` of the command are ignored.
    Unstuck,
}

/// Teleport characters to a point (`TeleportCharacter`, WP-ADMIN-12). Online characters move at once; offline ones are
/// the `OfflineCharacterEdit::SetPosition` of WP-ADMIN-16.
#[derive(Clone, Debug, PartialEq)]
pub struct TeleportCharacter {
    /// Who moves.
    pub characters: Vec<CharacterId>,
    /// Destination world.
    pub world: WorldId,
    /// Destination layer.
    pub layer: LayerId,
    /// Destination position (metres).
    pub pos: Vec3,
    /// A point, or the revival point of the character's continent.
    pub mode: TeleportMode,
}

/// Spawn monsters (`SpawnMonsters`, WP-ADMIN-12; answers `AdminOutput::Spawned`).
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnMonsters {
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// Centre (metres).
    pub pos: Vec3,
    /// `MoverProp` id.
    pub prop: u32,
    /// How many.
    pub count: u32,
    /// Scatter radius in metres.
    pub spread: f32,
    /// Spawn them aggressive.
    pub aggressive: bool,
    /// Despawn them after this many seconds; `None` = they stay.
    pub lifetime_s: Option<u32>,
}

/// Respawn a region now (`RespawnRegion`, WP-ADMIN-12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RespawnRegion {
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// The respawn line (`gm_index`; the `spawn_region` of an `EntityDetail` is this plus one).
    pub region: u32,
}

/// Drop an item on the ground (`DropItem`, WP-ADMIN-12). The item is already validated and normalised
/// (`POST /admin/api/items/validate`, WP-ADMIN-20); the body checks only that it exists and fits a stack.
#[derive(Clone, Debug, PartialEq)]
pub struct DropItem {
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// Position (metres).
    pub pos: Vec3,
    /// The item (boxed: it is by far the largest payload of `AdminCmd`).
    pub item: Box<StoredItem>,
    /// The character with loot priority; `None` = anyone.
    pub owner: Option<CharacterId>,
}

/// `SetWeather.kind`: no weather effect.
pub const WEATHER_CLEAR: u8 = 0;
/// `SetWeather.kind`: rain (`world::environment::fall_rain`).
pub const WEATHER_RAIN: u8 = 1;
/// `SetWeather.kind`: snow (an accepted no-op in the shipped build, 23 rows 39-40).
pub const WEATHER_SNOW: u8 = 2;

/// `true` once the weather service exists and `set_weather` has a body (WP-GM-30 flips it together with the body).
/// The console shows the weather controls only then (`EventInfo.weather.control`, `PUT /admin/api/weather/{world}`).
pub const WEATHER_CONTROL: bool = false;

/// Set the weather of a world (`SetWeather`, WP-ADMIN-30).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetWeather {
    /// World.
    pub world: WorldId,
    /// The weather kind: [`WEATHER_CLEAR`], [`WEATHER_RAIN`] or [`WEATHER_SNOW`].
    pub kind: u8,
}

/// Scale the population of a respawn line for a while (`SetSpawnScale`, WP-ADMIN-31, MAY).
#[derive(Clone, Debug, PartialEq)]
pub struct SetSpawnScale {
    /// The respawn line.
    pub region: u32,
    /// The factor on the maximum population.
    pub factor: f32,
    /// Until this tick; `None` = until reset.
    pub until: Option<u64>,
}

// ---- places -----------------------------------------------------------------------------------------------------------

/// Checks that `(world, layer, pos)` is somewhere an object can be put: the world is loaded, the layer exists and the
/// position is inside the world (VR-082).
fn check_place(
    ctx: &GameCtx<'_>,
    world: WorldId,
    layer: LayerId,
    pos: Vec3,
) -> Result<(), crate::admin::types::AdminFailure> {
    if world.0 == 0 || world.0 == u32::MAX {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!("world {} is not loaded", world.0),
        ));
    }
    let Some(w) = ctx.worlds.world(world) else {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!("world {} is not loaded", world.0),
        ));
    };
    if !w.layers.contains_key(&layer) {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!("world {} has no layer {}", world.0, layer.0),
        ));
    }
    if !w.statics.in_world3(pos) {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!(
                "({:.1}, {:.1}, {:.1}) is outside world {}",
                pos.x, pos.y, pos.z, world.0
            ),
        ));
    }
    Ok(())
}

/// "Return to town" for a mover at `(world, pos)` (25 §3.12, 15 §12.2, §12.4): in Madrigal the revival point of the
/// continent the position is in ([`Continents::revival_pos`], Flaris for the unmarked parts); in any other world the
/// world's own revival region (`select_revival`, the rule of the lodestar revival); a world without one sends the
/// character to Flaris. `None` when no destination world is loaded. The result is `(world, layer 0, position)`; a
/// zero height is resolved by the move itself.
pub fn unstuck_destination(
    worlds: &crate::world::Worlds,
    world: WorldId,
    pos: Vec3,
) -> Option<(WorldId, LayerId, Vec3)> {
    let madrigal = WorldId(WI_WORLD_MADRIGAL);
    if world == madrigal {
        let town = Continents::revival_pos(worlds.continents.continent_at(pos));
        return worlds.world(madrigal).map(|_| (madrigal, LayerId(0), town));
    }
    if let Some(w) = worlds.world(world)
        && let Some(p) = worlds
            .revival
            .select(world, &w.statics.settings, pos, false)
        && worlds.world(p.world).is_some()
    {
        return Some((p.world, LayerId(0), p.pos));
    }
    worlds
        .world(madrigal)
        .map(|_| (madrigal, LayerId(0), Continents::revival_pos(CONT_FLARIS)))
}

// ---- TeleportCharacter ------------------------------------------------------------------------------------------------

/// `TeleportCharacter`: every online character in the list moves with `REPLACE_FORCE` (the move of `/te`, 15 §10: the
/// same world and layer is a `SetPos`, anything else a `Replace`; both reach the player in the same tick). A character
/// that is offline, unknown or outranks the actor is reported and skipped; a destination that is wrong for everybody
/// (the world is not loaded, no such layer, outside the world) fails the whole command before anybody moves. An
/// instance-dungeon world is entered only by a character that is inside it already (the rule of `/te`, 23 row 28).
pub(super) fn teleport_character(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: TeleportCharacter,
) -> AdminOutput {
    if let Err(f) = check_group(cmd.characters.len(), "characters") {
        return AdminOutput::Failed(f);
    }
    if cmd.mode == TeleportMode::Point
        && let Err(f) = check_place(ctx, cmd.world, cmd.layer, cmd.pos)
    {
        return AdminOutput::Failed(f);
    }
    let mut per_target = Vec::with_capacity(cmd.characters.len());
    for id in cmd.characters.iter().copied() {
        let target = TargetId::Character(id);
        per_target.push(match teleport_one(svc, ctx, &actor, id, &cmd) {
            Ok(info) => TargetResult::ok(target).with_info(info),
            Err(f) => TargetResult::failed(target, f),
        });
    }
    report(per_target)
}

fn teleport_one(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    actor: &AdminActor,
    id: CharacterId,
    cmd: &TeleportCharacter,
) -> Result<serde_json::Value, crate::admin::types::AdminFailure> {
    let who = online_player(svc, ctx, id)?;
    check_rank(actor, ctx, who.0)?;
    let from = ctx
        .worlds
        .player(who.0)
        .map(Place::of)
        .ok_or_else(|| fail(AdminErrorCode::StaleTarget, "the character left the world"))?;
    let (world, layer, pos) = match cmd.mode {
        TeleportMode::Point => (cmd.world, cmd.layer, cmd.pos),
        TeleportMode::Unstuck => {
            let (w, l, p) =
                unstuck_destination(ctx.worlds, from.world, from.pos).ok_or_else(|| {
                    fail(
                        AdminErrorCode::Invalid,
                        "no town is loaded to return the character to",
                    )
                })?;
            check_place(ctx, w, l, p)?;
            (w, l, p)
        }
    };
    if world != from.world && svc.gm.instances.is_instance_world(world) {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!(
                "world {} is an instance dungeon: it cannot be entered by a teleport",
                world.0
            ),
        ));
    }
    ctx.worlds
        .replace(who.0, world, pos, layer, ReplaceKind::Force)
        .map_err(|e| match e {
            ReplaceError::NotInWorld => {
                fail(AdminErrorCode::StaleTarget, "the character left the world")
            }
            other => fail(AdminErrorCode::Invalid, other.to_string()),
        })?;
    let to = Place { world, layer, pos };
    Ok(json!({"from": from.json(), "to": to.json()}))
}

// ---- SpawnMonsters ----------------------------------------------------------------------------------------------------

/// A point of the disc of radius `spread` around `centre` (rejection sampling in the square, so no transcendental
/// function is involved and the stream of the world's random generator stays portable); the centre itself after
/// eight misses.
fn scatter(rng: &mut dyn GameRng, centre: Vec3, spread: f32) -> Vec3 {
    if spread <= 0.0 {
        return centre;
    }
    for _ in 0..8 {
        let dx = rng.random_f32(2.0 * spread) - spread;
        let dz = rng.random_f32(2.0 * spread) - spread;
        if dx * dx + dz * dz <= spread * spread {
            return Vec3::new(centre.x + dx, centre.y, centre.z + dz);
        }
    }
    centre
}

/// `SpawnMonsters`: `count` monsters of `prop` around the point, `spread` metres at most from it, at the layer's
/// land height; only the AI kinds `/createnpc` knows (23 row 74) spawn. They carry `level * 15` penya, are no member
/// of a respawn line and stand until the lifetime ends. Answers [`AdminOutput::Spawned`] with the ids; the monsters
/// appear to their viewers at the next `_add` of the layer.
pub(super) fn spawn_monsters(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: SpawnMonsters,
) -> AdminOutput {
    if cmd.count == 0 || cmd.count > MAX_SPAWN {
        return failed(
            AdminErrorCode::Invalid,
            format!("count must be 1..={MAX_SPAWN}"),
        );
    }
    if !cmd.spread.is_finite() || !(0.0..=MAX_SPREAD_M).contains(&cmd.spread) {
        return failed(
            AdminErrorCode::Invalid,
            format!("spread must be 0..={MAX_SPREAD_M} m"),
        );
    }
    if let Some(s) = cmd.lifetime_s
        && !(1..=MAX_LIFETIME_S).contains(&s)
    {
        return failed(
            AdminErrorCode::Invalid,
            format!("lifetime must be 1..={MAX_LIFETIME_S} s"),
        );
    }
    let data = Arc::clone(ctx.data);
    let Some(prop) = data.mover(cmd.prop) else {
        return failed(
            AdminErrorCode::Invalid,
            format!("monster {} does not exist", cmd.prop),
        );
    };
    if cmd.prop == 0 || !spawnable(prop.ai) {
        return failed(
            AdminErrorCode::Invalid,
            format!("{} is not a monster that can be spawned", prop.name),
        );
    }
    if let Err(f) = check_place(ctx, cmd.world, cmd.layer, cmd.pos) {
        return AdminOutput::Failed(f);
    }
    // `SetGold(level * 15)` with the C `int` wrap, as `/createnpc`
    let gold = i32::try_from(prop.level)
        .unwrap_or(i32::MAX)
        .wrapping_mul(15);
    let inside = |ctx: &GameCtx<'_>, p: Vec3| {
        ctx.worlds
            .world(cmd.world)
            .is_some_and(|w| w.statics.in_world3(p))
    };
    let mut ids: Vec<ObjId> = Vec::with_capacity(cmd.count as usize);
    for _ in 0..cmd.count {
        let mut pos = scatter(ctx.rng.world(cmd.world), cmd.pos, cmd.spread);
        if !inside(ctx, pos) {
            pos = cmd.pos;
        }
        let spec = MonsterSpawn {
            mover: cmd.prop,
            world: cmd.world,
            layer: cmd.layer,
            pos,
            active_attack: cmd.aggressive,
            gold: Some(gold),
            owner_ai: None,
        };
        let Some(m) = build(&data, ctx.rng.world(cmd.world), &spec) else {
            break;
        };
        match ctx.worlds.spawn_mover(cmd.world, cmd.layer, m) {
            Ok(id) => ids.push(id),
            Err(e) => {
                tracing::warn!(prop = cmd.prop, error = %e, "an admin spawn was refused");
                break;
            }
        }
    }
    if ids.is_empty() {
        return failed(
            AdminErrorCode::Invalid,
            "the world refused the spawn (no free object ids or a bad position)",
        );
    }
    if let Some(s) = cmd.lifetime_s {
        for id in &ids {
            ctx.sched.after_ms(
                ctx.clock,
                u64::from(s) * 1000,
                TimerEvent::Gm(timers::gm::Timer::DespawnSpawned(*id)),
                TimerScope::default(),
            );
        }
    }
    tracing::debug!(prop = cmd.prop, made = ids.len(), "admin spawned monsters");
    AdminOutput::Spawned { ids }
}

/// The one-shot of the GM tools (`TimerEvent::Gm`): a monster the console spawned with a lifetime is removed when it
/// is still there. Object ids are never reused (15 §5.4), so a timer for a monster that died is harmless.
pub fn on_timer(_svc: &mut Services, ctx: &mut GameCtx<'_>, t: timers::gm::Timer) {
    match t {
        timers::gm::Timer::DespawnSpawned(id) => {
            let removable = ctx
                .worlds
                .mover(id)
                .is_some_and(|m| !m.core.deleted && !m.is_player());
            if removable {
                ctx.worlds.delete(id);
            }
        }
    }
}

// ---- RespawnRegion ----------------------------------------------------------------------------------------------------

/// `RespawnRegion`: the respawn line is due at the next respawn pass of the layer (the P1 job runs once a second):
/// its countdown is zero and the "half" alternation is off, so that pass refills the line to its maximum. Answers
/// [`AdminOutput::Done`] with the number of objects the line is missing now. Respawning must be on (`events.respawn`).
pub(super) fn respawn_region(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: RespawnRegion,
) -> AdminOutput {
    if ctx.live.event_state(EventId::Respawn) == 0 {
        return failed(
            AdminErrorCode::Invalid,
            "respawning is switched off (events.respawn)",
        );
    }
    let Some(w) = ctx.worlds.world(cmd.world) else {
        return failed(
            AdminErrorCode::Invalid,
            format!("world {} is not loaded", cmd.world.0),
        );
    };
    if !w.layers.contains_key(&cmd.layer) {
        return failed(
            AdminErrorCode::Invalid,
            format!("world {} has no layer {}", cmd.world.0, cmd.layer.0),
        );
    }
    let index = cmd.region as usize;
    let Some(proto) = w.respawn.protos.get(index) else {
        return failed(
            AdminErrorCode::NotFound,
            format!(
                "world {} has no respawn line {} ({} lines)",
                cmd.world.0,
                cmd.region,
                w.respawn.len()
            ),
        );
    };
    let max = proto.max_count;
    let Some(st) = ctx
        .worlds
        .layer_mut(cmd.world, cmd.layer)
        .and_then(|l| l.respawner.states.get_mut(index))
    else {
        return failed(
            AdminErrorCode::NotFound,
            format!("layer {} has no state for line {}", cmd.layer.0, cmd.region),
        );
    };
    st.countdown = 0;
    st.half = false;
    let missing = (max - st.alive).max(0);
    AdminOutput::Done {
        affected: u32::try_from(missing).unwrap_or(0),
    }
}

// ---- DropItem ---------------------------------------------------------------------------------------------------------

/// `DropItem`: one ground item at the point, owned by nobody or by an online character for the loot-priority window
/// (19 §6.5). The record keeps what the validated item carries; its serial is zero and is numbered at pickup. Audit
/// `item.create` with `source = "admin"` (25 §3.12). Answers [`AdminOutput::Spawned`] with the id of the item.
pub(super) fn drop_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: DropItem,
) -> AdminOutput {
    if let Err(f) = check_place(ctx, cmd.world, cmd.layer, cmd.pos) {
        return AdminOutput::Failed(f);
    }
    let data = Arc::clone(ctx.data);
    let rec = cmd.item.item;
    let Some(prop) = data.item(rec.item_id) else {
        return failed(
            AdminErrorCode::Invalid,
            format!("item {} does not exist", rec.item_id),
        );
    };
    if prop.item_kind1 == IK1_GOLD {
        return failed(
            AdminErrorCode::Invalid,
            "penya is granted, not dropped: use the grant dialog",
        );
    }
    if rec.count == 0 || u32::from(rec.count) > prop.pack_max.max(1) {
        return failed(
            AdminErrorCode::Invalid,
            format!(
                "{} stacks to {} at most, not {}",
                prop.name,
                prop.pack_max.max(1),
                rec.count
            ),
        );
    }
    let owner = match cmd.owner {
        Some(c) => match online_player(svc, ctx, c) {
            Ok(p) => Some(p.0),
            Err(f) => return AdminOutput::Failed(f),
        },
        None => None,
    };
    let mut rec = rec;
    rec.obj_id = 0;
    rec.serial = 0;
    let (item_id, count) = (rec.item_id, rec.count);
    let meta = GroundItemMeta {
        owner,
        drop_time: ctx.clock.game_ms,
        ..GroundItemMeta::default()
    };
    let id = match ctx
        .worlds
        .spawn_ground_item(cmd.world, cmd.layer, cmd.pos, 0.0, rec, meta)
    {
        Ok(id) => id,
        Err(e) => {
            return failed(
                AdminErrorCode::Invalid,
                format!("the world refused the item: {e}"),
            );
        }
    };
    chat_audit::emit(
        svc,
        ctx,
        "item.create",
        json!({"data": {
            "source": "admin",
            "actor": actor.label(),
            "item": item_id,
            "count": count,
            "obj": id.0,
            "world": cmd.world.0,
            "layer": cmd.layer.0,
            "pos": [cmd.pos.x, cmd.pos.y, cmd.pos.z],
            "owner": cmd.owner.map(|c| c.0),
        }}),
    );
    AdminOutput::Spawned { ids: vec![id] }
}

/// `SetWeather`: phase-gated on WP-GM-30. The effect state and its broadcast exist (`world::environment`, 15 §17.2)
/// but the authority and the 60 s roll of the weather service (22 §17.5, WP-EVENT-22) do not, so a toggle from here
/// would be undone or ignored by the next roll; the body stays a `not_implemented` that names WP-GM-30, and the
/// route (`PUT /admin/api/weather/{world}`) maps it to `501 needs_feature`.
pub(super) fn set_weather(
    _svc: &mut Services,
    _ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    _cmd: SetWeather,
) -> AdminOutput {
    not_implemented("set weather", "WP-GM-30")
}

/// `SetSpawnScale` (stub).
pub(super) fn set_spawn_scale(
    _svc: &mut Services,
    _ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    _cmd: SetSpawnScale,
) -> AdminOutput {
    not_implemented("set spawn scale", "WP-ADMIN-31")
}
