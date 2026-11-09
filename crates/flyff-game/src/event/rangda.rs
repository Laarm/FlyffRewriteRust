//! `event::rangda`: the random event monster Rangda (Plan/22 §7.2, WP-EVENT-09; legacy `CRangda` /
//! `CRangdaController`, `WORLDSERVER/rangda.cpp:9-228`, `randomeventmonster.inc`; ref/15 §8; F-186).
//!
//! `randomeventmonster.inc` lists four roaming monsters (`MI_RANGDA01..04`): each has a few fixed points in the
//! world, an `nInterval` (seconds until it comes back after it is gone) and an `nReplace` (seconds until an
//! UNTOUCHED monster is moved to another of its points). The controller runs once per second (1 s slot item 13,
//! `sched/registry.rs`, independent of viewers, of the AI sleep rule and of `events.respawn`, like the original's
//! `1SEC_Process`) and walks the definitions in FILE order, which is also the order of the random draws:
//!
//! 1. **valid object?** The monster is valid while the object exists, is not flagged deleted and still has the
//!    definition's prop. A corpse is still valid (it is damaged, so it never relocates; the generate countdown
//!    starts only when the corpse is gone). A gone object forgets the id.
//! 2. **no valid object** (`ProcessGenerate`): the generate countdown goes down by one if positive; at 0 the
//!    monster is created.
//! 3. **valid object** (`ProcessReplace`, only while replaceable): damaged (`GetHitPoint() < GetMaxHitPoint()`)
//!    ends the relocations for the rest of this monster's life; otherwise the replace countdown goes down by one if
//!    positive and at 0 the monster is deleted and created again at once at another point.
//!
//! **Create** (`CreateMonster`): the point is drawn first (one point: index 0 and the memory untouched; several:
//! `xRandom(0, n)` until it differs from the point the monster stood on), then the world is looked up, then the
//! monster is built from the `event` random stream (never the world stream, so no world golden shifts), stands
//! (default AI state IDLE, no respawn line, no summon registration) in layer 0 with the definition's
//! `bActiveAttack` and is added to the world. A missing world or layer, an unknown prop or a point outside the
//! world leaves the state as it is, so the next second retries with a new point (as the original does after
//! `GetWorld == NULL`); each definition warns once, afterwards only debug lines.
//!
//! The original announces nothing (`rangda.cpp` only writes `OutputDebugString`): spawns and relocations are
//! `info!` lines here. The state is memory only (22 E9): a restart spawns every Rangda again on the first run.
//!
//! **Kept quirks** (no ledger row): `nReplace = 0` relocates an undamaged Rangda every second; a negative
//! interval never respawns it after the first death; two definitions of one mover are two independent controllers;
//! a failed create after a relocation leaves the old (deleted) id, so the respawn then waits `nInterval` runs.
//!
//! Plan/22 §2.1 puts `RangdaService` into `event/monsters.rs`; the registered stub is this file and Rangda stays
//! here (NOTES).

use crate::ai::spawn::{MonsterSpawn, build};
use crate::ctx::{GameCtx, Services};
use crate::stats::StatsCore;
use crate::stats::env::{max_hp, point_param};
use crate::world::Worlds;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::events::rangda::RangdaDef;
use flyff_data::types::MoverId;
use flyff_rules::consts::dst::DST_HP;
use std::collections::BTreeSet;

/// `nDefaultLayer`: the layer every Rangda stands in.
const LAYER: LayerId = LayerId(0);

/// Upper bound of the point draws of one create: the original loops until the draw differs from the old point,
/// which a working stream does within a few draws. A stream that returned one value forever would hang the
/// server, so after this many equal draws the next other point is taken (never reached with the real streams).
const MAX_POINT_DRAWS: u32 = 1_000;

/// One controller: `CRangda`'s run-time members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangdaState {
    /// The index of the definition in [`RangdaService`]'s list (= the order of `GameData.rangda`).
    pub def: usize,
    /// `m_objid`: the current monster, `None` before the first spawn and after it is gone.
    pub obj: Option<ObjId>,
    /// `m_nOldPos`: the point the monster was created at (only kept for definitions with several points).
    pub last_point: Option<usize>,
    /// `m_nGenerateCountdown` (an `int` like the original: a negative interval never reaches 0).
    pub generate_countdown: i32,
    /// `m_nReplaceCountdown`.
    pub replace_countdown: i32,
    /// `m_bReplaceable`: false for the rest of the monster's life once it was damaged.
    pub replaceable: bool,
}

