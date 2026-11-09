//! The item spawns of the generic events (`CSpawn`, `CEventGeneric::Spawn`; Plan/22 §6.3, ref/15 §5;
//! src:_Common/spevent.cpp:536-638, src:_Common/spevent.h:85-130, src:WORLDSERVER/DPDatabaseClient.cpp:1785-1825).
//!
//! While the flag of a spawn event is on, P16 [`spawn_tick`] drops one world item every `86 400 000 / MAX`
//! milliseconds into a random monster region of any world, the region chosen by the quadratic level weighting of
//! [`spawn_index`]; the item lies on the ground for 24 hours. Only `OT_ITEM` spawns exist; other object types are
//! dropped silently (the original deleted the object it had just created).

use super::GenericEventService;
use crate::ctx::{GameCtx, Services};
use crate::item::GroundItemMeta;
use crate::item::container::SerialSource;
use crate::item::ops::CtxSerials;
use crate::world::Worlds;
use crate::world::object::ObjKind;
use crate::world::respawn::{RESPAWN_POINTS, RespawnTable};
use flyff_common::defs::job::MAX_MONSTER_LEVEL;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, LayerId, WorldId};
use flyff_data::GameData;
use flyff_data::events::generic_event::EventSpawn;
use flyff_db::ItemRecord;
use std::sync::Arc;

/// How long a spawned item stays: `g_tmCurrent + MIN( 1440 )` as its drop time, so the 180 s expiry of a ground item
/// starts a day later (KEEP).
const SPAWN_KEEP_MS: u64 = 86_400_000;

/// One `CSpawn`: what spawns and when (the pacing state is the timeout).
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnPacer {
    /// The object type (`OT_ITEM` is the only one that spawns).
    pub object_type: u32,
    /// The object index (the item id).
    pub index: u32,
    /// Spawns a day (`m_nMax`).
    pub max: i32,
    /// The spawn weight of the highest region level against the lowest (`m_fRatio`).
    pub ratio: f32,
    /// `m_dwInterval`: `86 400 000 / max` ms between two spawns (0 = never).
    pub interval: u32,
    /// `m_dwTimeout`: 0 at load, so the first check spawns.
    pub timeout: GameMs,
}

impl SpawnPacer {
    /// The pacer of a `Spawn` row.
    pub fn new(s: &EventSpawn) -> SpawnPacer {
        SpawnPacer {
            object_type: s.object_type,
            index: s.index,
            max: s.max,
            ratio: s.ratio,
            interval: s.interval_ms,
            timeout: GameMs(0),
        }
    }

    /// `CSpawn::IsTimeout`: due when `now >= timeout`; the run sets `timeout = now + interval`. An interval of 0
    /// never spawns (FIX: the original spawned on every tick).
    pub fn is_timeout(&mut self, now: GameMs) -> bool {
        if self.interval == 0 || now < self.timeout {
            return false;
        }
        self.timeout = GameMs(now.0.saturating_add(u64::from(self.interval)));
        true
    }
}

/// One `REGION_GENERIC`: a monster respawn line of a world with the level of its monster.
#[derive(Clone, Debug)]
pub struct SpawnRegion {
    /// The level of the line's monster (`1..=160`).
    pub level: u32,
    /// The world the line belongs to.
    pub world: WorldId,
    /// The respawn table of the world (shared with the world).
    pub table: Arc<RespawnTable>,
    /// The index of the line in `table.protos`.
    pub line: usize,
}

impl SpawnRegion {
    /// `CRespawnInfo::GetPos( v, TRUE )`: the fixed position of a one-monster line, else one of its 100 precomputed
    /// points (`xRandom( 0, MAX_RESPOINT_PER_REGION )`). `None` when the line is gone.
    pub fn pick(&self, rng: &mut dyn GameRng) -> Option<(f32, f32)> {
        let p = self.table.protos.get(self.line)?;
        if p.fixed {
            return Some((p.vpos.x, p.vpos.z));
        }
        let r = rng.random_below(RESPAWN_POINTS as u32) as usize;
        p.points.get(r).map(|&(x, z)| (x as f32, z as f32))
    }
}

/// The regions of the generic spawns (`OnEventStateLua` + `SortRegionGeneric`): for every loaded world in ascending
/// id every mover line of a non-flying monster of level `1..=MAX_MONSTER_LEVEL`, then a STABLE sort by level
/// (the original `std::sort` was unstable: invisible).
pub fn spawn_regions(worlds: &Worlds, data: &GameData) -> Vec<SpawnRegion> {
    let mut out = Vec::new();
    for id in worlds.world_ids() {
        let Some(w) = worlds.world(id) else { continue };
        for (line, p) in w.respawn.protos.iter().enumerate() {
            if p.kind != ObjKind::Mover || p.flying {
                continue;
            }
            let Some(prop) = data.mover(p.index) else {
                continue;
            };
            if prop.level == 0 || prop.level > MAX_MONSTER_LEVEL {
                continue;
            }
            out.push(SpawnRegion {
                level: prop.level,
                world: id,
                table: Arc::clone(&w.respawn),
                line,
            });
        }
    }
    out.sort_by_key(|r| r.level);
    out
}

