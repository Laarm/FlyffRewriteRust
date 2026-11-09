//! `event::spawns`: the Event.lua spawn events (Plan/22 §4.10, WP-EVENT-06; legacy `CEventLua::SetSpawnEvent`,
//! `EventSpawnProcess`, `DeleteEndEventProcess`, `EventLua.cpp:601-770`).
//!
//! A running event with `SetSpawn(ITEM | MONSTER, id, per_day)` lines creates the item or the monster
//! `86 400 000 / per_day` ms apart at a random point of a random respawn rectangle of Madrigal (world 1, layer 0):
//!
//! * [`boot`] (from `Game::new`, after `Worlds::init_respawn` and the first Event.lua pass) collects the regions
//!   (every `.rgn` mover line of world 1 with a ground-walking prop of level `1..=MAX_MONSTER_LEVEL`) and applies the
//!   events that are running at the start,
//! * [`EventSpawnService::set_spawn_event`] runs for every [`EventChange`] after a state pass: a started event with a
//!   non-empty `GetSpawn` takes fresh entries (they spawn at once), a stopped one moves to the end list,
//! * [`process`] (1 s slot item 14) spawns what is due and removes the monsters of ended events, at most
//!   `100 / ending.len()` per id and second.
//!
//! **Monster rules (KQ-32).** [`EventSpawnFacts`] mirrors the set of mover PROP ids of the registered spawn events
//! for the combat code (`EventCombatHooks`) and the drop code (`EventDrops`): every monster of such a prop, the
//! ordinary respawn ones included, is always hit by players, takes exactly 1 damage per hit (GM one-kill mode
//! excepted) and drops without the level gate while the prop is registered (legacy rule by prop, not by object).
//! No death hook is needed: [`process`] forgets spawned monsters that are gone or dead at the start of every run, the
//! same state the C++ `RemoveSpawnedMonster` at the death produced (ObjIds are never reused within a run).
//!
//! **Deviations from the C++** (recorded in `NOTES.md`): a spawn line with `per_day == 0` is skipped with a warning
//! (the C++ divided by zero); a line with an unknown define or a kind other than ITEM/MONSTER is skipped with a
//! warning (the C++ kept it and never created anything for it); the entries draw their region and position only when
//! due (the C++ drew a region every second for every entry before its time-out test; not visible); a family that is
//! not `Ready` ends every spawn event (22 §2.5 rule 3; the C++ `EventSpawnProcess` had no `IsPossible` test and kept
//! spawning).

use super::EventServices;
use super::lua_events::EventChange;
use super::scripts::EventScripts;
use crate::ai::spawn::{MonsterSpawn, commit};
use crate::ctx::{GameCtx, Services};
use crate::item::GroundItemMeta;
use crate::item::container::SerialSource;
use crate::item::ops::CtxSerials;
use crate::world::{ObjKind, Worlds};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::types::{ItemId, MoverId, NULL_ID};
use flyff_db::ItemRecord;
use flyff_lua::{FamilyStatus, LuaHost, ScriptFamily};
use flyff_rules::combat::rng::x_random_range;
use flyff_rules::consts::MAX_MONSTER_LEVEL;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// `ITEM` of `EventFunc.lua` (`OT_ITEM`).
const KIND_ITEM: u32 = 4;
/// `MONSTER` of `EventFunc.lua` (`OT_MOVER`).
const KIND_MONSTER: u32 = 5;
/// A spawned item stays 24 h (`g_tmCurrent + MIN(1440)`), then the normal 180 s rule of the ground items applies.
const ITEM_KEEP_MS: u64 = 86_400_000;
/// `DeleteEndEventProcess` removes at most `100 / ending.len()` monsters per id and run.
const DELETE_BUDGET: usize = 100;
/// Legacy: `m_vecSpawnRegion` is filled for `WI_WORLD_MADRIGAL` only.
const SPAWN_WORLD: WorldId = WorldId(WI_WORLD_MADRIGAL);
const SPAWN_LAYER: LayerId = LayerId(0);

/// What an entry creates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    /// An item on the ground (`SetSpawn(ITEM, ...)`).
    Item,
    /// A monster (`SetSpawn(MONSTER, ...)`).
    Monster,
}

/// One `SetSpawn` line of a running event (`__SPAWNINFO`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnEntry {
    /// Item or monster.
    pub kind: SpawnKind,
    /// The item or mover prop id.
    pub id: u32,
    /// `86 400 000 / nDayNum` ms.
    pub interval_ms: u64,
    /// The next creation (`dwTimeOut`): 0 at the start, then `now + interval` (`IsTimeOut`; not `+=`).
    pub next_at: GameMs,
}