impl RangdaState {
    /// `CRangda::CRangda`: no monster, both countdowns 0 (so the first run spawns), replaceable.
    fn new(def: usize) -> RangdaState {
        RangdaState {
            def,
            obj: None,
            last_point: None,
            generate_countdown: 0,
            replace_countdown: 0,
            replaceable: true,
        }
    }
}

/// The read-only view of one controller ([`RangdaService::status`]): for tests, the admin console and the demo.
#[derive(Clone, Debug, PartialEq)]
pub struct RangdaStatus {
    /// The monster of the definition (`MI_RANGDA01..04`).
    pub mover: MoverId,
    /// The monster's object while it stands (a stale or deleted id is not shown).
    pub obj: Option<ObjId>,
    /// Its world.
    pub world: Option<WorldId>,
    /// Its position.
    pub pos: Option<Vec3>,
    /// Seconds (runs) until it comes back after it is gone.
    pub generate_countdown: i32,
    /// Runs until an undamaged monster is moved.
    pub replace_countdown: i32,
    /// `false` once the monster was damaged.
    pub replaceable: bool,
}

/// `CRangdaController` (`Services.event.rangda`): the definitions of `randomeventmonster.inc` and one
/// [`RangdaState`] per usable definition, in file order.
#[derive(Clone, Debug, Default)]
pub struct RangdaService {
    defs: Vec<RangdaDef>,
    states: Vec<RangdaState>,
    /// The definitions that already logged a refused create at `warn` level.
    warned: BTreeSet<usize>,
}

impl RangdaService {
    /// The controllers of `defs` in file order. A definition without a point is skipped with one warning (the
    /// original asserts and would divide by zero in `GetRandomPos`).
    pub fn new(defs: &[RangdaDef]) -> RangdaService {
        let mut states = Vec::with_capacity(defs.len());
        for (i, d) in defs.iter().enumerate() {
            if d.points.is_empty() {
                tracing::warn!(
                    mover = d.mover,
                    "randomeventmonster.inc: a Rangda without a position is skipped"
                );
                continue;
            }
            states.push(RangdaState::new(i));
        }
        RangdaService {
            defs: defs.to_vec(),
            states,
            warned: BTreeSet::new(),
        }
    }

    /// The controllers in file order.
    pub fn states(&self) -> &[RangdaState] {
        &self.states
    }

    /// The definition behind `state`.
    pub fn definition(&self, state: &RangdaState) -> Option<&RangdaDef> {
        self.defs.get(state.def)
    }

    /// The view of every controller in file order.
    pub fn status(&self, worlds: &Worlds) -> Vec<RangdaStatus> {
        self.states
            .iter()
            .filter_map(|st| {
                let def = self.defs.get(st.def)?;
                let live = st
                    .obj
                    .and_then(|id| valid_object(worlds, id, def.mover).map(|m| (id, m)));
                Some(RangdaStatus {
                    mover: def.mover,
                    obj: live.map(|(id, _)| id),
                    world: live.map(|(_, m)| m.core.world),
                    pos: live.map(|(_, m)| m.core.pos),
                    generate_countdown: st.generate_countdown,
                    replace_countdown: st.replace_countdown,
                    replaceable: st.replaceable,
                })
            })
            .collect()
    }
}

/// The monster `id` when it is a valid Rangda object of `mover`: it exists, is not flagged deleted and has the
/// definition's prop (`IsValidObj(prj.GetMover(m_objid))`; a corpse is valid until it is deleted).
fn valid_object(worlds: &Worlds, id: ObjId, mover: MoverId) -> Option<&crate::mover::Mover> {
    worlds
        .mover(id)
        .filter(|m| !m.core.deleted && m.prop_id() == mover)
}

/// 1 s slot item 13 `event::rangda::process` (22 §2.3, §7.2): `CRangdaController::OnTimer`, every definition in
/// file order.
pub fn process(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let Services { event, stats, .. } = s;
    let RangdaService {
        defs,
        states,
        warned,
    } = &mut event.rangda;
    let core = stats.core();
    for st in states.iter_mut() {
        let Some(def) = defs.get(st.def) else {
            continue;
        };
        let mut cx = RunCx {
            worlds: &mut *ctx.worlds,
            rng: ctx.rng.event(),
            data: ctx.data,
            core,
            warned: &mut *warned,
        };
        run(st, def, &mut cx);
    }
}