/// `CSpawn::GetMaxSpawnProbability( n )`: `(DWORD)( ( n + ( ratio - 1 ) * ( n - 1 ) / 2 ) * 100 )` in `f32`.
pub fn max_spawn_probability(n: usize, ratio: f32) -> u32 {
    let nf = n as f32;
    let span = n.saturating_sub(1) as f32;
    ((nf + (ratio - 1.0) * span / 2.0) * 100.0) as u32
}

/// `CSpawn::GetSpawnIndex( n )` for the draw `prob = xRand() % max_spawn_probability`: `c = (ratio - 1) / n`,
/// `D = (c + 2)^2 - 4c(2 - prob / 50)`, `idx = (int)((-(2 + c) +- sqrt D) / (2c) + 1)` (`+` iff `D > 0`); every step
/// in `f32`; an index outside `0..n` (including NaN) is 0. KEEP-QUIRK: ratio 1 makes `c` 0, so the quotient is
/// `0/0`: NaN, always index 0.
pub fn spawn_index(n: usize, ratio: f32, prob: u32) -> usize {
    let c = (ratio - 1.0) / n as f32;
    let t = c + 2.0;
    let d = t * t - 4.0 * c * (2.0 - prob as f32 / 50.0);
    let root = d.sqrt();
    let x = if d > 0.0 {
        (-(2.0 + c) + root) / (2.0 * c) + 1.0
    } else {
        (-(2.0 + c) - root) / (2.0 * c) + 1.0
    };
    // `as i32` saturates and maps NaN to 0, the original's undefined cast landed on a negative index: both give 0
    let idx = x as i32;
    match usize::try_from(idx) {
        Ok(i) if i < n => i,
        _ => 0,
    }
}

/// P16 `event::generic::spawn_tick` (22 §2.3, §6.3), every tick: returns at once unless the flag of some spawn event
/// is on. The regions are built at the first need. For every due pacer of an event whose flag is on, in ascending
/// event id: the region by [`spawn_index`] (draw `event_rng % max_spawn_probability`; none when that is 0), the
/// position by [`SpawnRegion::pick`], `y = land_height + 1`, and for `OT_ITEM` one item (count 1, a fresh serial,
/// the endurance of the prop) on the ground of layer 0 with no owner and `drop_time = now + 24 h`.
pub fn spawn_tick(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let data = ctx.data;
    let Services { event, serials, .. } = s;
    event.generic.ensure(data);
    if !event
        .generic
        .spawns
        .keys()
        .any(|id| event.flags.get(*id) != 0)
    {
        return;
    }
    let GenericEventService {
        spawns, regions, ..
    } = &mut event.generic;
    let regions = regions.get_or_insert_with(|| spawn_regions(ctx.worlds, data));
    if regions.is_empty() {
        return;
    }
    let now = ctx.clock.game_ms;
    for (id, pacer) in spawns.iter_mut() {
        if event.flags.get(*id) == 0 || !pacer.is_timeout(now) {
            continue;
        }
        let n = regions.len();
        let top = max_spawn_probability(n, pacer.ratio);
        let rng = ctx.rng.event();
        let prob = if top == 0 { 0 } else { rng.next_u32() % top };
        let Some(region) = regions.get(spawn_index(n, pacer.ratio, prob)) else {
            continue;
        };
        let Some((x, z)) = region.pick(rng) else {
            continue;
        };
        let Some(land) = ctx
            .worlds
            .world(region.world)
            .map(|w| w.statics.land_height(x, z))
        else {
            continue;
        };
        if pacer.object_type != u32::from(ObjKind::Item.ot()) {
            // monsters and controls were never implemented by the original either
            continue;
        }
        let Some(prop) = data.item(pacer.index) else {
            tracing::warn!(
                item = pacer.index,
                "generic event spawn: the item has no prop"
            );
            continue;
        };
        let mut record = ItemRecord::new(0, pacer.index, 1, 0);
        record.endurance = i32::try_from(prop.endurance).unwrap_or(i32::MAX);
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        record.serial = src.next_serial().unwrap_or(0);
        let meta = GroundItemMeta {
            owner: None,
            drop_time: GameMs(now.0.saturating_add(SPAWN_KEEP_MS)),
            ..GroundItemMeta::default()
        };
        let pos = Vec3::new(x, land + 1.0, z);
        if let Err(e) =
            ctx.worlds
                .spawn_ground_item(region.world, LayerId(0), pos, 0.0, record, meta)
        {
            tracing::debug!(world = region.world.0, error = %e, "generic event spawn refused");
        }
    }
}