/// The mover PROP ids of the registered spawn events as an atomic bitset (the `SiegeFacts` pattern): what the combat
/// and drop code ask without a borrow of the service. Sized from the mover table by [`boot`]; an id beyond the
/// size is never a prop of the game.
#[derive(Debug, Default)]
pub struct EventSpawnFacts {
    bits: Vec<AtomicU64>,
}

impl EventSpawnFacts {
    /// A set for ids up to `max_id`.
    pub fn sized(max_id: u32) -> EventSpawnFacts {
        let words = (max_id as usize) / 64 + 1;
        EventSpawnFacts {
            bits: (0..words).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    /// `IsEventSpawnMonster(id)`.
    pub fn contains(&self, id: MoverId) -> bool {
        self.bits
            .get(id as usize / 64)
            .is_some_and(|w| w.load(Ordering::Relaxed) & (1 << (id % 64)) != 0)
    }

    /// Makes the set equal to `ids` (the game thread is the only writer).
    fn replace_with(&self, ids: impl Iterator<Item = MoverId>) {
        for w in &self.bits {
            w.store(0, Ordering::Relaxed);
        }
        for id in ids {
            if let Some(w) = self.bits.get(id as usize / 64) {
                w.fetch_or(1 << (id % 64), Ordering::Relaxed);
            }
        }
    }
}

/// The spawn events of Event.lua (`Services.event.spawns`).
#[derive(Debug, Default)]
pub struct EventSpawnService {
    /// `m_vecSpawnRegion`: the rectangles `[left, top, right, bottom]` of Madrigal, in file order.
    regions: Vec<[i32; 4]>,
    /// `m_mapSpawnList`: the entries of every running spawn event, by event id.
    active: BTreeMap<u8, Vec<SpawnEntry>>,
    /// `m_vecEndSpawnEvent`: the events whose monsters are being removed.
    ending: Vec<u8>,
    /// `m_mapSpawnedMonster`: the monsters this service created, with the event that made them.
    spawned: BTreeMap<ObjId, u8>,
    /// `m_mapMonsterId`: the prop ids registered by an event (`map::insert`: the first event to register a prop
    /// owns it, KEEP).
    monster_ids: BTreeMap<MoverId, u8>,
    /// The bitset the combat and drop code read.
    facts: Arc<EventSpawnFacts>,
    /// Warnings already issued.
    warned: BTreeSet<String>,
}

impl EventSpawnService {
    /// The set of the registered mover props (a shared handle for `EventCombatHooks`).
    pub fn facts(&self) -> Arc<EventSpawnFacts> {
        Arc::clone(&self.facts)
    }

    /// `IsEventSpawnMonster(mover prop id)`.
    pub fn is_event_monster(&self, id: MoverId) -> bool {
        self.monster_ids.contains_key(&id)
    }

    /// Whether the object `obj` is a monster this service created and still tracks.
    pub fn is_event_spawn(&self, obj: ObjId) -> bool {
        self.spawned.contains_key(&obj)
    }

    /// The ids of the running spawn events, ascending.
    pub fn active_ids(&self) -> Vec<u8> {
        self.active.keys().copied().collect()
    }

    /// The ids whose monsters are being removed, in list order.
    pub fn ending_ids(&self) -> &[u8] {
        &self.ending
    }

    /// How many created monsters are tracked.
    pub fn spawned_len(&self) -> usize {
        self.spawned.len()
    }

    /// The spawn rectangles of Madrigal.
    pub fn regions(&self) -> &[[i32; 4]] {
        &self.regions
    }

    /// The entries of a running spawn event.
    pub fn entries(&self, id: u8) -> Option<&[SpawnEntry]> {
        self.active.get(&id).map(Vec::as_slice)
    }

    /// The tracked monsters of event `id`, ascending by object id.
    pub fn spawned_of(&self, id: u8) -> Vec<ObjId> {
        self.spawned
            .iter()
            .filter(|(_, e)| **e == id)
            .map(|(o, _)| *o)
            .collect()
    }

    fn warn_once(&mut self, key: String, text: &str) {
        if self.warned.insert(key.clone()) {
            tracing::warn!(key, "{text}");
        }
    }

    fn sync_facts(&self) {
        self.facts.replace_with(self.monster_ids.keys().copied());
    }

    /// `SetSpawnEvent(id, running)` (22 §4.10 step 1): see the module documentation. `scripts` is the view over
    /// the `Event` family that answers `GetSpawn(id)`.
    pub fn set_spawn_event(&mut self, scripts: &EventScripts<'_>, id: u8, running: bool) {
        let defs = if running {
            scripts.spawn(u32::from(id))
        } else {
            Vec::new()
        };
        if running && !defs.is_empty() {
            let mut entries = Vec::with_capacity(defs.len());
            for d in defs {
                let kind = match d.kind {
                    KIND_ITEM => SpawnKind::Item,
                    KIND_MONSTER => SpawnKind::Monster,
                    other => {
                        self.warn_once(
                            format!("spawn_kind:{id}:{other}:{}", d.name),
                            "an Event.lua spawn line has a kind other than ITEM or MONSTER: it is skipped",
                        );
                        continue;
                    }
                };
                if d.id == NULL_ID {
                    self.warn_once(
                        format!("spawn_unknown:{id}:{}", d.name),
                        "an Event.lua spawn line names an unknown define: it is skipped",
                    );
                    continue;
                }
                if d.per_day == 0 {
                    self.warn_once(
                        format!("spawn_per_day:{id}:{}", d.name),
                        "an Event.lua spawn line has nDayNum 0: it is skipped (the original divided by zero)",
                    );
                    continue;
                }
                if kind == SpawnKind::Monster {
                    // `map::insert`: the first event to register a prop keeps it
                    self.monster_ids.entry(d.id).or_insert(id);
                }
                entries.push(SpawnEntry {
                    kind,
                    id: d.id,
                    interval_ms: 86_400_000 / u64::from(d.per_day),
                    next_at: GameMs(0),
                });
            }
            self.active.insert(id, entries);
            self.ending.retain(|e| *e != id);
            self.sync_facts();
        } else if self.active.remove(&id).is_some() {
            self.ending.push(id);
        }
    }

    /// `SetSpawnEvent` for every change of a state pass, in the order the script returned them (22 §4.2 step 2).
    /// An event id above 255 does not exist for the C++ (`BYTE nId`): skipped with a warning.
    pub fn apply_changes(&mut self, scripts: &EventScripts<'_>, changes: &[EventChange]) {
        for c in changes {
            match u8::try_from(c.id) {
                Ok(id) => self.set_spawn_event(scripts, id, c.running),
                Err(_) => self.warn_once(
                    format!("spawn_event_id:{}", c.id),
                    "an Event.lua event id above 255 cannot have spawn lines: it is ignored",
                ),
            }
        }
    }

    /// Moves every running spawn event to the end list (a reload of `Event.lua`, a family that is not `Ready`):
    /// their monsters are removed over the next runs.
    pub fn end_all(&mut self) {
        let ids: Vec<u8> = self.active.keys().copied().collect();
        self.active.clear();
        for id in ids {
            if !self.ending.contains(&id) {
                self.ending.push(id);
            }
        }
    }

    /// Forgets the tracked monsters that are gone or dead (the C++ removed them at the death).
    fn sweep(&mut self, worlds: &Worlds) {
        self.spawned.retain(|obj, _| {
            let Some(core) = worlds.core(*obj) else {
                return false;
            };
            if core.deleted {
                return false;
            }
            worlds
                .mover(*obj)
                .is_some_and(|m| !crate::combat::state::is_die(m))
        });
    }

    /// `DeleteEndEventProcess` (see the module documentation).
    fn ending_pass(&mut self, worlds: &mut Worlds) {
        let mut i = 0;
        let mut changed = false;
        while i < self.ending.len() {
            let id = self.ending[i];
            // `100 / size` with the size as it stands now; 0 (more than 100 ids) means no cap (KEEP)
            let cap = DELETE_BUDGET / self.ending.len();
            let mut deleted = 0_usize;
            for obj in self.spawned_of(id) {
                worlds.delete(obj);
                self.spawned.remove(&obj);
                deleted += 1;
                if deleted == cap {
                    break;
                }
            }
            if deleted == 0 {
                self.monster_ids.retain(|_, e| *e != id);
                self.ending.remove(i);
                changed = true;
            } else {
                i += 1;
            }
        }
        if changed {
            self.sync_facts();
        }
    }
}

/// The start of the game (22 §4.10 step 2): sizes the prop set from the mover table, collects the regions of
/// Madrigal from the respawn lines `Worlds::init_respawn` accepted (file order, ground-walking mover props of level
/// `1..=MAX_MONSTER_LEVEL`) and applies the events that are running at the start. No world 1: no regions, one
/// warning, the spawns do nothing.
pub fn boot(
    event: &mut EventServices,
    lua: &LuaHost,
    data: &GameData,
    worlds: &Worlds,
    started: &[EventChange],
) {
    let max_id = data.movers.max_id().unwrap_or(0);
    let EventServices {
        lua_events, spawns, ..
    } = event;
    spawns.facts = Arc::new(EventSpawnFacts::sized(max_id));
    spawns.regions = match worlds.world(SPAWN_WORLD) {
        Some(w) => w
            .respawn
            .protos
            .iter()
            .filter(|p| p.kind == ObjKind::Mover && !p.flying)
            .filter(|p| {
                data.mover(p.index).is_some_and(|m| {
                    m.level > 0 && i64::from(m.level) <= i64::from(MAX_MONSTER_LEVEL)
                })
            })
            .map(|p| p.rect)
            .collect(),
        None => {
            tracing::warn!("Event.lua spawn events: world 1 is not loaded, nothing will spawn");
            Vec::new()
        }
    };
    let scripts = lua_events.scripts(lua, data);
    spawns.apply_changes(&scripts, started);
}

/// 1 s slot item 14 `event::spawns::process` (22 §2.3, §4.10): `EventSpawnProcess`.
pub fn process(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let ready = ctx.lua.family_status(ScriptFamily::Event) == FamilyStatus::Ready;
    let Services { serials, event, .. } = s;
    let sp = &mut event.spawns;
    if !ready {
        // 22 §2.5 rule 3: no spawn events while the family is down
        sp.end_all();
    }
    sp.sweep(ctx.worlds);
    if ready && !sp.regions.is_empty() && ctx.worlds.world(SPAWN_WORLD).is_some() {
        let ids: Vec<u8> = sp.active.keys().copied().collect();
        for id in ids {
            let count = sp.active.get(&id).map_or(0, Vec::len);
            for k in 0..count {
                let Some(entry) = sp.active.get_mut(&id).and_then(|v| v.get_mut(k)) else {
                    break;
                };
                if entry.next_at > now {
                    continue;
                }
                // `IsTimeOut`: the next run is `now + interval` (not `+=`, which would burst after the 0)
                entry.next_at = GameMs(now.0.saturating_add(entry.interval_ms));
                let entry = entry.clone();
                let rect = {
                    let rng = ctx.rng.event();
                    let i = rng.random_below(sp.regions.len() as u32) as usize;
                    sp.regions.get(i).copied().unwrap_or([0; 4])
                };
                let (x, z) = {
                    let rng = ctx.rng.event();
                    let x = x_random_range(rng, rect[0], rect[2]);
                    let z = x_random_range(rng, rect[1], rect[3]);
                    (x as f32, z as f32)
                };
                let Some(world) = ctx.worlds.world(SPAWN_WORLD) else {
                    break;
                };
                let y = world.statics.land_height(x, z) + 1.0;
                let pos = Vec3::new(x, y, z);
                match entry.kind {
                    SpawnKind::Item => {
                        spawn_item(ctx, serials, entry.id, pos, now);
                    }
                    SpawnKind::Monster => {
                        let spawn = MonsterSpawn {
                            mover: entry.id,
                            world: SPAWN_WORLD,
                            layer: SPAWN_LAYER,
                            pos,
                            active_attack: false,
                            gold: None,
                            owner_ai: None,
                        };
                        if let Some(obj) = commit(ctx.worlds, ctx.data, ctx.rng.event(), &spawn) {
                            sp.spawned.insert(obj, id);
                        }
                    }
                }
            }
        }
    }
    sp.ending_pass(ctx.worlds);
}

/// The item of an `ITEM` entry: one piece with the prop's endurance, a fresh serial, nobody's, kept 24 h.
fn spawn_item(
    ctx: &mut GameCtx<'_>,
    serials: &mut crate::persist::serial::SerialAllocator,
    item: ItemId,
    pos: Vec3,
    now: GameMs,
) {
    let Some(prop) = ctx.data.item(item) else {
        return;
    };
    let mut rec = ItemRecord::new(0, item, 1, 0);
    rec.endurance = prop.endurance as i32;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    match src.next_serial() {
        Ok(n) => rec.serial = n,
        Err(e) => {
            tracing::error!(item, error = %e, "Event.lua spawn: no serial for the item, it is not created");
            return;
        }
    }
    let meta = GroundItemMeta {
        owner: None,
        holder: None,
        drop_time: GameMs(now.0.saturating_add(ITEM_KEEP_MS)),
        from_monster: false,
        event_monster: None,
        gold: None,
        respawned: false,
    };
    if let Err(e) = ctx
        .worlds
        .spawn_ground_item(SPAWN_WORLD, SPAWN_LAYER, pos, 0.0, rec, meta)
    {
        tracing::warn!(item, ?e, "Event.lua spawn: the item was refused");
    }
}
