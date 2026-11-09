//! `#auto` idle speech (Plan/20 §8.3, D20-09, F-190; src:_Common/NpcProperty.cpp:12-31, Mover.cpp:1338-1347).
//!
//! Every NPC mover with a character key whose script has a function at key line 0 (`#auto`) gets a timer: the
//! first one is due `60 s + rng.below(30 000 ms)` after the NPC is first seen, afterwards every `15 s + 1 s *
//! rng.below(10)`. When `now > due` the next due time is drawn **first**, then the function runs. D20-09: the
//! function runs only if at least one player currently sees the NPC; the timer and its RNG draw advance either
//! way, so the sequence is the same with and without observers. `rng` is the NPC's world stream.
//!
//! The function runs with no player (`pc = None`): player helpers do nothing and return 0 (§6.8), `Speak(NpcId(),
//! n)` becomes a `ChatNtf` bubble to the NPC's viewers, `SetScriptTimer` is a no-op. A failing `#auto` marks the
//! script faulted (no message, §3.8).
//!
//! The scan is the 1 s slot item `npc_idle_speech` (`process`): timers are served at tick resolution
//! (<= 67 ms late, invisible). New NPCs are picked up by a sync pass every [`SYNC_EVERY_MS`].

use super::host::DialogHost;
use crate::ctx::{GameCtx, Services};
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, ObjId};
use std::collections::{BTreeMap, BTreeSet};

/// The first timer: base and random spread in milliseconds.
pub const FIRST_BASE_MS: u64 = 60_000;
/// See [`FIRST_BASE_MS`].
pub const FIRST_SPREAD_MS: u32 = 30_000;
/// The following timers: base in milliseconds.
pub const NEXT_BASE_MS: u64 = 15_000;
/// The following timers: `1000 * rng.below(NEXT_SPREAD_S)` is added.
pub const NEXT_SPREAD_S: u32 = 10;
/// How often the NPC set is scanned for NPCs without a timer.
pub const SYNC_EVERY_MS: u64 = 8_000;

/// The timers of the NPCs with an `#auto` function.
#[derive(Debug, Default)]
pub struct IdleSpeech {
    due: BTreeMap<ObjId, GameMs>,
    /// NPCs seen without an `#auto` function (never scheduled).
    skipped: BTreeSet<ObjId>,
    last_sync: Option<GameMs>,
    /// How many times `#auto` ran (tests, metrics).
    pub fired: u64,
}

impl IdleSpeech {
    /// The due time of an NPC's timer.
    pub fn due_of(&self, npc: ObjId) -> Option<GameMs> {
        self.due.get(&npc).copied()
    }

    /// The number of scheduled NPCs.
    pub fn len(&self) -> usize {
        self.due.len()
    }

    /// `true` when no NPC is scheduled.
    pub fn is_empty(&self) -> bool {
        self.due.is_empty()
    }
}

/// Slot item 18 `npc_idle_speech` (registered in `sched::registry`): picks up new NPCs, then serves the timers
/// that are due.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let Some(rt) = ctx.lua.npc_runtime() else {
        return;
    };
    if rt.dialog_keys().is_disabled() {
        return;
    }
    let now = ctx.clock.game_ms;
    sync(svc, ctx, &rt, now);
    // the timers that are due, in (due, id) order
    let mut due: Vec<(GameMs, ObjId)> = svc
        .npc
        .idle
        .due
        .iter()
        .filter(|(_, d)| now.0 > d.0)
        .map(|(id, d)| (*d, *id))
        .collect();
    due.sort_unstable();
    for (_, npc) in due {
        let Some(m) = ctx.worlds.mover(npc) else {
            svc.npc.idle.due.remove(&npc);
            continue;
        };
        let world = m.core.world;
        let observed = !m.core.viewers.is_empty();
        let draw = ctx.rng.world(world).random_below(NEXT_SPREAD_S);
        let next = GameMs(now.0 + NEXT_BASE_MS + 1000 * u64::from(draw));
        svc.npc.idle.due.insert(npc, next);
        if !observed {
            continue;
        }
        let Some(script) = script_of(ctx, &rt, npc) else {
            continue;
        };
        if rt.faulted(script) {
            continue;
        }
        svc.npc.idle.fired += 1;
        let Some(mut host) = DialogHost::new(svc, ctx, rt.clone(), None, npc, script) else {
            continue;
        };
        if let Err(e) = host.run_dialog("#auto", 0, 0) {
            tracing::error!(target: "lua", npc = npc.0, error = %e, "#auto failed");
        }
        // the ops of a player-less run are dropped: `Say`, `AddKey` ... do nothing without a player
    }
}

/// The script an NPC object is bound to.
fn script_of(
    ctx: &GameCtx<'_>,
    rt: &flyff_lua::NpcRuntime,
    npc: ObjId,
) -> Option<flyff_lua::NpcScriptId> {
    let key = &ctx.worlds.mover(npc)?.npc()?.character_key;
    let def = ctx.data.npc(key)?;
    rt.script(def.dialog_key.as_deref()?)
}

/// Schedules the NPCs that have an `#auto` function and no timer yet; forgets NPCs that are gone.
fn sync(svc: &mut Services, ctx: &mut GameCtx<'_>, rt: &flyff_lua::NpcRuntime, now: GameMs) {
    let idle = &mut svc.npc.idle;
    let due_for_sync = idle
        .last_sync
        .is_none_or(|t| now.0.saturating_sub(t.0) >= SYNC_EVERY_MS);
    if !due_for_sync {
        return;
    }
    idle.last_sync = Some(now);
    let store = ctx.worlds.store();
    idle.due.retain(|id, _| store.contains(*id));
    idle.skipped.retain(|id| store.contains(*id));
    let fresh: Vec<(ObjId, flyff_common::WorldId)> = store
        .ids()
        .filter(|id| !idle.due.contains_key(id) && !idle.skipped.contains(id))
        .filter_map(|id| {
            let m = store.mover(id)?;
            let npc = m.npc()?;
            (!npc.character_key.is_empty()).then_some((id, m.core.world))
        })
        .collect();
    for (id, world) in fresh {
        let has_auto = script_of(ctx, rt, id).is_some_and(|s| rt.has_key(s, 0));
        if !has_auto {
            svc.npc.idle.skipped.insert(id);
            continue;
        }
        let spread = ctx.rng.world(world).random_below(FIRST_SPREAD_MS);
        svc.npc
            .idle
            .due
            .insert(id, GameMs(now.0 + FIRST_BASE_MS + u64::from(spread)));
    }
}