/// What one controller run works on.
struct RunCx<'a> {
    worlds: &'a mut Worlds,
    rng: &'a mut dyn GameRng,
    data: &'a GameData,
    core: &'a StatsCore,
    warned: &'a mut BTreeSet<usize>,
}

/// `CRangda::OnTimer`.
fn run(st: &mut RangdaState, def: &RangdaDef, cx: &mut RunCx<'_>) {
    // `GetMonster`: an invalid object forgets its id
    let live = st
        .obj
        .filter(|&id| valid_object(cx.worlds, id, def.mover).is_some());
    if live.is_none() {
        st.obj = None;
    }
    match live {
        // `ProcessReplace`
        Some(id) => {
            if !st.replaceable {
                return;
            }
            // `IsDamaged`: `GetHitPoint() < GetMaxHitPoint()`
            let damaged = valid_object(cx.worlds, id, def.mover)
                .is_some_and(|m| point_param(m, cx.core, DST_HP) < max_hp(m, cx.core));
            if damaged {
                st.replaceable = false;
                return;
            }
            // `HavetoReplace`
            if st.replace_countdown > 0 {
                st.replace_countdown -= 1;
            }
            if st.replace_countdown == 0 {
                cx.worlds.delete(id);
                create(st, def, cx, "Rangda relocated");
            }
        }
        // `ProcessGenerate`, `HavetoGenerate`
        None => {
            if st.generate_countdown > 0 {
                st.generate_countdown -= 1;
            }
            if st.generate_countdown == 0 {
                create(st, def, cx, "Rangda spawned");
            }
        }
    }
}

/// `CRangda::CreateMonster`.
fn create(st: &mut RangdaState, def: &RangdaDef, cx: &mut RunCx<'_>, what: &'static str) {
    let (point, remember) = random_point(def.points.len(), st.last_point, &mut *cx.rng);
    if remember {
        st.last_point = Some(point);
    }
    let Some(&(world_id, p)) = def.points.get(point) else {
        return;
    };
    let (world, pos) = (WorldId(world_id), Vec3::new(p[0], p[1], p[2]));
    let key = st.def;
    let refused = |warned: &mut BTreeSet<usize>, why: &str| {
        if warned.insert(key) {
            tracing::warn!(
                mover = def.mover,
                world = world_id,
                x = p[0],
                y = p[1],
                z = p[2],
                why,
                "Rangda: the monster cannot be created, the next run tries again"
            );
        } else {
            tracing::debug!(
                mover = def.mover,
                world = world_id,
                why,
                "Rangda: the monster cannot be created"
            );
        }
    };
    // `GetWorld` first, as the original, then `CreateObj`
    if cx.worlds.world(world).is_none() {
        return refused(cx.warned, "the world is not loaded");
    }
    if cx.worlds.layer(world, LAYER).is_none() {
        return refused(cx.warned, "the world has no layer 0");
    }
    let spec = MonsterSpawn {
        mover: def.mover,
        world,
        layer: LAYER,
        pos,
        active_attack: def.active_attack,
        gold: None,
        owner_ai: None,
    };
    let Some(m) = build(cx.data, &mut *cx.rng, &spec) else {
        return refused(cx.warned, "the monster has no prop");
    };
    match cx.worlds.spawn_mover(world, LAYER, m) {
        Ok(id) => {
            st.obj = Some(id);
            st.generate_countdown = def.interval_s;
            st.replace_countdown = def.replace_s;
            st.replaceable = true;
            tracing::info!(
                mover = def.mover,
                world = world_id,
                x = p[0],
                y = p[1],
                z = p[2],
                "{what}"
            );
        }
        Err(e) => refused(cx.warned, &e.to_string()),
    }
}

/// `CRangda::GetRandomPos`: the index of the point to use and whether it is remembered. One point: index 0 and
/// nothing is drawn or remembered. Several: `xRandom(0, n)` until it differs from the point the monster stands on.
fn random_point(n: usize, last: Option<usize>, rng: &mut dyn GameRng) -> (usize, bool) {
    if n <= 1 {
        return (0, false);
    }
    let bound = u32::try_from(n).unwrap_or(u32::MAX);
    for _ in 0..MAX_POINT_DRAWS {
        let k = rng.random_range(0, bound) as usize;
        if Some(k) != last {
            return (k, true);
        }
    }
    // a degenerate stream: the first other point
    let k = usize::from(last == Some(0));
    (k, true)
}
